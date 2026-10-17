//! Bounded transient permission to do factory work, not a durable setup or
//! authorization to repeat an irreversible operation. The entry journal remains
//! authoritative when a lease expires or an upgrade interrupts a call.
use super::{RelaySetupKey, RelaySetupNotifyResult};
use std::cell::RefCell;
use std::future::Future;

const NEW_SLOTS: usize = 4;
const COMPLETION_SLOTS: usize = 2;
const WINDOW_SECONDS: u64 = 60;
const ATTEMPTS_PER_WINDOW: usize = 8;
const LEASE_SECONDS: u64 = 300;
const CALLS_PER_ATTEMPT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lane {
    New,
    Completion,
}

#[derive(Default)]
struct Window {
    number: u64,
    used: usize,
}

struct Slot {
    key: RelaySetupKey,
    token: u64,
    lane: Lane,
    expires: u64,
    calls: usize,
}

#[derive(Default)]
struct Limiter {
    next_token: u64,
    slots: Vec<Slot>,
    new: Window,
    completion: Window,
}

thread_local! { static LIMITER: RefCell<Limiter> = RefCell::new(Limiter::default()); }

#[cfg(feature = "debug_api")]
thread_local! { static TRAP_AFTER_CALL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

#[cfg(feature = "debug_api")]
pub(super) fn debug_trap_after_factory_call(value: bool) {
    TRAP_AFTER_CALL.with(|flag| flag.set(value));
}

#[cfg(feature = "debug_api")]
pub(super) fn debug_callback_boundary() {
    if TRAP_AFTER_CALL.with(|flag| flag.get()) {
        ic_cdk::trap("debug factory callback interruption");
    }
}

#[cfg(test)]
thread_local! { static TEST_SECONDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { static LAST_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
pub(super) fn last_calls_for_test() -> usize {
    LAST_CALLS.with(|count| count.get())
}

#[cfg(test)]
pub(super) fn set_time_for_test(seconds: u64) {
    TEST_SECONDS.with(|clock| clock.set(seconds));
}

#[cfg(test)]
pub(super) fn time_for_test() -> u64 {
    now()
}

fn now() -> u64 {
    #[cfg(test)]
    {
        TEST_SECONDS.with(|clock| clock.get())
    }
    #[cfg(not(test))]
    {
        super::now_secs()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Ticket {
    key: RelaySetupKey,
    token: u64,
}

pub(super) struct Guard(Ticket);

impl Guard {
    pub(super) fn acquire(key: RelaySetupKey, lane: Lane) -> Option<Self> {
        let now = now();
        LIMITER.with(|limiter| {
            let mut limiter = limiter.borrow_mut();
            limiter.slots.retain(|slot| slot.expires > now);
            let cap = match lane {
                Lane::New => NEW_SLOTS,
                Lane::Completion => COMPLETION_SLOTS,
            };
            if limiter.slots.iter().any(|slot| slot.key == key)
                || limiter
                    .slots
                    .iter()
                    .filter(|slot| slot.lane == lane)
                    .count()
                    >= cap
            {
                return None;
            }
            let window = match lane {
                Lane::New => &mut limiter.new,
                Lane::Completion => &mut limiter.completion,
            };
            let number = now / WINDOW_SECONDS;
            if number < window.number {
                return None;
            }
            if number > window.number {
                *window = Window { number, used: 0 };
            }
            if window.used >= ATTEMPTS_PER_WINDOW {
                return None;
            }
            window.used += 1; // Failure/early return never refunds attempted work.
            limiter.next_token = limiter
                .next_token
                .checked_add(1)
                .expect("factory admission generation exhausted");
            let ticket = Ticket {
                key,
                token: limiter.next_token,
            };
            limiter.slots.push(Slot {
                key,
                token: ticket.token,
                lane,
                expires: now.saturating_add(LEASE_SECONDS),
                calls: 0,
            });
            Some(Self(ticket))
        })
    }

    pub(super) fn ticket(&self) -> Ticket {
        self.0
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        LIMITER.with(|limiter| {
            #[cfg(test)]
            if let Some(slot) = limiter
                .borrow()
                .slots
                .iter()
                .find(|slot| slot.key == self.0.key && slot.token == self.0.token)
            {
                LAST_CALLS.with(|count| count.set(slot.calls));
            }
            limiter
                .borrow_mut()
                .slots
                .retain(|slot| slot.key != self.0.key || slot.token != self.0.token)
        });
    }
}

impl Ticket {
    fn visit(self, charge_call: bool) -> Result<(), RelaySetupNotifyResult> {
        LIMITER.with(|limiter| {
            let mut limiter = limiter.borrow_mut();
            let slot = limiter
                .slots
                .iter_mut()
                .find(|slot| slot.key == self.key && slot.token == self.token)
                .filter(|slot| slot.expires > now())
                .ok_or(RelaySetupNotifyResult::Busy)?;
            if charge_call {
                if slot.calls >= CALLS_PER_ATTEMPT {
                    return Err(RelaySetupNotifyResult::Busy);
                }
                slot.calls += 1;
            }
            Ok(())
        })
    }

    pub(super) fn check(self) -> Result<(), RelaySetupNotifyResult> {
        self.visit(false)
    }

    pub(super) async fn call<T>(
        self,
        future: impl Future<Output = T>,
    ) -> Result<T, RelaySetupNotifyResult> {
        self.visit(true)?;
        let result = future.await;
        #[cfg(feature = "debug_api")]
        debug_callback_boundary();
        self.check()?;
        Ok(result)
    }
}

#[cfg(any(test, feature = "debug_api"))]
pub(super) fn reset_for_debug() {
    LIMITER.with(|limiter| {
        let next_token = limiter.borrow().next_token;
        *limiter.borrow_mut() = Limiter {
            next_token,
            ..Default::default()
        };
    });
    #[cfg(feature = "debug_api")]
    TRAP_AFTER_CALL.with(|flag| flag.set(false));
}

#[cfg(test)]
pub(super) fn reset_for_test() {
    reset_for_debug();
    TEST_SECONDS.with(|clock| clock.set(0));
    LAST_CALLS.with(|count| count.set(0));
}

// Charge/fence the *nested* Auto discovery calls, not just the outer probe.
pub(super) struct Probe<'a, C> {
    pub inner: &'a C,
    pub ticket: Ticket,
}

impl<C: jupiter_ic_clients::cycles_probe::CyclesProbeClient> Probe<'_, C> {
    async fn call<T>(
        &self,
        future: impl Future<Output = T>,
    ) -> Result<T, jupiter_ic_clients::ClientError> {
        self.ticket.call(future).await.map_err(|_| {
            jupiter_ic_clients::ClientError::Call("factory admission expired or exhausted".into())
        })
    }
}

impl<C: jupiter_ic_clients::cycles_probe::CyclesProbeClient>
    jupiter_ic_clients::cycles_probe::CyclesProbeClient for Probe<'_, C>
{
    async fn self_cycles(&self, target: candid::Principal) -> Option<u128> {
        self.ticket.check().ok()?;
        let value = self.inner.self_cycles(target).await;
        self.ticket.check().ok()?;
        value
    }
    async fn direct_canister_status(
        &self,
        target: candid::Principal,
    ) -> Result<
        jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation,
        jupiter_ic_clients::ClientError,
    > {
        self.call(self.inner.direct_canister_status(target)).await?
    }
    async fn blackhole_cycles(
        &self,
        probe: candid::Principal,
        target: candid::Principal,
    ) -> Result<u128, jupiter_ic_clients::ClientError> {
        self.call(self.inner.blackhole_cycles(probe, target))
            .await?
    }
    async fn nns_root_cycles(
        &self,
        target: candid::Principal,
    ) -> Result<u128, jupiter_ic_clients::ClientError> {
        self.call(self.inner.nns_root_cycles(target)).await?
    }
    async fn list_deployed_snses(
        &self,
    ) -> Result<jupiter_ic_clients::sns::ListDeployedSnsesResponse, jupiter_ic_clients::ClientError>
    {
        self.call(self.inner.list_deployed_snses()).await?
    }
    async fn canister_info_controllers(
        &self,
        target: candid::Principal,
    ) -> Result<Vec<candid::Principal>, jupiter_ic_clients::ClientError> {
        self.call(self.inner.canister_info_controllers(target))
            .await?
    }
    async fn list_sns_canisters(
        &self,
        root: candid::Principal,
    ) -> Result<jupiter_ic_clients::sns::ListSnsCanistersResponse, jupiter_ic_clients::ClientError>
    {
        self.call(self.inner.list_sns_canisters(root)).await?
    }
    async fn sns_root_cycles(
        &self,
        root: candid::Principal,
        target: candid::Principal,
    ) -> Result<u128, jupiter_ic_clients::ClientError> {
        self.call(self.inner.sns_root_cycles(root, target)).await?
    }
    async fn sns_swap_cycles(
        &self,
        swap: candid::Principal,
    ) -> Result<u128, jupiter_ic_clients::ClientError> {
        self.call(self.inner.sns_swap_cycles(swap)).await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    fn key(n: u8) -> RelaySetupKey {
        RelaySetupKey::from_canonical_configuration(&[candid::Principal::from_slice(&[n])], &[])
    }

    #[test]
    fn slots_and_key_protection_are_bounded_across_both_lanes() {
        reset_for_test();
        let guards = (1..=NEW_SLOTS)
            .map(|n| Guard::acquire(key(n as u8), Lane::New).unwrap())
            .collect::<Vec<_>>();
        assert!(Guard::acquire(key(1), Lane::Completion).is_none());
        assert!(Guard::acquire(key(9), Lane::New).is_none());
        let completions = (10..10 + COMPLETION_SLOTS)
            .map(|n| Guard::acquire(key(n as u8), Lane::Completion).unwrap())
            .collect::<Vec<_>>();
        assert!(Guard::acquire(key(20), Lane::Completion).is_none());
        LIMITER
            .with(|limiter| assert_eq!(limiter.borrow().slots.len(), NEW_SLOTS + COMPLETION_SLOTS));
        drop(guards);
        drop(completions);
        LIMITER.with(|limiter| assert!(limiter.borrow().slots.is_empty()));
    }

    #[test]
    fn aligned_windows_refill_at_boundary_without_refunding_failed_attempts() {
        reset_for_test();
        for _ in 0..ATTEMPTS_PER_WINDOW {
            let guard = Guard::acquire(key(1), Lane::New).unwrap();
            assert_eq!(
                block_on(
                    guard
                        .ticket()
                        .call(async { Err::<(), _>("external failure") })
                )
                .unwrap(),
                Err("external failure")
            );
        }
        for seconds in [0, 59] {
            TEST_SECONDS.with(|clock| clock.set(seconds));
            assert!(Guard::acquire(key(2), Lane::New).is_none());
        }
        TEST_SECONDS.with(|clock| clock.set(60));
        assert!(Guard::acquire(key(2), Lane::New).is_some());
        TEST_SECONDS.with(|clock| clock.set(59));
        assert!(Guard::acquire(key(2), Lane::New).is_none());
        TEST_SECONDS.with(|clock| clock.set(120));
        assert!(Guard::acquire(key(2), Lane::New).is_some());
    }

    #[test]
    fn completion_budget_is_protected_when_new_setup_tokens_are_spent() {
        reset_for_test();
        for _ in 0..ATTEMPTS_PER_WINDOW {
            drop(Guard::acquire(key(1), Lane::New).unwrap());
        }
        assert!(Guard::acquire(key(2), Lane::New).is_none());
        let guard = Guard::acquire(key(3), Lane::Completion).unwrap();
        for _ in 0..3 {
            assert_eq!(block_on(guard.ticket().call(async { 7 })).unwrap(), 7);
        }
        assert!(guard.ticket().check().is_ok());
    }

    #[test]
    fn expired_callback_and_old_drop_cannot_publish_or_clear_a_successor() {
        reset_for_test();
        let old = Guard::acquire(key(1), Lane::New).unwrap();
        let ticket = old.ticket();
        let (send, receive) = futures::channel::oneshot::channel();
        block_on(async {
            let future = ticket.call(async { receive.await.unwrap() });
            futures::pin_mut!(future);
            assert!(futures::poll!(future.as_mut()).is_pending());
            TEST_SECONDS.with(|clock| clock.set(LEASE_SECONDS));
            let successor = Guard::acquire(key(1), Lane::New).unwrap();
            send.send(7).unwrap();
            assert_eq!(future.await, Err(RelaySetupNotifyResult::Busy));
            drop(old);
            assert!(successor.ticket().check().is_ok());
            let calls = std::cell::Cell::new(0);
            assert_eq!(
                ticket.call(async { calls.set(calls.get() + 1) }).await,
                Err(RelaySetupNotifyResult::Busy)
            );
            assert_eq!(calls.get(), 0);
        });
        // An interruption that never runs Drop also cannot hold the slot forever.
        let lost = Guard::acquire(key(2), Lane::New).unwrap();
        std::mem::forget(lost);
        TEST_SECONDS.with(|clock| clock.set(2 * LEASE_SECONDS));
        assert!(Guard::acquire(key(2), Lane::New).is_some());
    }

    #[test]
    fn call_limit_is_checked_before_dispatch_and_includes_failed_calls() {
        reset_for_test();
        let guard = Guard::acquire(key(1), Lane::New).unwrap();
        let count = std::cell::Cell::new(0);
        for _ in 0..CALLS_PER_ATTEMPT {
            block_on(guard.ticket().call(async {
                count.set(count.get() + 1);
                Err::<(), _>(())
            }))
            .unwrap()
            .unwrap_err();
        }
        assert_eq!(
            block_on(guard.ticket().call(async {
                count.set(count.get() + 1);
            })),
            Err(RelaySetupNotifyResult::Busy)
        );
        assert_eq!(count.get(), CALLS_PER_ATTEMPT);
    }
}
