//! One bounded dispatch record, separate from the byte-exact current V1 root.
//! Missing/mismatched evidence is always unresolved, including on older state.
use super::*;

#[derive(CandidType, Deserialize, Clone, Default)]
struct DispatchState(Option<DispatchRecord>);

#[derive(CandidType, Deserialize, Clone)]
struct DispatchRecord {
    job_id: u64,
    pending: PendingTransfer,
    dispatched: bool,
}

impl Storable for DispatchState {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("encode faucet dispatch provenance"))
    }

    fn into_bytes(self) -> Vec<u8> {
        self.to_bytes().into_owned()
    }

    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("decode faucet dispatch provenance")
    }

    const BOUND: Bound = Bound::Unbounded;
}

thread_local! {
    static DISPATCH: std::cell::RefCell<Option<StableCell<DispatchState, Memory>>> =
        const { std::cell::RefCell::new(None) };
}

fn with_dispatch<R>(f: impl FnOnce(&mut StableCell<DispatchState, Memory>) -> R) -> R {
    DISPATCH.with(|cell| {
        if cell.borrow().is_none() {
            // Faucet memory 0 is the unchanged V1 root; 1 is the skip-range
            // map. Memory 2 is newly allocated, not a retired memory ID.
            MEMORY_MANAGER.with(|manager| {
                let memory = manager.borrow().get(MemoryId::new(2));
                *cell.borrow_mut() = Some(StableCell::init(memory, DispatchState::default()));
            });
        }
        f(cell.borrow_mut().as_mut().expect("faucet dispatch cell"))
    })
}

pub(crate) fn pending_transfer_may_have_dispatched(job_id: u64, pending: &PendingTransfer) -> bool {
    with_dispatch(|cell| match &cell.get().0 {
        Some(record) if record.job_id == job_id && record.pending == *pending => record.dispatched,
        _ => true,
    })
}

// Callers stage/update this synchronously with the matching pending root state,
// without an intervening await. ICP commits both memories in the same message.
pub(crate) fn set_pending_transfer_dispatch(
    job_id: u64,
    pending: &PendingTransfer,
    dispatched: bool,
) {
    with_dispatch(|cell| {
        cell.set(DispatchState(Some(DispatchRecord {
            job_id,
            pending: pending.clone(),
            dispatched,
        })));
    });
}

pub(crate) fn clear_pending_transfer_dispatch() {
    with_dispatch(|cell| {
        cell.set(DispatchState::default());
    });
}

#[cfg(test)]
pub(super) fn evict_heap_cache_for_test() {
    DISPATCH.with(|cell| *cell.borrow_mut() = None);
}
