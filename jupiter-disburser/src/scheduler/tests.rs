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
    use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    struct UnexpectedLedger;

    #[async_trait]
    impl LedgerClient for UnexpectedLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { panic!("ledger should not be called") }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { panic!("ledger should not be called") }
        async fn transfer(&self, _arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> { panic!("ledger should not be called") }
    }

    struct PendingGovernance {
        get_full_neuron_calls: Arc<AtomicUsize>,
    }

    struct ZeroBalanceLedger;

    #[async_trait]
    impl LedgerClient for ZeroBalanceLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { panic!("fee_e8s should not be called") }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { Ok(0) }
        async fn transfer(&self, _arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            panic!("transfer should not be called")
        }
    }


    struct CountingLedger {
        balance: u64,
        transfer_calls: AtomicUsize,
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
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { Ok(10_000) }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { Ok(self.balance) }
        async fn transfer(&self, _arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
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
            self.refresh_voting_power_calls.fetch_add(1, Ordering::SeqCst);
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
            normal_recipient: Account { owner: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(), subaccount: None },
            age_bonus_recipient_1: Account { owner: Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(), subaccount: None },
            age_bonus_recipient_2: Account { owner: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(), subaccount: None },
            ledger_canister_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            governance_canister_id: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            rescue_controller: Principal::from_text("qaa6y-5yaaa-aaaaa-aaafa-cai").unwrap(),
            blackhole_controller: Some(Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap()),
            blackhole_armed: Some(false),
            main_interval_seconds: 60,
            rescue_interval_seconds: 60,
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker { RawWaker::new(std::ptr::null(), &VTABLE) }
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

    #[test]
    fn stale_main_lease_can_be_reclaimed_without_old_guard_clearing_the_new_lease() {
        let now_secs = 1_000_u64;
        state::set_state(state::State::new(test_config(), now_secs));

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = UnexpectedLedger;
        let calls = Arc::new(AtomicUsize::new(0));
        let gov = PendingGovernance { get_full_neuron_calls: calls.clone() };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(false, first_now_nanos, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        let second_now_secs = now_secs + MAIN_TICK_LEASE_SECONDS + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &cfg, &ledger, &gov));
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

        let mut fut = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
        assert_eq!(gov.claim_or_refresh_calls(), 1, "stake refresh should not depend on get_full_neuron");
        assert_eq!(gov.refresh_voting_power_calls(), 1, "voting-power refresh should run after stake refresh even when get_full_neuron fails");
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

        let mut fut = Box::pin(run_main_tick_with_clients(false, (now_secs + 1) * 1_000_000_000, now_secs + 1, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
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

        let mut fut = Box::pin(run_main_tick_with_clients(false, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
        assert_eq!(ledger.transfer_calls(), 0, "staged ICP must not be routed while a disbursement is already in flight");
        assert_eq!(gov.disburse_calls(), 0, "in-flight source-of-truth state must suppress a second initiation");
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(state::with_state(|st| st.prev_age_seconds), 777, "skipped ticks must preserve the previously captured age snapshot");
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
        fee: u64,
        steps: Mutex<VecDeque<TransferScriptStep>>,
        transfer_calls: AtomicUsize,
    }

    impl BalanceTrackingLedger {
        fn new(balance: u64, fee: u64, steps: Vec<TransferScriptStep>) -> Self {
            Self {
                balance: Mutex::new(balance),
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
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { Ok(self.fee) }

        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> {
            Ok(*self.balance.lock().unwrap())
        }

        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            let step = self.steps.lock().unwrap().pop_front().expect("missing transfer step");
            match step {
                TransferScriptStep::Ok(block) => {
                    let amount = u64::try_from(arg.amount.0).expect("amount should fit in u64 for tests");
                    let fee_nat = arg.fee.expect("tests always include a fee");
                    let fee = u64::try_from(fee_nat.0).expect("fee should fit in u64 for tests");
                    let mut balance = self.balance.lock().unwrap();
                    *balance = balance.saturating_sub(amount.saturating_add(fee));
                    Ok(Ok(block))
                }
                TransferScriptStep::TypedErr(err) => Ok(Err(err)),
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call("scripted transfer transport failure".to_string())),
            }
        }
    }

    #[async_trait]
    impl LedgerClient for ScriptedTransferLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { Ok(self.fee) }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { Ok(self.balance) }
        async fn transfer(&self, _arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            match self.steps.lock().unwrap().pop_front().expect("missing transfer step") {
                TransferScriptStep::Ok(block) => Ok(Ok(block)),
                TransferScriptStep::TypedErr(err) => Ok(Err(err)),
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call("scripted transfer transport failure".to_string())),
            }
        }
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
            Ok(*self.fees.lock().unwrap().front().expect("missing queued fee"))
        }

        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> {
            Ok(self.balance)
        }

        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            let fee_nat = arg.fee.expect("tests always include a fee");
            let fee = u64::try_from(fee_nat.0).expect("fee should fit in u64 for tests");
            self.transfer_fees.lock().unwrap().push(fee);
            match self.steps.lock().unwrap().pop_front().expect("missing transfer step") {
                TransferScriptStep::Ok(block) => Ok(Ok(block)),
                TransferScriptStep::TypedErr(err) => {
                    let mut fees = self.fees.lock().unwrap();
                    if !fees.is_empty() {
                        fees.pop_front();
                    }
                    Ok(Err(err))
                }
                TransferScriptStep::CallErr => Err(crate::clients::ClientError::Call("scripted transfer transport failure".to_string())),
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
                to: Account { owner: Principal::from_text("aaaaa-aa").unwrap(), subaccount: None },
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
            TransferError::InsufficientFunds { balance: Nat::from(0u64) },
            TransferError::GenericError { error_code: Nat::from(5u64), message: "permanent ledger failure".to_string() },
            TransferError::BadBurn { min_burn_amount: Nat::from(10u64) },
        ] {
            let now_secs = 2_100_u64;
            set_test_payout_plan(now_secs);

            let cfg = state::with_state(|st| st.config.clone());
            let ledger = ScriptedTransferLedger::new(50_000_000, 10_000, vec![TransferScriptStep::TypedErr(err)]);
            let gov = ScriptedGovernance::new(
                vec![Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                })],
                vec![],
            );

            let mut fut = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
            assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
            assert_eq!(ledger.transfer_calls(), 1);
            assert!(state::with_state(|st| st.payout_plan.is_none()), "permanent ledger error should clear the persisted plan");
            assert_eq!(gov.claim_or_refresh_calls(), 1, "stake refresh should still run after a failed payout");
        }
    }

    #[test]
    fn transport_errors_keep_persisted_plan_for_safe_retry() {
        let now_secs = 2_200_u64;
        set_test_payout_plan(now_secs);

        let cfg = state::with_state(|st| st.config.clone());
        let ledger = ScriptedTransferLedger::new(50_000_000, 10_000, vec![TransferScriptStep::CallErr]);
        let gov = ScriptedGovernance::new(
            vec![Ok(Neuron {
                aging_since_timestamp_seconds: 100,
                maturity_disbursements_in_progress: None,
                ..Default::default()
            })],
            vec![],
        );

        let mut fut = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
        assert_eq!(ledger.transfer_calls(), 1);
        assert!(state::with_state(|st| st.payout_plan.is_some()), "transport ambiguity should retain the plan for duplicate-safe retry");
        assert_eq!(gov.claim_or_refresh_calls(), 1);
    }

    #[test]
    fn too_old_and_created_in_future_clear_persisted_plan_and_allow_rebuild() {
        for err in [
            TransferError::TooOld,
            TransferError::CreatedInFuture { ledger_time: 9_999u64 },
        ] {
            let now_secs = 2_225_u64;
            set_test_payout_plan(now_secs);

            let cfg = state::with_state(|st| st.config.clone());
            let ledger = ScriptedTransferLedger::new(50_000_000, 10_000, vec![TransferScriptStep::TypedErr(err)]);
            let gov = ScriptedGovernance::new(
                vec![Ok(Neuron {
                    aging_since_timestamp_seconds: 100,
                    maturity_disbursements_in_progress: None,
                    ..Default::default()
                })],
                vec![],
            );

            let mut fut = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
            assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
            assert_eq!(ledger.transfer_calls(), 1);
            assert!(state::with_state(|st| st.payout_plan.is_none()), "typed timing rejection should clear the plan so a later tick can rebuild it");
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
            vec![TransferScriptStep::CallErr, TransferScriptStep::Ok(Nat::from(77u64))],
        );

        assert!(!run_ready(process_payout(&ledger, &cfg, now_secs * 1_000_000_000, now_secs)));
        let persisted = state::with_state(|st| st.payout_plan.clone()).expect("transport ambiguity should retain the original plan");
        assert_eq!(persisted.fee_e8s, 10_000);
        assert_eq!(ledger.fee_calls(), 1, "fee should be read only when the plan is first created");

        assert!(run_ready(process_payout(&ledger, &cfg, (now_secs + 1) * 1_000_000_000, now_secs + 1)));
        assert_eq!(ledger.fee_calls(), 1, "retry should reuse the persisted fee rather than re-reading the ledger fee");
        assert_eq!(ledger.transfer_fees(), vec![10_000, 10_000]);
        assert!(state::with_state(|st| st.payout_plan.is_none()), "successful retry should clear the completed plan");
    }

    #[test]
    fn rebuilt_plan_uses_new_live_fee_after_too_old_clears_stale_plan() {
        let now_secs = 2_240_u64;
        state::set_state(state::State::new(test_config(), now_secs));
        let cfg = state::with_state(|st| st.config.clone());
        let ledger = QueuedFeeLedger::new(
            50_000_000,
            vec![10_000, 20_000],
            vec![TransferScriptStep::TypedErr(TransferError::TooOld), TransferScriptStep::CallErr],
        );

        assert!(!run_ready(process_payout(&ledger, &cfg, now_secs * 1_000_000_000, now_secs)));
        assert!(state::with_state(|st| st.payout_plan.is_none()), "TooOld should clear the stale plan immediately");

        assert!(!run_ready(process_payout(&ledger, &cfg, (now_secs + 1) * 1_000_000_000, now_secs + 1)));
        let rebuilt = state::with_state(|st| st.payout_plan.clone()).expect("transport ambiguity on the rebuilt plan should retain it for inspection");
        assert_eq!(rebuilt.fee_e8s, 20_000, "rebuild should pick up the new live ledger fee");
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

        let mut fut = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(fut.as_mut()), Poll::Ready(())));
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(state::with_state(|st| st.prev_age_seconds), now_secs.saturating_sub(321), "age should still be captured when governance returns Ok(None)");
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
                    to: Account { owner: Principal::from_text("aaaaa-aa").unwrap(), subaccount: None },
                    gross_share_e8s: 50_000_000,
                    amount_e8s: 49_990_000,
                    created_at_time_nanos: now_secs * 1_000_000_000,
                    memo: b"first-leg".to_vec(),
                    status: state::TransferStatus::Pending,
                },
                state::PlannedTransfer {
                    to: Account { owner: Principal::from_text("2vxsx-fae").unwrap(), subaccount: None },
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

        let mut first = Box::pin(run_main_tick_with_clients(true, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(first.as_mut()), Poll::Ready(())));
        assert_eq!(ledger.transfer_calls(), 2, "first run should send one leg, then stop on the terminal rejection");
        assert_eq!(ledger.balance(), 50_000_000, "successful first leg should debit its gross share from staging");
        assert!(state::with_state(|st| st.payout_plan.is_none()), "terminal rejection should clear the stale split so the next tick replans from current balance");
        assert_eq!(state::with_state(|st| st.last_successful_transfer_ts), Some(now_secs));

        let retry_secs = now_secs + 60;
        let mut second = Box::pin(run_main_tick_with_clients(true, retry_secs * 1_000_000_000, retry_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(second.as_mut()), Poll::Ready(())));
        assert_eq!(ledger.transfer_calls(), 3, "second run should attempt exactly one transfer from the replanned remainder before the scripted transport failure");

        let replanned = state::with_state(|st| st.payout_plan.clone()).expect("transport ambiguity should retain the replanned remainder for inspection");
        assert_eq!(replanned.fee_e8s, 10_000);
        let replanned_total_gross: u64 = replanned.transfers.iter().map(|t| t.gross_share_e8s).sum();
        assert!(replanned_total_gross <= 50_000_000, "replanned gross cost must fit within the remaining staging balance");
        assert!(replanned_total_gross < 100_000_000, "replan must not reuse the original pre-partial-success balance");
        assert!(replanned.transfers.iter().all(|t| matches!(t.status, state::TransferStatus::Pending)));
        assert!(replanned.transfers.iter().all(|t| t.created_at_time_nanos >= retry_secs * 1_000_000_000));
        assert_eq!(gov.claim_or_refresh_calls(), 2, "stake refresh should run after each failed payout tick");
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

        let mut first = Box::pin(run_main_tick_with_clients(false, now_secs * 1_000_000_000, now_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(first.as_mut()), Poll::Ready(())));
        assert_eq!(gov.get_full_neuron_calls(), 1);
        assert_eq!(gov.disburse_calls(), 1);
        assert_eq!(gov.claim_or_refresh_calls(), 1);
        assert_eq!(state::with_state(|st| st.prev_age_seconds), 0, "failed initiation must not update prev_age_seconds");

        let later_secs = now_secs + 61;
        let mut second = Box::pin(run_main_tick_with_clients(false, later_secs * 1_000_000_000, later_secs, &cfg, &ledger, &gov));
        assert!(matches!(poll_once(second.as_mut()), Poll::Ready(())));
        assert_eq!(gov.get_full_neuron_calls(), 2);
        assert_eq!(gov.disburse_calls(), 1, "in-flight source-of-truth state must suppress a second initiation");
        assert_eq!(gov.claim_or_refresh_calls(), 2);
    }
}
