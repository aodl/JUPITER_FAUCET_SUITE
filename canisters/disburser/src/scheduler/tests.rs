use super::*;
#[cfg(test)]
// Scheduler tests are organized as a nested module to match the existing include-file layout.
#[allow(clippy::module_inception)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use candid::{Nat, Principal};
    use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferError};
    use jupiter_nns_types::{GovernanceError, MaturityDisbursement, Neuron};
    use std::collections::VecDeque;
    use std::future::{pending, Future};
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    };
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    struct UnexpectedLedger;

    #[async_trait]
    impl LedgerClient for UnexpectedLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            panic!("ledger should not be called")
        }
        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            panic!("ledger should not be called")
        }
        async fn transfer(
            &self,
            _arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            panic!("ledger should not be called")
        }
    }

    struct PendingGovernance {
        get_full_neuron_calls: Arc<AtomicUsize>,
    }

    struct ZeroBalanceLedger;

    #[async_trait]
    impl LedgerClient for ZeroBalanceLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            panic!("fee_e8s should not be called")
        }
        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(0)
        }
        async fn transfer(
            &self,
            _arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            panic!("transfer should not be called")
        }
    }

    struct CountingLedger {
        balance: u64,
        transfer_calls: AtomicUsize,
    }

    struct HeldTransferLedger {
        started: AtomicBool,
        release: AtomicBool,
    }

    #[async_trait]
    impl LedgerClient for HeldTransferLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            Ok(10_000)
        }

        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(50_000_000)
        }

        async fn transfer(
            &self,
            _arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.started.store(true, Ordering::SeqCst);
            std::future::poll_fn(|_| {
                if self.release.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await;
            Ok(Ok(Nat::from(77_u64)))
        }
    }

    impl CountingLedger {
        fn new(balance: u64) -> Self {
            Self {
                balance,
                transfer_calls: AtomicUsize::new(0),
            }
        }

        fn transfer_calls(&self) -> usize {
            self.transfer_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl LedgerClient for CountingLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            Ok(10_000)
        }
        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(self.balance)
        }
        async fn transfer(
            &self,
            _arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            panic!("transfer should not be called")
        }
    }

    struct ScriptedGovernance {
        get_full_neuron_results: Mutex<VecDeque<Result<Neuron, GovernanceError>>>,
        disburse_results: Mutex<VecDeque<Result<Option<u64>, GovernanceError>>>,
        get_full_neuron_calls: AtomicUsize,
        disburse_calls: AtomicUsize,
        claim_or_refresh_calls: AtomicUsize,
        refresh_voting_power_calls: AtomicUsize,
    }

    impl ScriptedGovernance {
        fn new(
            get_full_neuron_results: Vec<Result<Neuron, GovernanceError>>,
            disburse_results: Vec<Result<Option<u64>, GovernanceError>>,
        ) -> Self {
            Self {
                get_full_neuron_results: Mutex::new(get_full_neuron_results.into()),
                disburse_results: Mutex::new(disburse_results.into()),
                get_full_neuron_calls: AtomicUsize::new(0),
                disburse_calls: AtomicUsize::new(0),
                claim_or_refresh_calls: AtomicUsize::new(0),
                refresh_voting_power_calls: AtomicUsize::new(0),
            }
        }

        fn get_full_neuron_calls(&self) -> usize {
            self.get_full_neuron_calls.load(Ordering::SeqCst)
        }

        fn disburse_calls(&self) -> usize {
            self.disburse_calls.load(Ordering::SeqCst)
        }

        fn claim_or_refresh_calls(&self) -> usize {
            self.claim_or_refresh_calls.load(Ordering::SeqCst)
        }

        fn refresh_voting_power_calls(&self) -> usize {
            self.refresh_voting_power_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl GovernanceClient for ScriptedGovernance {
        async fn get_full_neuron(&self, _neuron_id: u64) -> Result<Neuron, GovernanceError> {
            self.get_full_neuron_calls.fetch_add(1, Ordering::SeqCst);
            self.get_full_neuron_results
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing get_full_neuron result")
        }

        async fn disburse_maturity_to_account(
            &self,
            _neuron_id: u64,
            _percentage: u32,
            _to_owner: Principal,
            _to_subaccount: Option<Vec<u8>>,
        ) -> Result<Option<u64>, GovernanceError> {
            self.disburse_calls.fetch_add(1, Ordering::SeqCst);
            self.disburse_results
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing disburse result")
        }

        async fn refresh_voting_power(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            self.refresh_voting_power_calls
                .fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn claim_or_refresh_neuron(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            self.claim_or_refresh_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[async_trait]
    impl GovernanceClient for PendingGovernance {
        async fn get_full_neuron(&self, _neuron_id: u64) -> Result<Neuron, GovernanceError> {
            self.get_full_neuron_calls.fetch_add(1, Ordering::SeqCst);
            pending::<Result<Neuron, GovernanceError>>().await
        }

        async fn disburse_maturity_to_account(
            &self,
            _neuron_id: u64,
            _percentage: u32,
            _to_owner: Principal,
            _to_subaccount: Option<Vec<u8>>,
        ) -> Result<Option<u64>, GovernanceError> {
            panic!("disburse_maturity_to_account should not be called")
        }

        async fn refresh_voting_power(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            Ok(())
        }

        async fn claim_or_refresh_neuron(&self, _neuron_id: u64) -> Result<(), GovernanceError> {
            Ok(())
        }
    }

    fn test_config() -> state::Config {
        state::Config {
            neuron_id: 1,
            normal_recipient: Account {
                owner: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
                subaccount: None,
            },
            age_bonus_recipient_1: Account {
                owner: Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(),
                subaccount: None,
            },
            age_bonus_recipient_2: Account {
                owner: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
                subaccount: None,
            },
            ledger_canister_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            governance_canister_id: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            rescue_controller: Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap(),
            autonomous_rescue_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
    }

    fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        future.poll(&mut cx)
    }

    fn run_ready<F: Future>(future: F) -> F::Output {
        let mut future = Box::pin(future);
        match poll_once(future.as_mut()) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    async fn process_test_payout<L: LedgerClient>(
        ledger: &L,
        cfg: &state::Config,
        now_nanos: u64,
        now_secs: u64,
    ) -> bool {
        let guard = MainGuard::acquire(now_secs).expect("test payout lease");
        process_payout(ledger, cfg, now_nanos, now_secs, &guard).await
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum HeldRead {
        Neuron,
        Balance,
        Fee,
    }

    struct TakeoverClients {
        held: HeldRead,
        release: AtomicBool,
        transfers: AtomicUsize,
    }

    impl TakeoverClients {
        async fn wait_at(&self, point: HeldRead) {
            std::future::poll_fn(|_| {
                if self.held != point || self.release.load(Ordering::SeqCst) {
                    Poll::Ready(())
                } else {
                    Poll::Pending
                }
            })
            .await
        }
    }

    #[async_trait]
    impl LedgerClient for TakeoverClients {
        async fn balance_of_e8s(&self, _: Account) -> Result<u64, crate::clients::ClientError> {
            self.wait_at(HeldRead::Balance).await;
            Ok(10_000_000_000)
        }

        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            self.wait_at(HeldRead::Fee).await;
            Ok(10_000)
        }

        async fn transfer(
            &self,
            _: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfers.fetch_add(1, Ordering::SeqCst);
            Err(crate::clients::ClientError::Call(
                "reply unavailable".into(),
            ))
        }
    }

    #[async_trait]
    impl GovernanceClient for TakeoverClients {
        async fn get_full_neuron(&self, _: u64) -> Result<Neuron, GovernanceError> {
            self.wait_at(HeldRead::Neuron).await;
            Ok(Neuron::default())
        }

        async fn disburse_maturity_to_account(
            &self,
            _: u64,
            _: u32,
            _: Principal,
            _: Option<Vec<u8>>,
        ) -> Result<Option<u64>, GovernanceError> {
            panic!("stale worker must not initiate maturity")
        }

        async fn refresh_voting_power(&self, _: u64) -> Result<(), GovernanceError> {
            Ok(())
        }

        async fn claim_or_refresh_neuron(&self, _: u64) -> Result<(), GovernanceError> {
            Ok(())
        }
    }

    fn successor_observes_maturity(now_secs: u64, cfg: &state::Config) -> Vec<u8> {
        #[cfg(feature = "debug_api")]
        let paused = debug_pause_after_planning();
        #[cfg(feature = "debug_api")]
        debug_set_pause_after_planning(false);
        let successor_gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                    amount_e8s: Some(10_000_000_000),
                    ..Default::default()
                }]),
                ..Default::default()
            })],
            vec![],
        );
        assert_eq!(
            run_ready(run_main_tick_with_clients(
                true,
                now_secs * 1_000_000_000,
                now_secs,
                cfg,
                &UnexpectedLedger,
                &successor_gov,
            )),
            MainTickOutcome::Completed
        );
        #[cfg(feature = "debug_api")]
        debug_set_pause_after_planning(paused);
        assert_eq!(successor_gov.get_full_neuron_calls(), 1);
        assert_eq!(successor_gov.disburse_calls(), 0);
        state::with_state(|st| candid::encode_one(st.clone()).unwrap())
    }

    fn assert_stale_read_cannot_pay(poke: bool, held: HeldRead) {
        let now = 5 * 365 * 24 * 60 * 60;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now);
        st.prev_age_seconds = 4 * 365 * 24 * 60 * 60;
        state::set_state(st);
        let clients = TakeoverClients {
            held,
            release: AtomicBool::new(false),
            transfers: AtomicUsize::new(0),
        };
        let mut old_worker = Box::pin(async {
            if poke {
                run_poke_with_clients(now * 1_000_000_000, now, &cfg, &clients, &clients).await;
            } else {
                run_main_tick_with_clients(
                    true,
                    now * 1_000_000_000,
                    now,
                    &cfg,
                    &clients,
                    &clients,
                )
                .await;
            }
        });
        assert!(poll_once(old_worker.as_mut()).is_pending());
        let successor_state = successor_observes_maturity(now + MAIN_TICK_LEASE_SECONDS + 1, &cfg);
        clients.release.store(true, Ordering::SeqCst);
        assert!(poll_once(old_worker.as_mut()).is_ready());
        drop(old_worker);
        assert_eq!(clients.transfers.load(Ordering::SeqCst), 0);
        assert_eq!(
            state::with_state(|st| candid::encode_one(st.clone()).unwrap()),
            successor_state,
            "obsolete callback must preserve every successor-owned state field"
        );
    }

    #[test]
    fn stale_poke_neuron_read_cannot_pay() {
        assert_stale_read_cannot_pay(true, HeldRead::Neuron);
    }

    #[test]
    fn stale_main_balance_read_cannot_pay() {
        assert_stale_read_cannot_pay(false, HeldRead::Balance);
    }

    #[test]
    fn stale_main_fee_read_cannot_pay() {
        assert_stale_read_cannot_pay(false, HeldRead::Fee);
    }

    #[test]
    fn stale_poke_balance_read_cannot_pay() {
        assert_stale_read_cannot_pay(true, HeldRead::Balance);
    }

    #[test]
    fn stale_poke_fee_read_cannot_pay() {
        assert_stale_read_cannot_pay(true, HeldRead::Fee);
    }

    struct FundedLateReplyLedger {
        balance: Mutex<u64>,
        accepted: Mutex<Vec<TransferArg>>,
        calls: Mutex<Vec<TransferArg>>,
        release: AtomicBool,
    }

    #[async_trait]
    impl LedgerClient for FundedLateReplyLedger {
        async fn balance_of_e8s(&self, _: Account) -> Result<u64, crate::clients::ClientError> {
            Ok(*self.balance.lock().unwrap())
        }

        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            Ok(10_000)
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.calls.lock().unwrap().push(arg.clone());
            let block = {
                let mut accepted = self.accepted.lock().unwrap();
                if let Some(block) = accepted.iter().position(|previous| previous == &arg) {
                    return Ok(Err(TransferError::Duplicate {
                        duplicate_of: Nat::from(block as u64),
                    }));
                }
                let debit = u64::try_from(&arg.amount.0).unwrap() + 10_000;
                let mut balance = self.balance.lock().unwrap();
                assert!(
                    *balance >= debit,
                    "fixture must actually fund every accepted payment"
                );
                *balance -= debit;
                let block = accepted.len() as u64;
                accepted.push(arg);
                block
            };
            if block == 0 {
                std::future::poll_fn(|_| {
                    if self.release.load(Ordering::SeqCst) {
                        Poll::Ready(())
                    } else {
                        Poll::Pending
                    }
                })
                .await;
            }
            Ok(Ok(Nat::from(block)))
        }
    }

    fn assert_late_transfer_recovery(lose_reply: bool) {
        let now = 5 * 365 * 24 * 60 * 60;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now);
        st.prev_age_seconds = 4 * 365 * 24 * 60 * 60;
        state::set_state(st);
        let ledger = FundedLateReplyLedger {
            balance: Mutex::new(10_000_000_000),
            accepted: Mutex::new(vec![]),
            calls: Mutex::new(vec![]),
            release: AtomicBool::new(false),
        };
        let gov = ScriptedGovernance::new(vec![Ok(Neuron::default())], vec![Ok(None)]);
        let mut old = Box::pin(run_main_tick_with_clients(
            true,
            now * 1_000_000_000,
            now,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(poll_once(old.as_mut()).is_pending());
        assert_eq!(ledger.accepted.lock().unwrap().len(), 1);
        let original_plan = state::with_state(|st| st.payout_plan.clone().unwrap());
        assert_eq!(original_plan.transfers.len(), 3);
        let successor_time = now + MAIN_TICK_LEASE_SECONDS + 1;
        successor_observes_maturity(successor_time, &cfg);
        if !lose_reply {
            ledger.release.store(true, Ordering::SeqCst);
            assert!(poll_once(old.as_mut()).is_ready());
        }
        drop(old);
        assert_eq!(
            ledger.calls.lock().unwrap().len(),
            1,
            "late reply cannot dispatch a second leg"
        );
        assert_eq!(gov.disburse_calls(), 0);
        state::with_state(|st| {
            assert_eq!(st.prev_age_seconds, 4 * 365 * 24 * 60 * 60);
            assert_eq!(st.last_main_run_ts, successor_time);
            let plan = st
                .payout_plan
                .as_ref()
                .expect("unfinished obligations remain recoverable");
            assert_eq!(plan.id, original_plan.id);
            assert_eq!(plan.transfers[1..], original_plan.transfers[1..]);
            assert_eq!(
                matches!(plan.transfers[0].status, state::TransferStatus::Pending),
                lose_reply
            );
        });
        let recovery_gov = ScriptedGovernance::new(vec![Ok(Neuron::default())], vec![]);
        run_ready(run_poke_with_clients(
            (successor_time + 1) * 1_000_000_000,
            successor_time + 1,
            &cfg,
            &ledger,
            &recovery_gov,
        ));
        assert!(state::with_state(|st| st.payout_plan.is_none()));
        assert_eq!(*ledger.balance.lock().unwrap(), 0);
        let accepted = ledger.accepted.lock().unwrap();
        assert_eq!(accepted.len(), 3);
        for (arg, planned) in accepted.iter().zip(&original_plan.transfers) {
            assert_eq!(arg.to, planned.to);
            assert_eq!(arg.amount, Nat::from(planned.amount_e8s));
            assert_eq!(arg.memo, Some(Memo::from(planned.memo.clone())));
            assert_eq!(arg.created_at_time, Some(planned.created_at_time_nanos));
            assert_eq!(arg.fee, Some(Nat::from(original_plan.fee_e8s)));
        }
        let calls = ledger.calls.lock().unwrap();
        assert_eq!(calls.len(), if lose_reply { 4 } else { 3 });
        if lose_reply {
            assert_eq!(
                calls[0], calls[1],
                "accepted-but-lost reply retries the exact identity"
            );
        }
    }

    #[test]
    fn late_accepted_transfer_is_reconciled_without_authorizing_another_leg() {
        assert_late_transfer_recovery(false);
    }

    #[test]
    fn accepted_transfer_with_lost_reply_recovers_by_exact_duplicate_without_double_debit() {
        assert_late_transfer_recovery(true);
    }

    #[test]
    fn lost_final_reply_requires_duplicate_confirmation_even_with_empty_staging() {
        let now = 1_000;
        let cfg = test_config();
        state::set_state(state::State::new(cfg.clone(), now));
        let ledger = FundedLateReplyLedger {
            balance: Mutex::new(10_000_000_000),
            accepted: Mutex::new(vec![]),
            calls: Mutex::new(vec![]),
            release: AtomicBool::new(false),
        };
        let mut old = Box::pin(process_test_payout(&ledger, &cfg, now * 1_000_000_000, now));
        assert!(poll_once(old.as_mut()).is_pending());
        assert_eq!(*ledger.balance.lock().unwrap(), 0);
        drop(old);
        assert!(run_ready(process_test_payout(
            &ledger,
            &cfg,
            (now + 1) * 1_000_000_000,
            now + 1,
        )));
        let calls = ledger.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "empty balance does not prove the pending identity was accepted"
        );
        assert_eq!(calls[0], calls[1]);
        assert_eq!(ledger.accepted.lock().unwrap().len(), 1);
        assert!(state::with_state(|st| st.payout_plan.is_none()));
    }

    #[test]
    fn expired_lease_without_takeover_cannot_dispatch_after_fee_read() {
        let now = 1_000;
        let cfg = test_config();
        state::set_state(state::State::new(cfg.clone(), now));
        let initial_nonce = state::with_state(|st| st.payout_nonce);
        let clients = TakeoverClients {
            held: HeldRead::Fee,
            release: AtomicBool::new(false),
            transfers: AtomicUsize::new(0),
        };
        let mut old = Box::pin(run_poke_with_clients(
            now * 1_000_000_000,
            now,
            &cfg,
            &clients,
            &clients,
        ));
        assert!(poll_once(old.as_mut()).is_pending());
        tick::TEST_NOW_SECONDS.with(|clock| clock.set(now + MAIN_TICK_LEASE_SECONDS));
        clients.release.store(true, Ordering::SeqCst);
        assert!(poll_once(old.as_mut()).is_ready());
        drop(old);
        assert_eq!(clients.transfers.load(Ordering::SeqCst), 0);
        assert!(state::with_state(|st| st.payout_plan.is_none()));
        assert_eq!(state::with_state(|st| st.payout_nonce), initial_nonce);
    }

    #[cfg(feature = "debug_api")]
    #[test]
    fn debug_pause_planner_is_fenced_after_takeover() {
        debug_set_pause_after_planning(true);
        assert_stale_read_cannot_pay(false, HeldRead::Balance);
        debug_set_pause_after_planning(false);
    }

    #[test]
    fn stale_main_lease_can_be_reclaimed_without_old_guard_clearing_the_new_lease() {
        let now_secs = 1_000_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = UnexpectedLedger;
        let calls = Arc::new(AtomicUsize::new(0));
        let gov = PendingGovernance {
            get_full_neuron_calls: calls.clone(),
        };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(
            false,
            first_now_nanos,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        let second_now_secs = now_secs + MAIN_TICK_LEASE_SECONDS + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(
            false,
            second_now_nanos,
            second_now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(poll_once(fut2.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(second_now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        drop(fut2);
        assert_eq!(state::with_state(|st| st.main_lock_state_ts), Some(0));
    }

    #[test]
    fn maintenance_refresh_runs_even_when_neuron_read_fails() {
        let now_secs = 1_150_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = UnexpectedLedger;
        let gov = ScriptedGovernance::new(
            vec![Err(GovernanceError {
                error_message: "temporary governance read failure".to_string(),
                error_type: 1,
            })],
            vec![],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(
            true,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(fut.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(
            gov.claim_or_refresh_calls(),
            1,
            "stake refresh should not depend on get_full_neuron"
        );
        assert_eq!(
            gov.refresh_voting_power_calls(),
            1,
            "voting-power refresh should run after stake refresh even when get_full_neuron fails"
        );
        assert_eq!(gov.get_full_neuron_calls(), 1);
        assert_eq!(gov.disburse_calls(), 0);
    }

    #[test]
    fn post_upgrade_clears_inflight_lock_and_allows_next_tick() {
        let now_secs = 1_200_u64;
        let mut st = state::State::new(test_config(), now_secs);
        st.main_lock_state_ts = Some(1);
        crate::apply_upgrade_args_to_state(&mut st, None, now_secs + 1);
        state::set_state(st);

        assert_eq!(state::with_state(|st| st.main_lock_state_ts), Some(0));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = UnexpectedLedger;
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 0,
                maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                    amount_e8s: Some(1),
                    ..Default::default()
                }]),
                ..Default::default()
            })],
            vec![],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(
            false,
            (now_secs + 1) * 1_000_000_000,
            now_secs + 1,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(fut.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(gov.get_full_neuron_calls(), 1);
        assert_eq!(gov.disburse_calls(), 0);
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(state::with_state(|st| st.main_lock_state_ts), Some(0));
    }

    #[test]
    fn staged_balance_is_left_untouched_while_maturity_disbursement_is_in_flight() {
        let now_secs = 1_900_u64;
        let mut st = state::State::new(test_config(), now_secs);
        st.prev_age_seconds = 777;
        state::set_state(st);

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = CountingLedger::new(50_000_000);
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 100,
                maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                    amount_e8s: Some(50),
                    ..Default::default()
                }]),
                ..Default::default()
            })],
            vec![],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(
            false,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(fut.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(
            ledger.transfer_calls(),
            0,
            "staged ICP must not be routed while a disbursement is already in flight"
        );
        assert_eq!(
            gov.disburse_calls(),
            0,
            "in-flight source-of-truth state must suppress a second initiation"
        );
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(
            state::with_state(|st| st.prev_age_seconds),
            777,
            "skipped ticks must preserve the previously captured age snapshot"
        );
    }

    #[test]
    fn poke_preserves_in_flight_maturity_and_daily_cadence() {
        let now_secs = 2_000_u64;
        let mut st = state::State::new(test_config(), 1_000);
        st.prev_age_seconds = 777;
        let last_main_run_ts = st.last_main_run_ts;
        state::set_state(st);
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = CountingLedger::new(50_000_000);
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                    amount_e8s: Some(50),
                    ..Default::default()
                }]),
                ..Default::default()
            })],
            vec![],
        );

        run_ready(run_poke_with_clients(
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));

        assert_eq!(ledger.transfer_calls(), 0);
        assert_eq!(gov.disburse_calls(), 0);
        assert_eq!(gov.claim_or_refresh_calls(), 0);
        assert_eq!(gov.refresh_voting_power_calls(), 0);
        assert_eq!(state::with_state(|st| st.prev_age_seconds), 777);
        assert_eq!(
            state::with_state(|st| st.last_main_run_ts),
            last_main_run_ts
        );
        assert_eq!(state::with_state(|st| st.main_lock_state_ts), Some(0));
    }

    #[test]
    fn busy_scheduled_main_attempt_retries_governance_work_after_poke_lease_releases() {
        let now_secs = 2_000_u64;
        let mut st = state::State::new(test_config(), 0);
        st.last_main_run_ts = 0;
        state::set_state(st);
        let cfg = state::with_state(|st| st.config.clone());
        let poke_guard = MainGuard::acquire(now_secs).expect("poke main lease");
        let ledger = CountingLedger::new(0);
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 100,
                maturity_disbursements_in_progress: None,
                ..Default::default()
            })],
            vec![Ok(Some(1))],
        );

        assert_eq!(
            run_ready(run_main_tick_with_clients(
                false,
                now_secs * 1_000_000_000,
                now_secs,
                &cfg,
                &ledger,
                &gov,
            )),
            MainTickOutcome::MainGuardBusy
        );
        assert_eq!(gov.claim_or_refresh_calls(), 0);
        assert_eq!(state::with_state(|st| st.last_main_run_ts), 0);

        drop(poke_guard);
        let retry_secs = now_secs + SCHEDULED_MAIN_RETRY_SECONDS;
        assert_eq!(
            run_ready(run_main_tick_with_clients(
                false,
                retry_secs * 1_000_000_000,
                retry_secs,
                &cfg,
                &ledger,
                &gov,
            )),
            MainTickOutcome::Completed
        );
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(gov.refresh_voting_power_calls(), 1);
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(state::with_state(|st| st.last_main_run_ts), retry_secs);
    }

    #[test]
    fn delayed_daily_attempt_does_not_consume_the_next_regular_opportunity() {
        const D: u64 = 86_400;
        clear_scheduled_main_retry();
        let mut cfg = test_config();
        cfg.main_interval_seconds = D;
        let mut st = state::State::new(cfg.clone(), 0);
        st.last_main_run_ts = 0;
        state::set_state(st);
        let ledger = CountingLedger::new(0);
        let gov = ScriptedGovernance::new(
            vec![
                Err(GovernanceError {
                    error_type: -1,
                    error_message: "unavailable".into()
                });
                3
            ],
            vec![],
        );
        let attempt = |force, now| {
            run_main_tick_with_clients(force, now * 1_000_000_000, now, &cfg, &ledger, &gov)
        };
        let calls = || gov.get_full_neuron_calls();
        let timers = std::cell::RefCell::new(Vec::new());
        let schedule = |reservation, delay| {
            assert_eq!(delay, SCHEDULED_MAIN_RETRY_SECONDS);
            timers.borrow_mut().push(reservation);
        };
        let busy = MainGuard::acquire(D).unwrap();
        assert_eq!(
            run_ready(run_scheduled_main_tick(attempt(false, D), &schedule)),
            MainTickOutcome::MainGuardBusy
        );
        let first = timers.borrow_mut().pop().unwrap();
        run_ready(run_scheduled_main_retry(
            first,
            || attempt(false, D + 60),
            &schedule,
        ));
        assert_eq!(
            timers.borrow().len(),
            1,
            "still-busy retry reserves one future opportunity"
        );
        run_ready(run_scheduled_main_tick(attempt(false, D + 60), &schedule));
        assert_eq!(timers.borrow().len(), 1, "concurrent requests coalesce");
        assert_eq!(calls(), 0);
        drop(busy);
        let retry = timers.borrow_mut().pop().unwrap();
        run_ready(run_scheduled_main_retry(
            retry,
            || attempt(false, D + 120),
            &schedule,
        ));
        assert_eq!(state::with_state(|st| st.last_main_run_ts), D + 120);
        let completed_calls = calls();
        assert!(completed_calls > 0);
        assert!(
            timers.borrow().is_empty(),
            "handled operational errors keep ordinary failure cadence"
        );

        assert_eq!(
            run_ready(run_scheduled_main_tick(attempt(false, 2 * D), &schedule)),
            MainTickOutcome::Deferred(60)
        );
        assert_eq!(
            state::with_state(|st| st.last_main_run_ts),
            D + 120,
            "a too-early daily opportunity must not advance the admitted-attempt timestamp"
        );
        assert_eq!(
            calls(),
            completed_calls,
            "Deferred performs no external work"
        );
        let retry = timers
            .borrow_mut()
            .pop()
            .expect("production decision must schedule Deferred work");
        assert!(timers.borrow().is_empty());
        run_ready(run_scheduled_main_retry(
            retry,
            || attempt(false, 2 * D + SCHEDULED_MAIN_RETRY_SECONDS),
            &schedule,
        ));
        assert_eq!(state::with_state(|st| st.last_main_run_ts), 2 * D + 60);
        assert!(
            calls() > completed_calls,
            "the scheduled retry actually calls the clients before 3D"
        );
        assert!(timers.borrow().is_empty());
    }

    #[test]
    fn forced_run_before_daily_timer_defers_once_until_due_without_minute_polling() {
        const D: u64 = 86_400;
        clear_scheduled_main_retry();
        let mut cfg = test_config();
        cfg.main_interval_seconds = D;
        state::set_state(state::State::new(cfg.clone(), 0));
        let ledger = CountingLedger::new(0);
        let gov = ScriptedGovernance::new(
            vec![
                Err(GovernanceError {
                    error_type: -1,
                    error_message: "unavailable".into(),
                });
                3
            ],
            vec![],
        );
        let attempt = |force, now| {
            run_main_tick_with_clients(force, now * 1_000_000_000, now, &cfg, &ledger, &gov)
        };
        let calls = || gov.get_full_neuron_calls();
        let timers = std::cell::RefCell::new(Vec::new());
        let schedule = |reservation, delay| timers.borrow_mut().push((reservation, delay));
        assert_eq!(run_ready(attempt(true, D - 1)), MainTickOutcome::Completed);
        let forced_calls = calls();
        assert!(forced_calls > 0);
        run_ready(run_scheduled_main_tick(attempt(false, D), &schedule));
        let due = (D - 1) + (D - 60);
        assert_eq!(timers.borrow().len(), 1);
        assert_eq!(
            timers.borrow()[0].1,
            due - D,
            "Deferred must schedule the known due time, not another minute poll"
        );
        for now in [D + 1, D + 60, due - 1] {
            run_ready(run_scheduled_main_tick(attempt(false, now), &schedule));
            assert_eq!(timers.borrow().len(), 1, "duplicate requests coalesce");
        }
        assert_eq!(calls(), forced_calls);
        assert_eq!(state::with_state(|st| st.last_main_run_ts), D - 1);
        let (first, delay) = timers.borrow_mut().pop().unwrap();
        run_ready(run_scheduled_main_retry(
            first,
            || attempt(false, D + delay),
            &schedule,
        ));
        assert_eq!(state::with_state(|st| st.last_main_run_ts), due);
        assert!(
            calls() > forced_calls,
            "the delayed callback performs actual client work"
        );
        assert!(timers.borrow().is_empty());

        // The next anchored daily invocation also waits directly for its due time.
        let completed_calls = calls();
        run_ready(run_scheduled_main_tick(attempt(false, 2 * D), &schedule));
        let next_due = due + (D - 60);
        assert_eq!(timers.borrow().len(), 1);
        assert_eq!(timers.borrow()[0].1, next_due - 2 * D);
        let (successor, delay) = timers.borrow_mut().pop().unwrap();
        run_ready(run_scheduled_main_retry(
            first,
            || async { panic!("obsolete callback performed work") },
            &schedule,
        ));
        assert!(
            reserve_scheduled_main_retry().is_none(),
            "successor remains reserved"
        );
        assert_eq!(calls(), completed_calls);
        run_ready(run_scheduled_main_retry(
            successor,
            || attempt(false, 2 * D + delay),
            &schedule,
        ));
        assert_eq!(state::with_state(|st| st.last_main_run_ts), next_due);
        assert!(calls() > completed_calls);
        assert!(timers.borrow().is_empty());
    }

    #[test]
    fn obsolete_scheduled_retry_cannot_poll_or_clear_a_newer_intent() {
        const D: u64 = 86_400;
        clear_scheduled_main_retry();
        let mut cfg = test_config();
        cfg.main_interval_seconds = D;
        let mut st = state::State::new(cfg.clone(), 0);
        st.last_main_run_ts = 0;
        state::set_state(st);
        let ledger = CountingLedger::new(0);
        let gov = ScriptedGovernance::new(
            vec![
                Err(GovernanceError {
                    error_type: -1,
                    error_message: "unavailable".into()
                });
                3
            ],
            vec![],
        );
        let attempt = |force, now| {
            run_main_tick_with_clients(force, now * 1_000_000_000, now, &cfg, &ledger, &gov)
        };
        let calls = || gov.get_full_neuron_calls();
        let timers = std::cell::RefCell::new(Vec::new());
        let schedule = |reservation, _delay| timers.borrow_mut().push(reservation);
        let busy = MainGuard::acquire(D).unwrap();
        run_ready(run_scheduled_main_tick(attempt(false, D), &schedule));
        let old = timers.borrow_mut().pop().unwrap();
        drop(busy);
        run_ready(attempt(true, D + 120));
        let completed_calls = calls();
        run_ready(run_scheduled_main_retry(
            old,
            || async { panic!("obsolete retry ran") },
            &schedule,
        ));
        assert!(
            timers.borrow().is_empty(),
            "obsolete callback must not start minute polling"
        );
        run_ready(run_scheduled_main_tick(attempt(false, 2 * D), &schedule));
        let successor = timers.borrow_mut().pop().unwrap();
        assert_ne!(old, successor);
        run_ready(run_scheduled_main_retry(
            old,
            || async { panic!("stale callback ran") },
            &schedule,
        ));
        assert!(
            reserve_scheduled_main_retry().is_none(),
            "stale callback must leave the successor reserved"
        );
        assert_eq!(calls(), completed_calls);
        run_ready(run_scheduled_main_retry(
            successor,
            || attempt(false, 2 * D + 60),
            &schedule,
        ));
        assert!(calls() > completed_calls);
        assert!(timers.borrow().is_empty());
    }

    #[test]
    fn scheduled_callback_completion_cannot_discard_an_intent_created_during_await() {
        const D: u64 = 86_400;
        clear_scheduled_main_retry();
        let mut cfg = test_config();
        cfg.main_interval_seconds = D;
        let mut st = state::State::new(cfg.clone(), 0);
        st.last_main_run_ts = 0;
        state::set_state(st);
        let ledger = CountingLedger::new(0);
        let gov = ScriptedGovernance::new(
            vec![
                Err(GovernanceError {
                    error_type: -1,
                    error_message: "unavailable".into()
                });
                3
            ],
            vec![],
        );
        let attempt = |force, now| {
            run_main_tick_with_clients(force, now * 1_000_000_000, now, &cfg, &ledger, &gov)
        };
        let calls = || gov.get_full_neuron_calls();
        let timers = std::cell::RefCell::new(Vec::new());
        let schedule = |reservation, _delay| timers.borrow_mut().push(reservation);
        let old = reserve_scheduled_main_retry().unwrap();
        let ready = std::cell::Cell::new(false);
        let mut callback = Box::pin(run_scheduled_main_retry(
            old,
            || {
                std::future::poll_fn(|_| {
                    if ready.get() {
                        std::task::Poll::Ready(MainTickOutcome::Completed)
                    } else {
                        std::task::Poll::Pending
                    }
                })
            },
            &schedule,
        ));
        assert!(poll_once(callback.as_mut()).is_pending());
        run_ready(attempt(true, D + 120));
        let completed_calls = calls();
        assert!(completed_calls > 0);
        run_ready(run_scheduled_main_tick(attempt(false, 2 * D), &schedule));
        let successor = timers.borrow_mut().pop().unwrap();
        ready.set(true);
        run_ready(callback);
        assert_eq!(
            calls(),
            completed_calls,
            "stale completion performs no client work"
        );
        assert!(reserve_scheduled_main_retry().is_none());
        run_ready(run_scheduled_main_retry(
            successor,
            || attempt(false, 2 * D + 60),
            &schedule,
        ));
        assert_eq!(state::with_state(|st| st.last_main_run_ts), 2 * D + 60);
        assert!(
            calls() > completed_calls,
            "the successor still performs client work"
        );
        assert!(timers.borrow().is_empty());
    }

    #[test]
    fn scheduled_main_retry_requests_coalesce_until_timer_begins() {
        clear_scheduled_main_retry();

        assert!(reserve_scheduled_main_retry().is_some());
        assert!(reserve_scheduled_main_retry().is_none());

        clear_scheduled_main_retry();
        assert!(reserve_scheduled_main_retry().is_some());
        assert!(reserve_scheduled_main_retry().is_none());

        clear_scheduled_main_retry();
    }

    #[derive(Clone)]
    enum TransferScriptStep {
        Ok(BlockIndex),
        TypedErr(TransferError),
        CallErr,
    }

    struct ScriptedTransferLedger {
        balance: u64,
        fee: u64,
        steps: Mutex<VecDeque<TransferScriptStep>>,
        transfer_calls: AtomicUsize,
    }

    impl ScriptedTransferLedger {
        fn new(balance: u64, fee: u64, steps: Vec<TransferScriptStep>) -> Self {
            Self {
                balance,
                fee,
                steps: Mutex::new(steps.into()),
                transfer_calls: AtomicUsize::new(0),
            }
        }

        fn transfer_calls(&self) -> usize {
            self.transfer_calls.load(Ordering::SeqCst)
        }
    }

    struct BalanceTrackingLedger {
        balance: Mutex<u64>,
        transfers: Mutex<Vec<TransferArg>>,
        fee: u64,
        steps: Mutex<VecDeque<TransferScriptStep>>,
        transfer_calls: AtomicUsize,
    }

    impl BalanceTrackingLedger {
        fn new(balance: u64, fee: u64, steps: Vec<TransferScriptStep>) -> Self {
            Self {
                balance: Mutex::new(balance),
                transfers: Mutex::new(Vec::new()),
                fee,
                steps: Mutex::new(steps.into()),
                transfer_calls: AtomicUsize::new(0),
            }
        }

        fn transfer_calls(&self) -> usize {
            self.transfer_calls.load(Ordering::SeqCst)
        }

        fn balance(&self) -> u64 {
            *self.balance.lock().unwrap()
        }
    }

    #[async_trait]
    impl LedgerClient for BalanceTrackingLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            Ok(self.fee)
        }

        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(*self.balance.lock().unwrap())
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            self.transfers.lock().unwrap().push(arg.clone());
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing transfer step");
            match step {
                TransferScriptStep::Ok(block) => {
                    let amount =
                        u64::try_from(arg.amount.0).expect("amount should fit in u64 for tests");
                    let fee_nat = arg.fee.expect("tests always include a fee");
                    let fee = u64::try_from(fee_nat.0).expect("fee should fit in u64 for tests");
                    let mut balance = self.balance.lock().unwrap();
                    *balance = balance.saturating_sub(amount.saturating_add(fee));
                    Ok(Ok(block))
                }
                TransferScriptStep::TypedErr(err) => Ok(Err(err)),
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call(
                    "scripted transfer transport failure".to_string(),
                )),
            }
        }
    }

    #[async_trait]
    impl LedgerClient for ScriptedTransferLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            Ok(self.fee)
        }
        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(self.balance)
        }
        async fn transfer(
            &self,
            _arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            match self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing transfer step")
            {
                TransferScriptStep::Ok(block) => Ok(Ok(block)),
                TransferScriptStep::TypedErr(err) => Ok(Err(err)),
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call(
                    "scripted transfer transport failure".to_string(),
                )),
            }
        }
    }

    #[test]
    fn poke_pays_staged_icp_without_governance_maintenance_or_cadence_movement() {
        let now_secs = 2_100_u64;
        let st = state::State::new(test_config(), 1_000);
        let last_main_run_ts = st.last_main_run_ts;
        state::set_state(st);
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = ScriptedTransferLedger::new(
            100_000_000,
            10_000,
            vec![
                TransferScriptStep::Ok(Nat::from(1_u64)),
                TransferScriptStep::Ok(Nat::from(2_u64)),
                TransferScriptStep::Ok(Nat::from(3_u64)),
            ],
        );
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                maturity_disbursements_in_progress: None,
                ..Default::default()
            })],
            vec![],
        );

        run_ready(run_poke_with_clients(
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));

        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(gov.disburse_calls(), 0);
        assert_eq!(gov.claim_or_refresh_calls(), 0);
        assert_eq!(gov.refresh_voting_power_calls(), 0);
        assert_eq!(
            state::with_state(|st| st.last_main_run_ts),
            last_main_run_ts
        );
    }

    struct QueuedFeeLedger {
        balance: u64,
        fees: Mutex<VecDeque<u64>>,
        steps: Mutex<VecDeque<TransferScriptStep>>,
        fee_calls: AtomicUsize,
        transfer_fees: Mutex<Vec<u64>>,
    }

    impl QueuedFeeLedger {
        fn new(balance: u64, fees: Vec<u64>, steps: Vec<TransferScriptStep>) -> Self {
            Self {
                balance,
                fees: Mutex::new(fees.into()),
                steps: Mutex::new(steps.into()),
                fee_calls: AtomicUsize::new(0),
                transfer_fees: Mutex::new(Vec::new()),
            }
        }

        fn fee_calls(&self) -> usize {
            self.fee_calls.load(Ordering::SeqCst)
        }

        fn transfer_fees(&self) -> Vec<u64> {
            self.transfer_fees.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LedgerClient for QueuedFeeLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> {
            self.fee_calls.fetch_add(1, Ordering::SeqCst);
            Ok(*self
                .fees
                .lock()
                .unwrap()
                .front()
                .expect("missing queued fee"))
        }

        async fn balance_of_e8s(
            &self,
            _account: Account,
        ) -> Result<u64, crate::clients::ClientError> {
            Ok(self.balance)
        }

        async fn transfer(
            &self,
            arg: TransferArg,
        ) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            let fee_nat = arg.fee.expect("tests always include a fee");
            let fee = u64::try_from(fee_nat.0).expect("fee should fit in u64 for tests");
            self.transfer_fees.lock().unwrap().push(fee);
            match self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("missing transfer step")
            {
                TransferScriptStep::Ok(block) => Ok(Ok(block)),
                TransferScriptStep::TypedErr(err) => {
                    let mut fees = self.fees.lock().unwrap();
                    if !fees.is_empty() {
                        fees.pop_front();
                    }
                    Ok(Err(err))
                }
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call(
                    "scripted transfer transport failure".to_string(),
                )),
            }
        }
    }

    fn set_test_payout_plan(now_secs: u64) {
        let mut st = state::State::new(test_config(), now_secs);
        st.payout_plan = Some(state::PayoutPlan {
            id: 9,
            fee_e8s: 10_000,
            created_at_base_nanos: now_secs * 1_000_000_000,
            transfers: vec![state::PlannedTransfer {
                to: Account {
                    owner: Principal::from_text("aaaaa-aa").unwrap(),
                    subaccount: None,
                },
                gross_share_e8s: 50_000_000,
                amount_e8s: 49_990_000,
                created_at_time_nanos: now_secs * 1_000_000_000,
                memo: b"test-transfer".to_vec(),
                status: state::TransferStatus::Pending,
            }],
        });
        state::set_state(st);
    }

    #[test]
    fn permanent_ledger_errors_clear_persisted_plan_instead_of_wedging() {
        for err in [
            TransferError::InsufficientFunds {
                balance: Nat::from(0u64),
            },
            TransferError::GenericError {
                error_code: Nat::from(5u64),
                message: "permanent ledger failure".to_string(),
            },
            TransferError::BadBurn {
                min_burn_amount: Nat::from(10u64),
            },
        ] {
            let now_secs = 2_100_u64;
            set_test_payout_plan(now_secs);

            let cfg = state::with_state(|st| st.config.clone());
            let ledger = ScriptedTransferLedger::new(
                50_000_000,
                10_000,
                vec![TransferScriptStep::TypedErr(err)],
            );
            let gov = ScriptedGovernance::new(
                vec![Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                })],
                vec![],
            );

            let mut fut = Box::pin(run_main_tick_with_clients(
                true,
                now_secs * 1_000_000_000,
                now_secs,
                &cfg,
                &ledger,
                &gov,
            ));
            assert!(matches!(
                poll_once(fut.as_mut()),
                Poll::Ready(MainTickOutcome::Completed)
            ));
            assert_eq!(ledger.transfer_calls(), 1);
            assert!(
                state::with_state(|st| st.payout_plan.is_none()),
                "permanent ledger error should clear the persisted plan"
            );
            assert_eq!(
                gov.claim_or_refresh_calls(),
                1,
                "stake refresh should still run after a failed payout"
            );
        }
    }

    #[test]
    fn transport_errors_keep_persisted_plan_for_safe_retry() {
        let now_secs = 2_200_u64;
        set_test_payout_plan(now_secs);

        let cfg = state::with_state(|st| st.config.clone());
        let ledger =
            ScriptedTransferLedger::new(50_000_000, 10_000, vec![TransferScriptStep::CallErr]);
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 100,
                maturity_disbursements_in_progress: None,
                ..Default::default()
            })],
            vec![],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(
            true,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(fut.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(ledger.transfer_calls(), 1);
        assert!(
            state::with_state(|st| st.payout_plan.is_some()),
            "transport ambiguity should retain the plan for duplicate-safe retry"
        );
        assert_eq!(gov.claim_or_refresh_calls(), 1);
    }

    #[test]
    fn too_old_and_created_in_future_clear_persisted_plan_and_allow_rebuild() {
        for err in [
            TransferError::TooOld,
            TransferError::CreatedInFuture {
                ledger_time: 9_999u64,
            },
        ] {
            let now_secs = 2_225_u64;
            set_test_payout_plan(now_secs);

            let cfg = state::with_state(|st| st.config.clone());
            let ledger = ScriptedTransferLedger::new(
                50_000_000,
                10_000,
                vec![TransferScriptStep::TypedErr(err)],
            );
            let gov = ScriptedGovernance::new(
                vec![Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                })],
                vec![],
            );

            let mut fut = Box::pin(run_main_tick_with_clients(
                true,
                now_secs * 1_000_000_000,
                now_secs,
                &cfg,
                &ledger,
                &gov,
            ));
            assert!(matches!(
                poll_once(fut.as_mut()),
                Poll::Ready(MainTickOutcome::Completed)
            ));
            assert_eq!(ledger.transfer_calls(), 1);
            assert!(
                state::with_state(|st| st.payout_plan.is_none()),
                "typed timing rejection should clear the plan so a later tick can rebuild it"
            );
            assert_eq!(gov.claim_or_refresh_calls(), 1);
        }
    }

    #[test]
    fn transport_retry_uses_persisted_fee_even_if_live_fee_changes_before_retry() {
        let now_secs = 2_235_u64;
        state::set_state(state::State::new(test_config(), now_secs));
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = QueuedFeeLedger::new(
            50_000_000,
            vec![10_000, 20_000],
            vec![
                TransferScriptStep::CallErr,
                TransferScriptStep::Ok(Nat::from(77u64)),
            ],
        );

        assert!(!run_ready(process_test_payout(
            &ledger,
            &cfg,
            now_secs * 1_000_000_000,
            now_secs
        )));
        let persisted = state::with_state(|st| st.payout_plan.clone())
            .expect("transport ambiguity should retain the original plan");
        assert_eq!(persisted.fee_e8s, 10_000);
        assert_eq!(
            ledger.fee_calls(),
            1,
            "fee should be read only when the plan is first created"
        );

        assert!(run_ready(process_test_payout(
            &ledger,
            &cfg,
            (now_secs + 1) * 1_000_000_000,
            now_secs + 1
        )));
        assert_eq!(
            ledger.fee_calls(),
            1,
            "retry should reuse the persisted fee rather than re-reading the ledger fee"
        );
        assert_eq!(ledger.transfer_fees(), vec![10_000, 10_000]);
        assert!(
            state::with_state(|st| st.payout_plan.is_none()),
            "successful retry should clear the completed plan"
        );
    }

    #[test]
    fn stale_ledger_callback_cannot_mark_replacement_plan_transfer_sent() {
        let now_secs = 2_238_u64;
        state::set_state(state::State::new(test_config(), now_secs));
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = HeldTransferLedger {
            started: AtomicBool::new(false),
            release: AtomicBool::new(false),
        };
        let mut future = Box::pin(process_test_payout(
            &ledger,
            &cfg,
            now_secs * 1_000_000_000,
            now_secs,
        ));
        assert!(poll_once(future.as_mut()).is_pending());
        assert!(ledger.started.load(Ordering::SeqCst));

        let replacement = state::with_state_mut(|st| {
            let mut replacement = st.payout_plan.clone().expect("planned payout");
            replacement.id = replacement.id.saturating_add(100);
            replacement.transfers[0].memo.push(99);
            st.payout_plan = Some(replacement.clone());
            replacement
        });
        ledger.release.store(true, Ordering::SeqCst);
        assert!(matches!(poll_once(future.as_mut()), Poll::Ready(false)));
        state::with_state(|st| {
            assert_eq!(st.payout_plan.as_ref(), Some(&replacement));
            assert_eq!(st.last_successful_transfer_ts, None);
        });
    }

    #[test]
    fn rebuilt_plan_uses_new_live_fee_after_too_old_clears_stale_plan() {
        let now_secs = 2_240_u64;
        state::set_state(state::State::new(test_config(), now_secs));
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = QueuedFeeLedger::new(
            50_000_000,
            vec![10_000, 20_000],
            vec![
                TransferScriptStep::TypedErr(TransferError::TooOld),
                TransferScriptStep::CallErr,
            ],
        );

        assert!(!run_ready(process_test_payout(
            &ledger,
            &cfg,
            now_secs * 1_000_000_000,
            now_secs
        )));
        assert!(
            state::with_state(|st| st.payout_plan.is_none()),
            "TooOld should clear the stale plan immediately"
        );

        assert!(!run_ready(process_test_payout(
            &ledger,
            &cfg,
            (now_secs + 1) * 1_000_000_000,
            now_secs + 1
        )));
        let rebuilt = state::with_state(|st| st.payout_plan.clone())
            .expect("transport ambiguity on the rebuilt plan should retain it for inspection");
        assert_eq!(
            rebuilt.fee_e8s, 20_000,
            "rebuild should pick up the new live ledger fee"
        );
        assert_eq!(ledger.fee_calls(), 2);
        assert_eq!(ledger.transfer_fees(), vec![10_000, 20_000]);
    }

    #[test]
    fn governance_ok_none_disbursement_is_treated_as_successful_initiation() {
        let now_secs = 2_245_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = ZeroBalanceLedger;
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 321,
                maturity_disbursements_in_progress: None,
                ..Default::default()
            })],
            vec![Ok(None)],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(
            true,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(fut.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(
            state::with_state(|st| st.prev_age_seconds),
            now_secs.saturating_sub(321),
            "age should still be captured when governance returns Ok(None)"
        );
    }

    #[test]
    fn terminal_failure_after_partial_success_replans_from_remaining_staging_balance() {
        let now_secs = 2_250_u64;
        let mut st = state::State::new(test_config(), now_secs);
        st.payout_plan = Some(state::PayoutPlan {
            id: 17,
            fee_e8s: 10_000,
            created_at_base_nanos: now_secs * 1_000_000_000,
            transfers: vec![
                state::PlannedTransfer {
                    to: Account {
                        owner: Principal::from_text("aaaaa-aa").unwrap(),
                        subaccount: None,
                    },
                    gross_share_e8s: 50_000_000,
                    amount_e8s: 49_990_000,
                    created_at_time_nanos: now_secs * 1_000_000_000,
                    memo: b"first-leg".to_vec(),
                    status: state::TransferStatus::Pending,
                },
                state::PlannedTransfer {
                    to: Account {
                        owner: Principal::from_text("2vxsx-fae").unwrap(),
                        subaccount: None,
                    },
                    gross_share_e8s: 50_000_000,
                    amount_e8s: 49_990_000,
                    created_at_time_nanos: now_secs * 1_000_000_000 + 1,
                    memo: b"second-leg".to_vec(),
                    status: state::TransferStatus::Pending,
                },
            ],
        });
        state::set_state(st);

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = BalanceTrackingLedger::new(
            100_000_000,
            10_000,
            vec![
                TransferScriptStep::Ok(Nat::from(11u64)),
                TransferScriptStep::TypedErr(TransferError::GenericError {
                    error_code: Nat::from(5u64),
                    message: "terminal rejection after first leg".to_string(),
                }),
                TransferScriptStep::CallErr,
            ],
        );
        let gov = ScriptedGovernance::new(
            vec![
                Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                }),
                Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                }),
            ],
            vec![],
        );

        let mut first = Box::pin(run_main_tick_with_clients(
            true,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(first.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(
            ledger.transfer_calls(),
            2,
            "first run should send one leg, then stop on the terminal rejection"
        );
        assert_eq!(
            ledger.balance(),
            50_000_000,
            "successful first leg should debit its gross share from staging"
        );
        assert!(state::with_state(|st| st.payout_plan.is_none()), "terminal rejection should clear the stale split so the next tick replans from current balance");
        assert_eq!(
            state::with_state(|st| st.last_successful_transfer_ts),
            Some(now_secs)
        );

        let retry_secs = now_secs + 60;
        let mut second = Box::pin(run_main_tick_with_clients(
            true,
            retry_secs * 1_000_000_000,
            retry_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(second.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(ledger.transfer_calls(), 3, "second run should attempt exactly one transfer from the replanned remainder before the scripted transport failure");

        let replanned = state::with_state(|st| st.payout_plan.clone())
            .expect("transport ambiguity should retain the replanned remainder for inspection");
        assert_eq!(replanned.fee_e8s, 10_000);
        let replanned_total_gross: u64 =
            replanned.transfers.iter().map(|t| t.gross_share_e8s).sum();
        assert!(
            replanned_total_gross <= 50_000_000,
            "replanned gross cost must fit within the remaining staging balance"
        );
        assert!(
            replanned_total_gross < 100_000_000,
            "replan must not reuse the original pre-partial-success balance"
        );
        assert!(replanned
            .transfers
            .iter()
            .all(|t| matches!(t.status, state::TransferStatus::Pending)));
        assert!(replanned
            .transfers
            .iter()
            .all(|t| t.created_at_time_nanos >= retry_secs * 1_000_000_000));
        assert_eq!(
            gov.claim_or_refresh_calls(),
            2,
            "stake refresh should run after each failed payout tick"
        );
    }

    #[test]
    fn lost_maturity_reply_preserves_capped_age_and_routes_100_icp_once() {
        let now = 5 * 365 * 86_400;
        let cfg = test_config();
        state::set_state(state::State::new(cfg.clone(), 0));
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 0,
                ..Default::default()
            })],
            vec![Err(GovernanceError {
                error_message: "accepted request, reply lost".into(),
                error_type: 1,
            })],
        );
        run_ready(run_main_tick_with_clients(
            true,
            now * 1_000_000_000,
            now,
            &cfg,
            &ZeroBalanceLedger,
            &gov,
        ));
        let ledger = BalanceTrackingLedger::new(
            100 * 100_000_000,
            10_000,
            (1..=3)
                .map(|block| TransferScriptStep::Ok(Nat::from(block as u64)))
                .collect(),
        );
        assert!(run_ready(process_test_payout(
            &ledger,
            &cfg,
            (now + 1) * 1_000_000_000,
            now + 1
        )));
        let transfers = ledger.transfers.lock().unwrap();
        assert_eq!(transfers.len(), 3);
        for (transfer, (to, gross)) in transfers.iter().zip([
            (cfg.normal_recipient, 80_u64 * 100_000_000),
            (cfg.age_bonus_recipient_1, 19 * 100_000_000),
            (cfg.age_bonus_recipient_2, 100_000_000),
        ]) {
            assert_eq!(transfer.to, to);
            assert_eq!(transfer.amount, Nat::from(gross - 10_000));
            assert_eq!(transfer.fee, Some(Nat::from(10_000_u64)));
        }
        drop(transfers);
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(ledger.balance(), 0);
        assert!(run_ready(process_test_payout(
            &ledger,
            &cfg,
            (now + 2) * 1_000_000_000,
            now + 2
        )));
        assert_eq!(ledger.transfer_calls(), 3);
    }

    #[test]
    fn ambiguous_initiation_is_reconciled_via_in_flight_neuron_state() {
        let now_secs = 2_000_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = ZeroBalanceLedger;
        let gov = ScriptedGovernance::new(
            vec![
                Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                }),
                Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: Some(vec![MaturityDisbursement {
                        amount_e8s: Some(50),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
            ],
            vec![Err(GovernanceError {
                error_message: "bounded wait timed out".to_string(),
                error_type: 1,
            })],
        );

        let mut first = Box::pin(run_main_tick_with_clients(
            false,
            now_secs * 1_000_000_000,
            now_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(first.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(gov.get_full_neuron_calls(), 1);
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(
            state::with_state(|st| st.prev_age_seconds),
            now_secs - 100,
            "an ambiguous initiation must retain its pre-dispatch age snapshot"
        );

        let later_secs = now_secs + 61;
        let mut second = Box::pin(run_main_tick_with_clients(
            false,
            later_secs * 1_000_000_000,
            later_secs,
            &cfg,
            &ledger,
            &gov,
        ));
        assert!(matches!(
            poll_once(second.as_mut()),
            Poll::Ready(MainTickOutcome::Completed)
        ));
        assert_eq!(gov.get_full_neuron_calls(), 2);
        assert_eq!(
            gov.disburse_calls(),
            1,
            "in-flight source-of-truth state must suppress a second initiation"
        );
        assert_eq!(gov.claim_or_refresh_calls(), 2);
    }
}
