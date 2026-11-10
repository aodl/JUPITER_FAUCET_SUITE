//! Idempotent stake refresh only. This journal never dispatches a Ledger transfer.
use super::{discovery, storage, types::*};
use candid::{CandidType, Deserialize};
use ic_cdk::call::Call;
use ic_stable_structures::{memory_manager::MemoryId, StableBTreeMap};
use jupiter_ic_clients::ledger::IcrcLedgerCanister;
use serde::Serialize;
use std::cell::RefCell;

type Memory =
    ic_stable_structures::memory_manager::VirtualMemory<ic_stable_structures::DefaultMemoryImpl>;
type Map = StableBTreeMap<u64, Vec<u8>, Memory>;
thread_local! {
    static JOBS: RefCell<Option<Map>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<StableBTreeMap<u64, u8, Memory>>> = const { RefCell::new(None) };
}
const ATTEMPTS: u8 = 5;
#[derive(Debug)]
enum RefreshError {
    Retry(String),
    Manual(String),
}
impl From<String> for RefreshError {
    fn from(error: String) -> Self {
        Self::Retry(error)
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub enum RefreshStatus {
    Pending,
    Confirmed,
    ManualReview(String),
}
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct RefreshJob {
    pub contribution_block: u64,
    pub attempt: u64,
    pub attempts_remaining: u8,
    pub next_attempt_seconds: u64,
    pub status: RefreshStatus,
}
fn jobs<R>(f: impl FnOnce(&mut Map) -> R) -> R {
    JOBS.with_borrow_mut(|map| {
        f(map.get_or_insert_with(|| {
            StableBTreeMap::init(crate::state::programme_memory(MemoryId::new(16)))
        }))
    })
}
fn pending<R>(f: impl FnOnce(&mut StableBTreeMap<u64, u8, Memory>) -> R) -> R {
    PENDING.with_borrow_mut(|map| {
        f(map.get_or_insert_with(|| {
            StableBTreeMap::init(crate::state::programme_memory(MemoryId::new(17)))
        }))
    })
}
pub fn read(block: u64) -> Option<RefreshJob> {
    jobs(|map| {
        map.get(&block)
            .map(|bytes| candid::decode_one(&bytes).expect("stake refresh journal"))
    })
}
fn put(job: &RefreshJob) {
    jobs(|map| {
        map.insert(
            job.contribution_block,
            candid::encode_one(job).expect("encode stake refresh"),
        )
    });
    pending(|map| {
        if job.status == RefreshStatus::Pending {
            map.insert(job.contribution_block, 0);
        } else {
            map.remove(&job.contribution_block);
        }
    });
}
pub fn enqueue(record: &PatronRecord) {
    if record.compounded_neuron.is_none()
        || !matches!(
            record.outcome,
            PatronOutcome::Verified { .. } | PatronOutcome::AssumedPaidUnverified { .. }
        )
        || record.refresh_result.as_deref() == Some("confirmed stake refresh")
        || read(record.contribution_block).is_some()
    {
        return;
    }
    put(&RefreshJob {
        contribution_block: record.contribution_block,
        attempt: 0,
        attempts_remaining: ATTEMPTS,
        next_attempt_seconds: 0,
        status: RefreshStatus::Pending,
    });
}

/// Only the originally pinned jUP Governance may authorize another bounded
/// refresh window. Neither payment identity nor neuron selection is editable.
pub fn retry(block: u64, caller: candid::Principal) -> Result<(), String> {
    let record = storage::record(block).ok_or("patron record absent")?;
    let governance = record
        .ownership_snapshot
        .sources
        .first()
        .ok_or("missing pinned jUP source")?
        .governance;
    if caller == candid::Principal::anonymous() || caller != governance {
        return Err("only the pinned jUP Governance may retry stake refresh".into());
    }
    let mut job = read(block).ok_or("refresh journal absent")?;
    if !matches!(job.status, RefreshStatus::ManualReview(_)) {
        return Err("only a stopped refresh is eligible for reviewed retry".into());
    }
    job.status = RefreshStatus::Pending;
    job.attempts_remaining = ATTEMPTS;
    job.next_attempt_seconds = 0;
    put(&job);
    Ok(())
}

pub async fn tick(now: u64) -> Result<(), String> {
    // One bounded attempt per programme tick. The oldest job can delay later
    // jobs only through its finite retry window, never indefinitely.
    let Some(block) = pending(|map| map.first_key_value().map(|(block, _)| block)) else {
        return Ok(());
    };
    let mut job = read(block).ok_or("missing stake refresh journal")?;
    if job.status != RefreshStatus::Pending || now < job.next_attempt_seconds {
        return Ok(());
    }
    let record = storage::record(block).ok_or("missing paid patron record")?;
    if job.attempts_remaining == 0 {
        finish_attempt(
            &job,
            &record,
            Err(RefreshError::Manual(
                "final bounded refresh attempt was interrupted; reviewed retry required".into(),
            )),
        );
        return Ok(());
    }
    let id = record
        .compounded_neuron
        .clone()
        .ok_or("missing pinned compound neuron")?;
    let source = record
        .ownership_snapshot
        .sources
        .first()
        .ok_or("missing pinned source")?;
    if !matches!(
        record.outcome,
        PatronOutcome::Verified { .. } | PatronOutcome::AssumedPaidUnverified { .. }
    ) || record.to.owner != source.governance
        || record.to.subaccount.map(|s| s.to_vec()) != Some(id.clone())
        || record.ledger != source.ledger
    {
        return Err("stake refresh/payment identity mismatch".into());
    }
    job.attempt = job
        .attempt
        .checked_add(1)
        .ok_or("refresh attempt exhausted")?;
    job.attempts_remaining = job.attempts_remaining.saturating_sub(1);
    // Persist before either read or refresh: upgrade safely retries this idempotent
    // operation, and a duplicate callback cannot own a successor attempt.
    job.next_attempt_seconds = now.saturating_add(300);
    put(&job);
    let result = async {
        let neuron = discovery::get_neuron(source.governance, id.clone()).await?;
        if neuron.id.as_ref().map(|n| &n.id) != Some(&id) {
            return Err(RefreshError::Manual(
                "get_neuron returned a different neuron".into(),
            ));
        }
        let balance = IcrcLedgerCanister::new(record.ledger)
            .balance_of_e8s(record.to)
            .await
            .map_err(|e| e.to_string())?;
        if neuron.cached_neuron_stake_e8s == balance {
            return Ok(());
        }
        refresh(source.governance, id).await
    }
    .await;
    finish_attempt(&job, &record, result);
    Ok(())
}

fn finish_attempt(expected: &RefreshJob, record: &PatronRecord, result: Result<(), RefreshError>) {
    if read(expected.contribution_block).as_ref() != Some(expected)
        || storage::record(expected.contribution_block).as_ref() != Some(record)
    {
        return;
    }
    let mut job = expected.clone();
    let mut record = record.clone();
    match result {
        Ok(()) => {
            job.status = RefreshStatus::Confirmed;
            record.refresh_result = Some("confirmed stake refresh".into());
        }
        Err(error) => {
            let permanent = matches!(error, RefreshError::Manual(_));
            let (RefreshError::Retry(error) | RefreshError::Manual(error)) = error;
            let error = super::compact(&error);
            // A typed Governance refusal is definitive; transport/decode failures
            // get a bounded retry. Both remain observable for governed resolution.
            if permanent || job.attempts_remaining == 0 {
                job.status = RefreshStatus::ManualReview(error.clone());
            }
            record.refresh_result = Some(error);
        }
    }
    put(&job);
    storage::put(&record);
}

async fn refresh(governance: candid::Principal, id: Vec<u8>) -> Result<(), RefreshError> {
    use crate::generated as sns;
    let expected_id = id.clone();
    let response: sns::ManageNeuronResponse = Call::bounded_wait(governance, "manage_neuron")
        .with_arg(sns::ManageNeuron {
            subaccount: id,
            command: Some(sns::Command::ClaimOrRefresh(sns::ClaimOrRefresh {
                by: Some(sns::By::NeuronId {}),
            })),
        })
        .change_timeout(60)
        .await
        .map_err(|e| format!("stake refresh uncertain: {e:?}"))?
        .candid()
        .map_err(|e| format!("stake refresh decode: {e:?}"))?;
    match response.command {
        Some(sns::Command1::ClaimOrRefresh(reply))
            if reply
                .refreshed_neuron_id
                .as_ref()
                .is_some_and(|n| n.id == expected_id) =>
        {
            Ok(())
        }
        Some(sns::Command1::ClaimOrRefresh(_)) => Err(RefreshError::Manual(
            "stake refresh returned a different neuron identity".into(),
        )),
        Some(sns::Command1::Error(e)) => {
            let message = format!(
                "stake refresh rejected: {}: {}",
                e.error_type, e.error_message
            );
            // Pinned official Governance ErrorType: resource exhaustion (9),
            // external dependency (11) and ledger-update-in-progress (12) may
            // recover. All other typed refusals stop for explicit review.
            Err(if matches!(e.error_type, 9 | 11 | 12) {
                RefreshError::Retry(message)
            } else {
                RefreshError::Manual(message)
            })
        }
        _ => Err(RefreshError::Retry(
            "stake refresh decode: unexpected response".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use candid::Principal;
    use icrc_ledger_types::icrc1::account::Account;
    fn record() -> PatronRecord {
        let source = Source {
            root: Principal::from_slice(&[1]),
            governance: Principal::from_slice(&[2]),
            ledger: Principal::from_slice(&[3]),
        };
        let snapshot = Snapshot {
            id: 7,
            config_version: 1,
            sources: vec![source.clone()],
            started_at_seconds: 1,
            completed_at_seconds: 2,
            slot: 0,
            owners: 1,
        };
        PatronRecord {
            contribution_block: 99,
            snapshot_id: 7,
            ownership_snapshot: snapshot,
            recognition: None,
            config_version: 1,
            contributor_icp_account: vec![1; 32],
            commitment_timestamp_seconds: 1,
            qualifying_icp_e8s: 100_000_000,
            launch_timestamp_seconds: 1,
            compounded_neuron: Some(vec![7; 32]),
            ledger: source.ledger,
            from: Account {
                owner: Principal::from_slice(&[4]),
                subaccount: None,
            },
            to: Account {
                owner: source.governance,
                subaccount: Some([7; 32]),
            },
            amount_e8s: 200_000_000,
            fee_e8s: 10_000_000,
            memo: vec![9],
            created_at_time_nanos: 123,
            outcome: PatronOutcome::Verified {
                block: 42_u64.into(),
                duplicate: false,
            },
            refresh_result: None,
        }
    }
    #[test]
    fn confirmed_and_assumed_payments_keep_identity_across_refresh_failure_and_duplicate_callbacks()
    {
        for outcome in [
            PatronOutcome::Verified {
                block: 42_u64.into(),
                duplicate: false,
            },
            PatronOutcome::AssumedPaidUnverified {
                reason: "timeout; payment not invented".into(),
            },
        ] {
            jobs(|m| m.clear_new());
            pending(|m| m.clear_new());
            let mut record = record();
            record.outcome = outcome;
            storage::put(&record);
            enqueue(&record);
            let mut first = read(99).unwrap();
            first.attempt = 1;
            first.attempts_remaining -= 1;
            first.next_attempt_seconds = 300;
            put(&first);
            finish_attempt(
                &first,
                &record,
                Err(RefreshError::Retry("transport unavailable".into())),
            );
            let saved = storage::record(99).unwrap();
            assert_eq!(saved.outcome, record.outcome);
            assert_eq!(saved.to, record.to);
            assert_eq!(saved.memo, record.memo);
            assert_eq!(saved.created_at_time_nanos, 123);
            // Stale callback with an old record cannot overwrite failure evidence.
            finish_attempt(&first, &record, Ok(()));
            assert_eq!(storage::record(99).unwrap(), saved);
            let mut second = read(99).unwrap();
            second.attempt += 1;
            second.attempts_remaining -= 1;
            second.next_attempt_seconds += 300;
            put(&second);
            finish_attempt(&first, &saved, Ok(()));
            assert_eq!(read(99).unwrap(), second);
            finish_attempt(&second, &saved, Ok(()));
            assert_eq!(read(99).unwrap().status, RefreshStatus::Confirmed);
            assert!(pending(|m| m.is_empty()));
            assert_eq!(storage::record(99).unwrap().outcome, record.outcome);
            enqueue(&record);
            assert_eq!(read(99).unwrap().attempt, 2);
        }
    }
    #[test]
    fn upgrade_after_last_admission_cannot_create_a_sixth_attempt() {
        jobs(|m| m.clear_new());
        pending(|m| m.clear_new());
        let record = record();
        storage::put(&record);
        enqueue(&record);
        let mut job = read(99).unwrap();
        job.attempt = 5;
        job.attempts_remaining = 0;
        job.next_attempt_seconds = 300;
        put(&job);
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(tick(301))
            .unwrap();
        let stopped = read(99).unwrap();
        assert_eq!(stopped.attempt, 5);
        assert!(matches!(stopped.status, RefreshStatus::ManualReview(_)));
        assert!(pending(|m| m.is_empty()));
        assert_eq!(storage::record(99).unwrap().outcome, record.outcome);
    }
    #[test]
    fn unsupported_and_exhausted_refreshes_stop_and_require_pinned_governance() {
        for permanent in [false, true] {
            jobs(|m| m.clear_new());
            pending(|m| m.clear_new());
            let record = record();
            storage::put(&record);
            enqueue(&record);
            let mut job = read(99).unwrap();
            job.attempt = 5;
            job.attempts_remaining = 0;
            put(&job);
            finish_attempt(
                &job,
                &record,
                Err(if permanent {
                    RefreshError::Manual("unsupported".into())
                } else {
                    RefreshError::Retry("timeout".into())
                }),
            );
            assert!(matches!(
                read(99).unwrap().status,
                RefreshStatus::ManualReview(_)
            ));
            assert!(pending(|m| m.is_empty()));
            assert!(retry(99, Principal::anonymous()).is_err());
            retry(99, record.to.owner).unwrap();
            let next = read(99).unwrap();
            assert_eq!(next.attempt, 5);
            assert_eq!(next.attempts_remaining, ATTEMPTS);
            assert!(retry(99, record.to.owner).is_err());
        }
    }
}
