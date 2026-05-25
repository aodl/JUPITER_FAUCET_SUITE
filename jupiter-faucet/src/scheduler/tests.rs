use super::*;
#[cfg(test)]
// Large scheduler tests favor explicit setup values over lint-driven terseness.
#[allow(
    clippy::bool_assert_comparison,
    clippy::clone_on_copy,
    clippy::manual_contains,
    clippy::module_inception,
    clippy::type_complexity,
    clippy::unnecessary_sort_by
)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use candid::Principal;
    use crate::clients::index::{
        account_identifier_text_for_account, GetAccountIdentifierTransactionsResponse,
        IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens,
    };
    use icrc_ledger_types::icrc1::transfer::{BlockIndex, TransferError};
    use std::collections::VecDeque;
    use std::future::{pending, Future};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn assert_no_persistence_batch() {
        assert!(
            !state::persistence_batch_active(),
            "mock async client was invoked while a persistence batch was active"
        );
    }

    struct UnexpectedIndex;

    #[async_trait]
    impl IndexClient for UnexpectedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            assert_no_persistence_batch();
            panic!("index should not be called")
        }
    }

    struct NoopGovernance;

    #[async_trait]
    impl GovernanceClient for NoopGovernance {
        async fn neuron_staking_subaccount(&self, _neuron_id: u64) -> Result<[u8; 32], crate::clients::ClientError> {
            assert_no_persistence_batch();
            panic!("governance should not be called")
        }

        async fn claim_or_refresh_neuron(&self, _neuron_id: u64) -> Result<(), crate::clients::ClientError> {
            assert_no_persistence_batch();
            panic!("governance should not be called")
        }
    }

    struct ScriptedGovernance {
        steps: Arc<Mutex<VecDeque<Result<[u8; 32], crate::clients::ClientError>>>>,
        calls: Arc<Mutex<Vec<u64>>>,
        refresh_calls: Arc<Mutex<Vec<u64>>>,
    }

    impl ScriptedGovernance {
        fn new(steps: Vec<Result<[u8; 32], crate::clients::ClientError>>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
                calls: Arc::new(Mutex::new(Vec::new())),
                refresh_calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<u64> {
            self.calls.lock().unwrap().clone()
        }

        fn refresh_calls(&self) -> Vec<u64> {
            self.refresh_calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl GovernanceClient for ScriptedGovernance {
        async fn neuron_staking_subaccount(&self, neuron_id: u64) -> Result<[u8; 32], crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.calls.lock().unwrap().push(neuron_id);
            self.steps.lock().unwrap().pop_front().expect("missing governance step")
        }

        async fn claim_or_refresh_neuron(&self, neuron_id: u64) -> Result<(), crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.refresh_calls.lock().unwrap().push(neuron_id);
            Ok(())
        }
    }

    struct PendingCmc {
        calls: Arc<AtomicUsize>,
    }

    struct ExistingCanisterStatus {
        existing: Vec<Principal>,
    }

    impl ExistingCanisterStatus {
        fn new(existing: Vec<Principal>) -> Self {
            Self { existing }
        }
    }

    #[async_trait]
    impl CanisterStatusClient for ExistingCanisterStatus {
        async fn canister_exists(&self, canister_id: Principal) -> Result<bool, crate::clients::ClientError> {
            assert_no_persistence_batch();
            Ok(self.existing.iter().any(|existing| *existing == canister_id))
        }
    }

    #[async_trait]
    impl CmcClient for PendingCmc {
        async fn notify_top_up(&self, _canister_id: Principal, _block_index: u64) -> Result<(), crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.calls.fetch_add(1, Ordering::SeqCst);
            pending::<Result<(), crate::clients::ClientError>>().await
        }
    }

    #[derive(Clone)]
    enum LedgerStep {
        Ok(u64),
        Duplicate(u64),
        TemporarilyUnavailable,
        CallErr,
        PermanentErr,
    }

    struct ScriptedLedger {
        steps: Arc<Mutex<VecDeque<LedgerStep>>>,
        transfer_calls: Arc<AtomicUsize>,
        created_at_times: Arc<Mutex<Vec<Option<u64>>>>,
        destinations: Arc<Mutex<Vec<Account>>>,
        memos: Arc<Mutex<Vec<Option<Vec<u8>>>>>,
    }

    impl ScriptedLedger {
        fn new(steps: Vec<LedgerStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
                transfer_calls: Arc::new(AtomicUsize::new(0)),
                created_at_times: Arc::new(Mutex::new(Vec::new())),
                destinations: Arc::new(Mutex::new(Vec::new())),
                memos: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn transfer_calls(&self) -> usize {
            self.transfer_calls.load(Ordering::SeqCst)
        }

        fn created_at_times(&self) -> Vec<Option<u64>> {
            self.created_at_times.lock().unwrap().clone()
        }

        fn destinations(&self) -> Vec<Account> {
            self.destinations.lock().unwrap().clone()
        }

        fn memos(&self) -> Vec<Option<Vec<u8>>> {
            self.memos.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LedgerClient for ScriptedLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { assert_no_persistence_batch(); panic!("fee_e8s should not be called") }
        async fn balance_of_e8s(&self, _account: Account) -> Result<u64, crate::clients::ClientError> { assert_no_persistence_batch(); panic!("balance_of_e8s should not be called") }
        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.transfer_calls.fetch_add(1, Ordering::SeqCst);
            self.created_at_times.lock().unwrap().push(arg.created_at_time);
            self.destinations.lock().unwrap().push(arg.to);
            self.memos.lock().unwrap().push(arg.memo.map(|memo| memo.0.to_vec()));
            match self.steps.lock().unwrap().pop_front().expect("missing ledger step") {
                LedgerStep::Ok(block) => Ok(Ok(BlockIndex::from(block))),
                LedgerStep::Duplicate(block) => Ok(Err(TransferError::Duplicate { duplicate_of: BlockIndex::from(block) })),
                LedgerStep::TemporarilyUnavailable => Ok(Err(TransferError::TemporarilyUnavailable)),
                LedgerStep::CallErr => Err(crate::clients::ClientError::Call("scripted ledger transport failure".to_string())),
                LedgerStep::PermanentErr => Ok(Err(TransferError::BadFee { expected_fee: 10_000u64.into() })),
            }
        }
    }


    #[derive(Clone)]
    struct BalanceRecordingLedger {
        fee_e8s: u64,
        payout_balance_e8s: u64,
        staking_balance_e8s: u64,
        transfer_blocks: Arc<Mutex<VecDeque<u64>>>,
        transfer_amounts: Arc<Mutex<Vec<u64>>>,
    }

    impl BalanceRecordingLedger {
        fn new(fee_e8s: u64, payout_balance_e8s: u64, staking_balance_e8s: u64, transfer_blocks: Vec<u64>) -> Self {
            Self {
                fee_e8s,
                payout_balance_e8s,
                staking_balance_e8s,
                transfer_blocks: Arc::new(Mutex::new(transfer_blocks.into())),
                transfer_amounts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn transfer_amounts(&self) -> Vec<u64> {
            self.transfer_amounts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LedgerClient for BalanceRecordingLedger {
        async fn fee_e8s(&self) -> Result<u64, crate::clients::ClientError> { assert_no_persistence_batch(); Ok(self.fee_e8s) }
        async fn balance_of_e8s(&self, account: Account) -> Result<u64, crate::clients::ClientError> {
            assert_no_persistence_batch();
            let staking = state::with_state(|st| st.config.staking_account.clone());
            if account == staking {
                Ok(self.staking_balance_e8s)
            } else {
                Ok(self.payout_balance_e8s)
            }
        }
        async fn transfer(&self, arg: TransferArg) -> Result<Result<BlockIndex, TransferError>, crate::clients::ClientError> {
            assert_no_persistence_batch();
            let amount_u64 = arg.amount.0.to_string().parse::<u64>().unwrap_or(0);
            self.transfer_amounts.lock().unwrap().push(amount_u64);
            let block = self.transfer_blocks.lock().unwrap().pop_front().unwrap_or(1);
            Ok(Ok(BlockIndex::from(block)))
        }
    }

    #[derive(Clone)]
    enum CmcStep {
        Ok,
        RetryableErr,
        TerminalErr,
    }

    struct ScriptedCmc {
        steps: Arc<Mutex<VecDeque<CmcStep>>>,
        calls: Arc<AtomicUsize>,
    }

    impl ScriptedCmc {
        fn new(steps: Vec<CmcStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl CmcClient for ScriptedCmc {
        async fn notify_top_up(&self, _canister_id: Principal, _block_index: u64) -> Result<(), crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.steps.lock().unwrap().pop_front().expect("missing cmc step") {
                CmcStep::Ok => Ok(()),
                CmcStep::RetryableErr => Err(crate::clients::ClientError::Call("scripted cmc failure".to_string())),
                CmcStep::TerminalErr => Err(crate::clients::ClientError::TerminalNotify("scripted terminal cmc failure".to_string())),
            }
        }
    }

    struct ExclusiveIndex {
        txs: Vec<IndexTransactionWithId>,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl ExclusiveIndex {
        fn new(txs: Vec<IndexTransactionWithId>) -> Self {
            Self { txs, starts: Arc::new(Mutex::new(Vec::new())) }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for ExclusiveIndex {
        async fn get_account_identifier_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.starts.lock().unwrap().push(start);
            let start_idx = match start {
                None => 0,
                Some(last_seen) => self.txs.iter().position(|t| t.id == last_seen).map(|i| i + 1).unwrap_or(self.txs.len()),
            };
            let mut out = Vec::new();
            for tx in self.txs[start_idx..].iter() {
                let include = matches!(&tx.transaction.operation, IndexOperation::Transfer { to, .. } if to == &account_identifier);
                if include {
                    out.push(tx.clone());
                }
                if out.len() >= max_results as usize {
                    break;
                }
            }
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: self.txs.first().map(|tx| tx.id),
                transactions: out,
            })
        }
    }


    struct RecordingIndex {
        txs: Vec<IndexTransactionWithId>,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
    }

    impl RecordingIndex {
        fn new(txs: Vec<IndexTransactionWithId>) -> Self {
            Self { txs, starts: Arc::new(Mutex::new(Vec::new())) }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for RecordingIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.starts.lock().unwrap().push(start);
            let transactions = self
                .txs
                .iter()
                .filter(|tx| start.map(|last_seen| tx.id > last_seen).unwrap_or(true))
                .take(max_results as usize)
                .cloned()
                .collect();
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: self.txs.first().map(|tx| tx.id),
                transactions,
            })
        }
    }


    struct BarrenPagedIndex {
        page_count: u64,
        starts: Arc<Mutex<Vec<Option<u64>>>>,
        staking_id: String,
    }

    impl BarrenPagedIndex {
        fn new(page_count: u64, staking_id: String) -> Self {
            Self {
                page_count,
                starts: Arc::new(Mutex::new(Vec::new())),
                staking_id,
            }
        }

        fn starts(&self) -> Vec<Option<u64>> {
            self.starts.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for BarrenPagedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            assert_no_persistence_batch();
            self.starts.lock().unwrap().push(start);
            let page_idx = start.map(|last_seen| last_seen / PAGE_SIZE).unwrap_or(0);
            if page_idx >= self.page_count {
                return Ok(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    oldest_tx_id: Some(1),
                    transactions: Vec::new(),
                });
            }
            let first_id = page_idx * PAGE_SIZE + 1;
            let transactions = (0..max_results)
                .map(|offset| commitment_tx(first_id + offset, &self.staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
                .collect();
            Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions,
            })
        }
    }


    #[derive(Clone)]
    enum IndexResponseStep {
        Ok(GetAccountIdentifierTransactionsResponse),
        Err,
    }

    struct ScriptedIndex {
        steps: Arc<Mutex<VecDeque<IndexResponseStep>>>,
    }

    impl ScriptedIndex {
        fn new(steps: Vec<IndexResponseStep>) -> Self {
            Self {
                steps: Arc::new(Mutex::new(steps.into())),
            }
        }
    }

    #[async_trait]
    impl IndexClient for ScriptedIndex {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            assert_no_persistence_batch();
            match self.steps.lock().unwrap().pop_front().expect("missing index step") {
                IndexResponseStep::Ok(resp) => Ok(resp),
                IndexResponseStep::Err => Err(crate::clients::ClientError::Call("scripted index failure".to_string())),
            }
        }
    }

    fn commitment_tx_at(id: u64, staking_id: &str, amount_e8s: u64, memo: Option<Vec<u8>>, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: memo,
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "mock-sender".to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn commitment_tx(id: u64, staking_id: &str, amount_e8s: u64, memo: Option<Vec<u8>>) -> IndexTransactionWithId {
        commitment_tx_at(id, staking_id, amount_e8s, memo, 0)
    }

    fn funding_tx_at(id: u64, from: &str, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::Transfer {
                    to: to.to_string(),
                    fee: Tokens::new(10_000),
                    from: from.to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn funding_tx_without_timestamp(id: u64, from: &str, to: &str, amount_e8s: u64) -> IndexTransactionWithId {
        let mut tx = funding_tx_at(id, from, to, amount_e8s, 0);
        tx.transaction.timestamp = None;
        tx.transaction.created_at_time = None;
        tx
    }

    fn test_config_with_intervals(main_interval_seconds: u64, rescue_interval_seconds: u64) -> state::Config {
        state::Config {
            staking_account: Account { owner: Principal::management_canister(), subaccount: None },
            payout_subaccount: None,
            ledger_canister_id: Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").unwrap(),
            index_canister_id: Principal::from_text("qhbym-qaaaa-aaaaa-aaafq-cai").unwrap(),
            cmc_canister_id: Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap(),
            governance_canister_id: Some(Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap()),
            funding_source_account: Account {
                owner: Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai").unwrap(),
                subaccount: None,
            },
            rescue_controller: Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            blackhole_controller: Some(Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap()),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds,
            rescue_interval_seconds,
            min_tx_e8s: crate::MIN_MIN_TX_E8S,
            stake_recognition_delay_seconds: Some(24 * 60 * 60),
        }
    }

    fn test_config() -> state::Config {
        test_config_with_intervals(60, 60)
    }

    fn set_active_job(now_secs: u64, mut job: ActivePayoutJob) -> state::Config {
        let mut cfg = test_config();
        cfg.stake_recognition_delay_seconds = Some(0);
        if job.round_start_time_nanos.is_none()
            && job.round_start_staking_balance_e8s.is_none()
            && job.round_start_latest_tx_id.is_none()
            && job.round_end_time_nanos.is_none()
        {
            job.configure_round_accounting(
                Some(0),
                Some(job.denom_staking_balance_e8s),
                None,
                now_secs.saturating_mul(1_000_000_000),
                None,
                job.denom_staking_balance_e8s,
                true,
            );
        }
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);
        cfg
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
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    fn take_test_logs() -> Vec<String> {
        TEST_LOG_LINES.with(|logs| std::mem::take(&mut *logs.borrow_mut()))
    }

    #[test]
    fn resume_active_job_if_present_runs_when_active_job_exists() {
        let now_secs = 1_000_u64;
        let job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let resume_calls = Arc::new(AtomicUsize::new(0));
        let resume_calls_clone = resume_calls.clone();
        run_ready(resume_active_job_if_present(move || {
            let resume_calls = resume_calls_clone.clone();
            async move {
                resume_calls.fetch_add(1, Ordering::SeqCst);
            }
        }));

        assert_eq!(resume_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resume_active_job_if_present_skips_when_no_active_job_exists() {
        let now_secs = 1_001_u64;
        let cfg = test_config();
        state::set_state(state::State::new(cfg, now_secs));

        let resume_calls = Arc::new(AtomicUsize::new(0));
        let resume_calls_clone = resume_calls.clone();
        run_ready(resume_active_job_if_present(move || {
            let resume_calls = resume_calls_clone.clone();
            async move {
                resume_calls.fetch_add(1, Ordering::SeqCst);
            }
        }));

        assert_eq!(resume_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn stale_main_lease_can_be_reclaimed_without_old_guard_clearing_the_new_lease() {
        let now_secs = 1_000_u64;
        let mut st = state::State::new(test_config(), now_secs);
        let mut job = ActivePayoutJob::new(1, 10_000, 1_000_000, 2_000_000, now_secs * 1_000_000_000);
        job.scan_complete = true;
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(7)]);
        let index = UnexpectedIndex;
        let calls = Arc::new(AtomicUsize::new(0));
        let cmc = PendingCmc { calls: calls.clone() };

        let first_now_nanos = now_secs * 1_000_000_000;
        let mut fut1 = Box::pin(run_main_tick_with_clients(false, first_now_nanos, now_secs, &ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient));
        assert!(matches!(poll_once(fut1.as_mut()), Poll::Pending));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            state::with_state(|st| st.main_lock_state_ts),
            Some(now_secs + MAIN_TICK_LEASE_SECONDS),
        );

        let second_now_secs = now_secs + MAIN_TICK_LEASE_SECONDS + 1;
        let second_now_nanos = second_now_secs * 1_000_000_000;
        let mut fut2 = Box::pin(run_main_tick_with_clients(false, second_now_nanos, second_now_secs, &ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient));
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
    fn transfer_arg_uses_little_endian_top_up_memo() {
        state::clear_skip_ranges();
        state::set_state(state::State::new(test_config(), 0));
        let arg = transfer_arg(
            Account { owner: Principal::management_canister(), subaccount: Some([7u8; 32]) },
            123_456_789,
            10_000,
            42,
            logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec(),
        );
        let memo = arg.memo.expect("memo should be present");
        assert_eq!(memo.0, logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec());
    }

    #[test]
    fn immediate_transfer_retry_reuses_created_at_time_and_succeeds_inline() {
        let now_secs = 1_000_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(1, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(91),
            LedgerStep::Ok(92),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "first commitment should retry inline once and the job should still send the remainder");
        let created_at_times = ledger.created_at_times();
        assert_eq!(created_at_times.len(), 3);
        assert_eq!(created_at_times[0], created_at_times[1], "immediate retry must reuse the original transfer identity");
        assert_ne!(created_at_times[1], created_at_times[2], "later transfers should allocate fresh created_at_time values");
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_990_000);
        assert_eq!(summary.remainder_to_self_e8s, 74_990_000);
        assert_eq!(summary.failed_topups, 0);
    }

    #[test]
    fn immediate_transfer_retry_duplicate_is_treated_as_success() {
        let now_secs = 1_250_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(2, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Duplicate(91),
            LedgerStep::Ok(92),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "duplicate-on-retry should still allow the same job to send the remainder");
        let created_at_times = ledger.created_at_times();
        assert_eq!(created_at_times.len(), 3);
        assert_eq!(created_at_times[0], created_at_times[1], "duplicate retry must reuse the original transfer identity");
        assert_ne!(created_at_times[1], created_at_times[2], "remainder transfer should get its own created_at_time");
        assert_eq!(cmc.call_count(), 2);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 74_990_000);
    }

    #[test]
    fn raw_icp_directive_sends_to_default_account_with_declared_memo_without_cmc_notify() {
        let now_secs = 1_300_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(21, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let compact = beneficiary.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(format!("{compact}.vault42").into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91), LedgerStep::Ok(92)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "raw ICP payout plus remainder should each transfer once");
        assert_eq!(cmc.call_count(), 1, "raw ICP payout should not call notify_top_up; only the remainder is notified");
        let destinations = ledger.destinations();
        assert_eq!(destinations[0], Account { owner: beneficiary, subaccount: None });
        assert_eq!(destinations[1], logic::cmc_deposit_account(cfg.cmc_canister_id, Principal::anonymous()));
        let memos = ledger.memos();
        assert_eq!(memos[0], Some(b"vault42".to_vec()));
        assert_eq!(memos[1], Some(logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 49_990_000);
        assert_eq!(summary.remainder_to_self_e8s, 49_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn raw_icp_directive_allows_empty_transfer_memo_after_dot() {
        let now_secs = 1_350_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(22, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let compact = beneficiary.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(format!("{compact}.").into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91)]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 0);
        assert_eq!(ledger.destinations(), vec![Account { owner: beneficiary, subaccount: None }]);
        assert_eq!(ledger.memos(), vec![Some(Vec::new())]);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 99_990_000);
        assert_eq!(summary.remainder_to_self_e8s, 0);
        assert_eq!(
            state::with_state(|st| st.last_successful_transfer_ts),
            None,
            "raw ICP transfers do not prove the CMC notify health path"
        );
    }

    #[test]
    fn raw_icp_transfer_retry_reuses_identity_destination_and_declared_memo() {
        let now_secs = 1_375_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(23, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let compact = beneficiary.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(format!("{compact}.retry42").into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(91),
            LedgerStep::Ok(92),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "raw ICP retry plus remainder should produce three transfer calls");
        assert_eq!(cmc.call_count(), 1, "raw ICP retry must not notify CMC; only the remainder does");
        let created_at_times = ledger.created_at_times();
        assert_eq!(created_at_times[0], created_at_times[1], "raw ICP immediate retry must reuse created_at_time");
        assert_ne!(created_at_times[1], created_at_times[2], "remainder should allocate its own transfer identity");
        let destinations = ledger.destinations();
        assert_eq!(destinations[0], Account { owner: beneficiary, subaccount: None });
        assert_eq!(destinations[1], Account { owner: beneficiary, subaccount: None });
        assert_eq!(destinations[2], logic::cmc_deposit_account(cfg.cmc_canister_id, Principal::anonymous()));
        let memos = ledger.memos();
        assert_eq!(memos[0], Some(b"retry42".to_vec()));
        assert_eq!(memos[1], Some(b"retry42".to_vec()));
        assert_eq!(memos[2], Some(logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 49_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 49_990_000);
    }

    #[test]
    fn raw_icp_deterministic_ledger_failure_counts_failed_without_cmc_health_attempt() {
        let now_secs = 1_400_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(24, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        state::with_state_mut(|st| st.consecutive_cmc_zero_success_runs = Some(1));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let compact = beneficiary.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(format!("{compact}.raw").into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::PermanentErr,
            LedgerStep::Ok(91),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "deterministic raw ICP rejection should not retry before sending remainder");
        assert_eq!(cmc.call_count(), 1, "only the remainder should call CMC");
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(state::with_state(|st| st.consecutive_cmc_zero_success_runs), Some(1));
    }

    #[test]
    fn raw_icp_retry_exhaustion_counts_ambiguous_without_cmc_health_attempt() {
        let now_secs = 1_425_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(25, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        state::with_state_mut(|st| st.consecutive_cmc_zero_success_runs = Some(1));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let compact = beneficiary.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(format!("{compact}.raw").into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::CallErr,
            LedgerStep::CallErr,
            LedgerStep::Ok(91),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "ambiguous raw ICP transfer should still allow remainder cleanup");
        assert_eq!(cmc.call_count(), 1, "only the remainder should call CMC");
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(state::with_state(|st| st.consecutive_cmc_zero_success_runs), Some(1));
    }

    #[test]
    fn mixed_raw_icp_and_cycles_top_up_job_routes_each_transfer_correctly() {
        let now_secs = 1_450_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(26, 10_000, 200_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let raw_target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let topup_target = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
        let compact_raw = raw_target.to_text().replace('-', "");
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(format!("{compact_raw}.route1").into_bytes())),
            commitment_tx(11, &staking_id, 100_000_000, Some(topup_target.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91), LedgerStep::Ok(92)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2);
        assert_eq!(cmc.call_count(), 1);
        let destinations = ledger.destinations();
        assert_eq!(destinations[0], Account { owner: raw_target, subaccount: None });
        assert_eq!(destinations[1], logic::cmc_deposit_account(cfg.cmc_canister_id, topup_target));
        let memos = ledger.memos();
        assert_eq!(memos[0], Some(b"route1".to_vec()));
        assert_eq!(memos[1], Some(logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.topped_up_sum_e8s, 199_980_000);
        assert_eq!(summary.remainder_to_self_e8s, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn numeric_neuron_id_memo_resolves_staking_subaccount_and_transfers_without_cmc_notify() {
        let now_secs = 1_475_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(27, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let neuron_id = 11_614_578_985_374_291_210_u64;
        let neuron_subaccount = [7u8; 32];
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(neuron_id.to_string().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91)]);
        let cmc = ScriptedCmc::new(vec![]);
        let governance = ScriptedGovernance::new(vec![Ok(neuron_subaccount)]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), vec![neuron_id]);
        assert_eq!(governance.refresh_calls(), vec![neuron_id]);
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 0);
        assert_eq!(
            ledger.destinations(),
            vec![Account {
                owner: cfg.governance_canister_id.expect("governance_canister_id configured"),
                subaccount: Some(neuron_subaccount),
            }]
        );
        assert_eq!(ledger.memos(), vec![Some(logic::MEMO_TOP_UP_CANISTER_U64.to_le_bytes().to_vec())]);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 99_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn governance_lookup_failure_once_then_success_pays_neuron_stake() {
        let now_secs = 1_476_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(270, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let neuron_subaccount = [8u8; 32];
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(b"42".to_vec())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91)]);
        let cmc = ScriptedCmc::new(vec![]);
        let governance = ScriptedGovernance::new(vec![
            Err(crate::clients::ClientError::Call("transient governance failure".to_string())),
            Ok(neuron_subaccount),
        ]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), vec![42, 42]);
        assert_eq!(governance.refresh_calls(), vec![42]);
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(
            ledger.destinations(),
            vec![Account {
                owner: cfg.governance_canister_id.expect("governance_canister_id configured"),
                subaccount: Some(neuron_subaccount),
            }]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn valid_neuron_stake_followed_by_valid_cycles_top_up_completes_both() {
        let now_secs = 1_477_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(271, 10_000, 200_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let neuron_subaccount = [10u8; 32];
        let canister_id = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 100_000_000, Some(b"42".to_vec())),
            commitment_tx(11, &staking_id, 100_000_000, Some(canister_id.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91), LedgerStep::Ok(92)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let governance = ScriptedGovernance::new(vec![Ok(neuron_subaccount)]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), vec![42]);
        assert_eq!(governance.refresh_calls(), vec![42]);
        assert_eq!(ledger.transfer_calls(), 2);
        assert_eq!(cmc.call_count(), 1);
        assert_eq!(
            ledger.destinations(),
            vec![
                Account {
                    owner: cfg.governance_canister_id.expect("governance_canister_id configured"),
                    subaccount: Some(neuron_subaccount),
                },
                logic::cmc_deposit_account(cfg.cmc_canister_id, canister_id),
            ]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.topped_up_sum_e8s, 199_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 0);
    }

    #[test]
    fn governance_lookup_failure_then_cycles_top_up_continues_scanner() {
        let now_secs = 1_478_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(272, 10_000, 160_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let canister_id = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(b"42".to_vec())),
            commitment_tx(11, &staking_id, 80_000_000, Some(canister_id.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91), LedgerStep::Ok(92)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);
        let governance = ScriptedGovernance::new(vec![
            Err(crate::clients::ClientError::Call("first governance failure".to_string())),
            Err(crate::clients::ClientError::Call("second governance failure".to_string())),
        ]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), vec![42, 42]);
        assert_eq!(ledger.transfer_calls(), 2, "cycles top-up plus self-remainder should still be sent");
        assert_eq!(cmc.call_count(), 2);
        assert_eq!(
            ledger.destinations(),
            vec![
                logic::cmc_deposit_account(cfg.cmc_canister_id, canister_id),
                logic::cmc_deposit_account(cfg.cmc_canister_id, Principal::anonymous()),
            ]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
    }

    #[test]
    fn dotted_neuron_id_memo_resolves_staking_subaccount_and_preserves_transfer_memo() {
        let cases = [
            (b"42.vault.memo".to_vec(), b"vault.memo".to_vec()),
            (b"42.".to_vec(), Vec::new()),
            (b"42..memo".to_vec(), b".memo".to_vec()),
        ];

        for (input_memo, expected_transfer_memo) in cases {
            let now_secs = 1_480_u64;
            let cfg = set_active_job(now_secs, ActivePayoutJob::new(29, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000));
            let staking_id = account_identifier_text_for_account(&cfg.staking_account);
            let neuron_subaccount = [9u8; 32];
            let index = ExclusiveIndex::new(vec![
                commitment_tx(10, &staking_id, 100_000_000, Some(input_memo.clone())),
            ]);
            let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91)]);
            let cmc = ScriptedCmc::new(vec![]);
            let governance = ScriptedGovernance::new(vec![Ok(neuron_subaccount)]);

            assert!(
                run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)),
                "payout should complete for memo {:?}",
                String::from_utf8_lossy(&input_memo),
            );
            assert_eq!(governance.calls(), vec![42]);
            assert_eq!(
                ledger.destinations(),
                vec![Account {
                    owner: cfg.governance_canister_id.expect("governance_canister_id configured"),
                    subaccount: Some(neuron_subaccount),
                }]
            );
            assert_eq!(ledger.memos(), vec![Some(expected_transfer_memo)]);
            assert_eq!(cmc.call_count(), 0);
        }
    }

    #[test]
    fn governance_lookup_failure_twice_counts_failed_and_preserves_remainder() {
        let now_secs = 1_485_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(28, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(b"42".to_vec())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(91)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let governance = ScriptedGovernance::new(vec![
            Err(crate::clients::ClientError::Call("not authorized".to_string())),
            Err(crate::clients::ClientError::Call("still not authorized".to_string())),
        ]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), vec![42, 42]);
        assert_eq!(ledger.transfer_calls(), 1, "only the self-remainder transfer should be sent after unresolved neuron lookup");
        assert_eq!(cmc.call_count(), 1);
        assert_eq!(ledger.destinations(), vec![logic::cmc_deposit_account(cfg.cmc_canister_id, Principal::anonymous())]);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
    }

    #[test]
    fn immediate_transfer_retry_failure_counts_once_and_moves_on() {
        let now_secs = 1_500_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(5, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 50_000_000, Some(beneficiary_a.to_text().into_bytes())),
            commitment_tx(11, &staking_id, 60_000_000, Some(beneficiary_b.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(191),
            LedgerStep::Ok(192),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 4);
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 29_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 69_990_000);
    }

    #[test]
    fn transport_failure_retry_exhaustion_counts_once_and_sends_remainder() {
        let now_secs = 1_600_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(6, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::CallErr,
            LedgerStep::CallErr,
            LedgerStep::Ok(291),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "beneficiary should get one immediate retry and then the remainder should still be sent");
        assert_eq!(cmc.call_count(), 1);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn retryable_then_deterministic_transfer_failure_is_still_counted_as_ambiguous() {
        let now_secs = 1_650_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(61, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::CallErr,
            LedgerStep::PermanentErr,
            LedgerStep::Ok(292),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
    }

    #[test]
    fn deterministic_ledger_failure_does_not_retry_and_sends_remainder() {
        let now_secs = 1_700_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(7, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::PermanentErr,
            LedgerStep::Ok(391),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "deterministic ledger rejection should not trigger an immediate retry");
        assert_eq!(cmc.call_count(), 1);
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn immediate_notify_retry_does_not_repeat_ledger_transfer() {
        let now_secs = 3_000_u64;
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(3, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(55)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::Ok]);

        let status_client = ExistingCanisterStatus::new(vec![Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap()]);
        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &status_client, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1, "notify retry must not resend the ledger transfer");
        assert_eq!(cmc.call_count(), 2);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after inline retry");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
    }

    #[test]
    fn immediate_notify_retry_failure_counts_once_and_finalizes() {
        let now_secs = 4_000_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(4, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(88), LedgerStep::Ok(188)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::RetryableErr, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2);
        assert_eq!(cmc.call_count(), 3);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after retry exhaustion");
        assert_eq!(summary.remainder_to_self_e8s, 39_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 1);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn exhausted_terminal_notify_failure_counts_as_failed_after_one_safe_retry() {
        let now_secs = 4_025_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(4025, 10_000, 80_000_000, 160_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 80_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(89), LedgerStep::Ok(189)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::TerminalErr, CmcStep::TerminalErr, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "exhausted terminal notify failures should skip the beneficiary and still send the remainder");
        assert_eq!(cmc.call_count(), 3, "terminal notify failures should get one safe inline retry before the remainder notify");
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized after exhausted terminal notify failure");
        assert_eq!(summary.failed_topups, 1);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.remainder_to_self_e8s, 39_990_000);
        assert_eq!(summary.topped_up_count, 0);
    }

    #[test]
    fn completed_job_counts_beneficiary_zero_success_once_even_if_interrupted_across_ticks() {
        let now_secs = 4_050_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        let mut job = ActivePayoutJob::new(40, 10_000, 10_000, 1, now_secs * 1_000_000_000);
        job.pending_transfer = Some(PendingTransfer {
            notification: PendingNotification {
                kind: TransferKind::Beneficiary,
                beneficiary,
                gross_share_e8s: 10_000,
                amount_e8s: 0,
                block_index: 99,
                next_start: Some(99),
                transfer_memo: None,
                destination_subaccount: None,
                neuron_id: None,
            },
            created_at_time_nanos: now_secs * 1_000_000_000,
            phase: PendingTransferPhase::TransferAccepted,
        });
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let first_tick_cmc = ScriptedCmc::new(vec![CmcStep::RetryableErr, CmcStep::RetryableErr]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &first_tick_cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "accepted pending notifications should not resend ledger transfers");
        assert_eq!(first_tick_cmc.call_count(), 2);
        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(0));
            let job = st.active_payout_job.as_ref().expect("job should remain active until it completes");
            assert!(job.pending_transfer.is_none());
            assert_eq!(job.cmc_attempt_count, Some(2));
            assert_eq!(job.cmc_success_count, Some(0));
            assert_eq!(job.failed_topups, 0);
            assert_eq!(job.ambiguous_topups, 1);
        });

        state::with_state_mut(|st| st.active_payout_job.as_mut().expect("job should still exist").scan_complete = true);
        let second_tick_cmc = ScriptedCmc::new(vec![]);
        let status_client = ExistingCanisterStatus::new(vec![beneficiary]);
        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &second_tick_cmc, &NoopGovernance, &status_client, now_secs * 1_000_000_000, now_secs)));

        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(1));
            assert!(st.active_payout_job.is_none());
            assert_eq!(st.forced_rescue_reason, None);
        });
    }

    #[test]
    fn remainder_success_does_not_reset_beneficiary_zero_success_streak() {
        let now_secs = 4_060_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.consecutive_cmc_zero_success_runs = Some(1);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let mut job = ActivePayoutJob::new(41, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
        job.scan_complete = true;
        job.cmc_attempt_count = Some(2);
        job.cmc_success_count = Some(0);
        job.cmc_attempted_beneficiaries = Some(vec![beneficiary]);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(123)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let status_client = ExistingCanisterStatus::new(vec![beneficiary]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &status_client, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);

        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::CmcZeroSuccessRuns));
            let summary = st.last_summary.as_ref().expect("summary should be finalized");
            assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
            assert_eq!(summary.failed_topups, 0);
        });
    }

    #[test]
    fn zero_success_runs_for_non_canister_targets_do_not_advance_rescue_threshold() {
        let now_secs = 4_065_u64;
        let mut st = state::State::new(test_config(), now_secs);
        st.consecutive_cmc_zero_success_runs = Some(1);
        let beneficiary = Principal::from_text("uuc56-gyb").unwrap();
        let mut job = ActivePayoutJob::new(41065, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
        job.scan_complete = true;
        job.cmc_attempt_count = Some(2);
        job.cmc_success_count = Some(0);
        job.cmc_attempted_beneficiaries = Some(vec![beneficiary]);
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(123)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        let status_client = ExistingCanisterStatus::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &status_client, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_cmc_zero_success_runs, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
        });
    }

    #[test]
    fn stale_pending_transfer_is_marked_ambiguous_without_reusing_an_expired_created_at_time() {
        let now_secs = 3 * 24 * 60 * 60;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let stale_created_at_nanos = (now_secs - 2 * 24 * 60 * 60) * 1_000_000_000;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        let mut job = ActivePayoutJob::new(42, 10_000, 80_000_000, 1, stale_created_at_nanos);
        job.pending_transfer = Some(PendingTransfer {
            notification: PendingNotification {
                kind: TransferKind::Beneficiary,
                beneficiary,
                gross_share_e8s: 40_000_000,
                amount_e8s: 39_990_000,
                block_index: 0,
                next_start: Some(7),
                transfer_memo: None,
                destination_subaccount: None,
                neuron_id: None,
            },
            created_at_time_nanos: stale_created_at_nanos,
            phase: PendingTransferPhase::AwaitingTransfer,
        });
        st.active_payout_job = Some(job);
        state::clear_skip_ranges();
        state::set_state(st);

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);

        assert!(!run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "expired created_at_time should fail before touching the ledger");
        assert_eq!(cmc.call_count(), 0);
        state::with_state(|st| {
            let job = st.active_payout_job.as_ref().expect("job should remain active for inspection");
            assert!(job.pending_transfer.is_none());
            assert_eq!(job.failed_topups, 0);
            assert_eq!(job.ambiguous_topups, 1);
        });
    }

    #[test]
    fn summary_logging_emits_one_compact_line_without_per_transfer_noise() {
        let now_secs = 4_100_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(8, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 50_000_000, Some(beneficiary_a.to_text().into_bytes())),
            commitment_tx(11, &staking_id, 60_000_000, Some(beneficiary_b.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(491),
            LedgerStep::Ok(492),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        take_test_logs();
        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        let logs = take_test_logs();
        assert_eq!(logs.len(), 1, "expected exactly one compact summary log line, got {logs:?}");
        let summary = &logs[0];
        assert!(summary.starts_with("SUMMARY:"), "expected summary log prefix, got {summary}");
        assert!(summary.contains("topped_up_count=1"));
        assert!(summary.contains("failed_topups=0"));
        assert!(summary.contains("ambiguous_topups=1"));
        assert!(summary.contains("remainder_to_self_e8s=69990000"));
        assert!(!summary.contains("ERR:"));
        assert!(!summary.contains("TOPUP"));
    }

    #[test]
    fn summary_log_includes_tranche_identifiers_needed_for_production_verification() {
        let summary = state::Summary {
            funding_tx_id: Some(42),
            funding_amount_e8s: Some(100_000_000),
            pot_start_e8s: 100_000_000,
            round_end_latest_tx_id: Some(41),
            round_end_time_nanos: Some(123_000_000_000),
            effective_denom_staking_balance_e8s: Some(500_000_000),
            last_processed_funding_tx_id: Some(42),
            topped_up_count: 2,
            topped_up_sum_e8s: 99_970_000,
            remainder_to_self_e8s: 10_000,
            pot_remaining_e8s: 0,
            ..Default::default()
        };

        let line = format_summary_log(&summary);
        for field in [
            "funding_tx_id=",
            "funding_amount_e8s=",
            "pot_start_e8s=",
            "round_end_latest_tx_id=",
            "round_end_time_nanos=",
            "effective_denom_e8s=",
            "last_processed_funding_tx_id=",
            "topped_up_count=",
            "topped_up_sum_e8s=",
            "remainder_to_self_e8s=",
            "pot_remaining_e8s=",
        ] {
            assert!(line.contains(field), "summary log missing {field}: {line}");
        }
    }


    #[test]
    fn resumes_pending_transfer_after_upgrade_boundary_before_transfer_outcome_is_known() {
        let now_secs = 3_600_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(7, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::Beneficiary,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 0,
                    next_start: Some(10),
                    transfer_memo: None,
                    destination_subaccount: None,
                neuron_id: None,
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::AwaitingTransfer,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(700)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn resumes_pending_notification_after_upgrade_boundary_without_retransferring() {
        let now_secs = 3_700_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(8, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.gross_outflow_e8s = 24_990_000;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::Beneficiary,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 701,
                    next_start: Some(10),
                    transfer_memo: None,
                    destination_subaccount: None,
                neuron_id: None,
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::TransferAccepted,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0, "accepted transfers should resume at notify without another ledger transfer");
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn resumes_raw_icp_pending_transfer_after_upgrade_boundary_with_declared_memo() {
        let now_secs = 3_750_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(81, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::RawIcp,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 0,
                    next_start: Some(10),
                    transfer_memo: Some(b"resume-raw".to_vec()),
                    destination_subaccount: None,
                neuron_id: None,
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::AwaitingTransfer,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(700)]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 0);
        assert_eq!(ledger.destinations(), vec![Account { owner: beneficiary, subaccount: None }]);
        assert_eq!(ledger.memos(), vec![Some(b"resume-raw".to_vec())]);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn resumes_accepted_raw_icp_pending_transfer_after_upgrade_boundary_without_retransferring_or_notifying() {
        let now_secs = 3_775_u64;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(82, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.gross_outflow_e8s = 24_990_000;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::RawIcp,
                    beneficiary,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 701,
                    next_start: Some(10),
                    transfer_memo: Some(b"accepted-raw".to_vec()),
                    destination_subaccount: None,
                neuron_id: None,
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::TransferAccepted,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 0);
        assert_eq!(cmc.call_count(), 0);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn resumes_neuron_stake_pending_transfer_after_upgrade_boundary_with_resolved_subaccount() {
        let now_secs = 3_800_u64;
        let governance_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let neuron_subaccount = [11u8; 32];
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(83, 10_000, 24_990_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job.pending_transfer = Some(PendingTransfer {
                notification: PendingNotification {
                    kind: TransferKind::NeuronStake,
                    beneficiary: governance_id,
                    gross_share_e8s: 24_990_000,
                    amount_e8s: 24_980_000,
                    block_index: 0,
                    next_start: Some(10),
                    transfer_memo: None,
                    destination_subaccount: Some(neuron_subaccount),
                    neuron_id: Some(42),
                },
                created_at_time_nanos: now_secs * 1_000_000_000,
                phase: PendingTransferPhase::AwaitingTransfer,
            });
            job
        });

        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(700)]);
        let cmc = ScriptedCmc::new(vec![]);
        let governance = ScriptedGovernance::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &governance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(governance.calls(), Vec::<u64>::new());
        assert_eq!(governance.refresh_calls(), vec![42]);
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 0);
        assert_eq!(
            ledger.destinations(),
            vec![Account {
                owner: governance_id,
                subaccount: Some(neuron_subaccount),
            }]
        );
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.topped_up_sum_e8s, 24_980_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }

    #[test]
    fn debug_runtime_reset_and_inline_retry_leave_no_persisted_retry_footprint() {
        let now_secs = 4_200_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(9, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![
            LedgerStep::TemporarilyUnavailable,
            LedgerStep::Ok(591),
            LedgerStep::Ok(592),
        ]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        let (active_job_present, summary_present) = state::with_state(|st| (st.active_payout_job.is_some(), st.last_summary.is_some()));
        assert!(!active_job_present, "inline retry flow should not leave an active job behind once complete");
        assert!(summary_present, "completed job should finalize exactly one summary");
    }


    #[test]
    fn scan_latest_tx_id_accepts_real_index_descending_order() {
        let cfg = test_config();
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            commitment_tx(3, &staking_id, 50_000_000, None),
            commitment_tx(2, &staking_id, 50_000_000, None),
            commitment_tx(1, &staking_id, 50_000_000, None),
        ]);

        let scan = run_ready(scan_latest_tx_id(&index, staking_id, None));
        assert_eq!(scan, LatestScan::Read(Some(3)), "latest scan should treat the first tx on a real-index newest-first page as the latest tx id");
    }

    #[test]
    fn payout_scan_resumes_from_real_index_descending_cursor_without_invariant_failure() {
        let now_secs = 4_260_u64;
        let mut job = ActivePayoutJob::new(10, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(3);
        job.configure_round_accounting(None, None, None, now_secs * 1_000_000_000, None, 100_000_000, true);
        let cfg = set_active_job(now_secs, job);
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(3, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
            commitment_tx(2, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(701), LedgerStep::Ok(702)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(
            run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)),
            "faucet should continue from the next older real-index page instead of treating tx_id 2 after cursor 3 as an invariant failure",
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("payout should complete and summarize");
        assert_eq!(summary.topped_up_count, 1, "the older beneficiary commitment behind the cursor should be paid exactly once");
        assert_eq!(ledger.transfer_calls(), 2, "one beneficiary transfer plus one remainder-to-self transfer should be sent");
        assert_eq!(state::with_state(|st| st.consecutive_index_latest_invariant_failures), Some(0));
    }

    #[test]
    fn payout_scan_skips_newer_descending_txs_until_round_end_boundary_instead_of_stopping() {
        let now_secs = 4_280_u64;
        let mut job = ActivePayoutJob::new(11, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000);
        job.configure_round_accounting(None, None, None, now_secs * 1_000_000_000, Some(3), 100_000_000, true);
        let cfg = set_active_job(now_secs, job);
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = ExclusiveIndex::new(vec![
            commitment_tx(5, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
            commitment_tx(3, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
            commitment_tx(2, &staking_id, 50_000_000, Some(beneficiary.to_text().into_bytes())),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(711), LedgerStep::Ok(712)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 2, "newer txs above the round-end boundary should be skipped while in-boundary and older eligible commitments are still paid");
    }

    #[test]
    fn overlapping_index_pages_do_not_double_count_the_last_seen_tx() {
        let now_secs = 4_300_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(10, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let first = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let second = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai").unwrap();

        let mut first_page = Vec::new();
        for id in 1..500u64 {
            first_page.push(commitment_tx(id, &staking_id, 1, None));
        }
        first_page.push(commitment_tx(500, &staking_id, 50_000_000, Some(first.to_text().into_bytes())));

        let second_page = vec![
            commitment_tx(500, &staking_id, 50_000_000, Some(first.to_text().into_bytes())),
            commitment_tx(501, &staking_id, 50_000_000, Some(second.to_text().into_bytes())),
        ];

        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: first_page,
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: second_page,
            }),
        ]);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(601), LedgerStep::Ok(602), LedgerStep::Ok(603)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 3, "expected two beneficiary transfers plus one self remainder transfer");
        assert_eq!(cmc.call_count(), 3);

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.topped_up_count, 2, "overlapping page replay must not duplicate the tx id 500 commitment");
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ignored_under_threshold, 499);
        assert_eq!(summary.ignored_bad_memo, 0);
    }

    #[test]
    fn scan_latest_tx_id_detects_non_advancing_full_page() {
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| commitment_tx(id, "staking", 1, None)).collect(),
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| commitment_tx(id, "staking", 1, None)).collect(),
            }),
        ]);

        let latest = run_ready(scan_latest_tx_id(&index, "staking".to_string(), Some(10)));
        assert_eq!(latest, LatestScan::InvariantBroken);
    }

    #[test]
    fn process_payout_stops_and_records_invariant_failure_when_index_page_does_not_advance() {
        let now_secs = 4_600_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(12, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        state::with_state_mut(|st| {
            let job = st.active_payout_job.as_mut().expect("active job");
            job.next_start = Some(10);
        });
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let repeated_page: Vec<_> = (11..=(10 + PAGE_SIZE)).map(|id| commitment_tx(id, &staking_id, 1, None)).collect();
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: repeated_page.clone(),
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: repeated_page,
            }),
        ]);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(!run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
        });

        let second_index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(1),
                transactions: (11..=(10 + PAGE_SIZE)).map(|id| commitment_tx(id, &staking_id, 1, None)).collect(),
            }),
        ]);
        assert!(!run_ready(process_payout(&ledger, &second_index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestInvariantBroken));
        });
    }

    #[test]
    fn process_payout_yields_after_bounded_number_of_barren_pages() {
        let now_secs = 4_700_u64;
        let cfg = set_active_job(now_secs, ActivePayoutJob::new(13, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = BarrenPagedIndex::new(MAX_INDEX_PAGES_PER_PAYOUT_TICK + 1, staking_id);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(&ledger, &index, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        let job = state::with_state(|st| st.active_payout_job.clone()).expect("job should remain active after bounded yield");
        assert_eq!(job.scan_complete, false);
        assert_eq!(job.next_start, Some(MAX_INDEX_PAGES_PER_PAYOUT_TICK * PAGE_SIZE));
        assert_eq!(index.starts().len(), MAX_INDEX_PAGES_PER_PAYOUT_TICK as usize);
    }

    #[test]
    fn scan_latest_tx_id_uses_exclusive_start_cursor_contract() {
        let cfg = test_config();
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ExclusiveIndex::new(vec![
            commitment_tx(10, &staking_id, 1, None),
            commitment_tx(11, &staking_id, 1, None),
            commitment_tx(12, &staking_id, 1, None),
        ]);

        let latest = run_ready(scan_latest_tx_id(&index, staking_id, Some(10)));
        assert_eq!(latest, LatestScan::Read(Some(12)));
        assert_eq!(index.starts(), vec![Some(10)]);
    }

    #[test]
    fn latest_invariant_break_still_requires_two_consecutive_observations() {
        let now_secs = 5_000_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::clear_skip_ranges();
        state::set_state(st);

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Read(Some(10))));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(1));
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
        });

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Read(Some(10))));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestInvariantBroken));
        });
    }

    #[test]
    fn latest_unreadable_requires_two_consecutive_observations_and_uses_distinct_reason() {
        let now_secs = 5_100_u64;
        let cfg = test_config();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::clear_skip_ranges();
        state::set_state(st);

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Unreadable));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });

        state::with_state_mut(|st| apply_latest_observation(st, 200, LatestScan::Unreadable));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestUnreadable));
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });
    }

    #[test]
    fn first_page_unreadable_also_requires_two_consecutive_observations() {
        let now_secs = 5_150_u64;
        let cfg = test_config();
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Err,
            IndexResponseStep::Err,
        ]);

        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::clear_skip_ranges();
        state::set_state(st);

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
        });

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(2));
            assert_eq!(st.forced_rescue_reason, Some(ForcedRescueReason::IndexLatestUnreadable));
        });
    }

    #[test]
    fn unreadable_latest_does_not_latch_if_next_observation_confirms_advancement() {
        let now_secs = 5_200_u64;
        let cfg = test_config();
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = ScriptedIndex::new(vec![
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![commitment_tx(10, &staking_id, 100, None)],
            }),
            IndexResponseStep::Err,
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![commitment_tx(10, &staking_id, 100, None)],
            }),
            IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 200,
                oldest_tx_id: Some(10),
                transactions: vec![commitment_tx(11, &staking_id, 100, None)],
            }),
        ]);

        let mut st = state::State::new(cfg.clone(), now_secs);
        st.last_observed_staking_balance_e8s = Some(100);
        st.last_observed_latest_tx_id = Some(10);
        state::clear_skip_ranges();
        state::set_state(st);

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(1));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(100));
            assert_eq!(st.last_observed_latest_tx_id, Some(10));
        });

        run_ready(probe_index_health(&index, &staking_id, 200));
        state::with_state(|st| {
            assert_eq!(st.consecutive_index_latest_unreadable_failures, Some(0));
            assert_eq!(st.consecutive_index_latest_invariant_failures, Some(0));
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.last_observed_staking_balance_e8s, Some(200));
            assert_eq!(st.last_observed_latest_tx_id, Some(11));
        });
    }

    #[test]
    fn remainder_duplicate_still_notifies_and_finalizes_summary() {
        let now_secs = 2_000_u64;
        set_active_job(now_secs, {
            let mut job = ActivePayoutJob::new(2, 10_000, 80_000_000, 1, now_secs * 1_000_000_000);
            job.scan_complete = true;
            job
        });
        let ledger = ScriptedLedger::new(vec![LedgerStep::Duplicate(77)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(&ledger, &UnexpectedIndex, &cmc, &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient, now_secs * 1_000_000_000, now_secs)));
        assert_eq!(ledger.transfer_calls(), 1);
        assert_eq!(cmc.call_count(), 1);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.remainder_to_self_e8s, 79_990_000);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 0);
    }


    #[test]
    fn large_skippable_history_persists_a_single_skip_range() {
        let now_secs = 10_000;
        let job = ActivePayoutJob::new(100, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
            }]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.ignored_under_threshold, MIN_SKIP_RANGE_TX_COUNT);
        assert_eq!(summary.ignored_bad_memo, 0);
        assert_eq!(index.starts().first().copied(), Some(None));
    }

    #[test]
    fn history_below_skip_threshold_does_not_persist_range() {
        let now_secs = 10_100;
        let job = ActivePayoutJob::new(101, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let _cfg = set_active_job(now_secs, job);

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let txs: Vec<_> = (1..MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert!(state::list_skip_ranges().is_empty());
    }

    #[test]
    fn repeated_below_threshold_history_replays_from_start_and_still_reaches_later_qualifying_commitment_without_persisting_skip_ranges() {
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();

        for round in 0..2_u64 {
            let now_secs = 10_150 + round;
            let job = ActivePayoutJob::new(150 + round, 10_000, 100_000_000, 1_000_000_000, now_secs * 1_000_000_000);
            let cfg = set_active_job(now_secs, job);
            let staking_id = account_identifier_text_for_account(&cfg.staking_account);
            let txs: Vec<_> = (1..MIN_SKIP_RANGE_TX_COUNT)
                .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
                .chain(std::iter::once(commitment_tx(
                    MIN_SKIP_RANGE_TX_COUNT,
                    &staking_id,
                    crate::MIN_MIN_TX_E8S,
                    Some(beneficiary.to_text().into_bytes()),
                )))
                .collect();
            let index = RecordingIndex::new(txs);
            let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(400 + round), LedgerStep::Ok(500 + round)]);
            let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

            assert!(run_ready(process_payout(
                &ledger,
                &index,
                &cmc,
                &NoopGovernance, &ExistingCanisterStatus::new(vec![beneficiary.clone()]),
                now_secs * 1_000_000_000,
                now_secs,
            )));

            assert_eq!(index.starts().first().copied(), Some(None), "round {round} should replay from the beginning when no skip range is persisted");
            assert!(state::list_skip_ranges().is_empty(), "round {round} should not persist sub-threshold barren history");
            let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
            assert_eq!(summary.ignored_under_threshold, MIN_SKIP_RANGE_TX_COUNT - 1, "round {round} should still rescan and ignore the same barren span");
            assert_eq!(summary.topped_up_count, 1, "round {round} should still reach the qualifying commitment after replay");
        }
    }

    #[test]
    fn persisted_skip_range_causes_next_run_to_jump_before_fetching_inside_it() {
        let now_secs = 10_200;
        let mut job = ActivePayoutJob::new(7, 10_000, 500_000_000, 500_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(0);
        let _cfg = set_active_job(now_secs, job);
        state::insert_skip_range(SkipRange {
            start_tx_id: 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
        })
        .expect("preexisting skip range should persist");

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let txs = vec![commitment_tx(
            MIN_SKIP_RANGE_TX_COUNT + 1,
            &staking_id,
            500_000_000,
            Some(beneficiary.to_text().into_bytes()),
        )];
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![LedgerStep::Ok(42)]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &ExistingCanisterStatus::new(vec![beneficiary.clone()]),
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(index.starts().first().copied(), Some(Some(MIN_SKIP_RANGE_TX_COUNT)));
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.topped_up_count, 1);
    }

    #[test]
    fn skip_range_persistence_fault_latches_sticky_fault_instead_of_trapping() {
        let now_secs = 10_225;
        let job = ActivePayoutJob::new(8, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        let cfg = set_active_job(now_secs, job);

        state::insert_skip_range(SkipRange {
            start_tx_id: MIN_SKIP_RANGE_TX_COUNT + 1,
            end_tx_id: MIN_SKIP_RANGE_TX_COUNT + 10,
        })
        .expect("conflicting persisted skip range should be installed for the test");

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        state::with_state(|st| {
            assert_eq!(st.forced_rescue_reason, None);
            assert_eq!(st.skip_range_invariant_fault, Some(true));
            assert!(st.active_payout_job.is_some(), "job should remain available for rescue inspection");
        });
        assert_eq!(ledger.transfer_calls(), 0);
        assert_eq!(cmc.call_count(), 0);
    }

    #[test]
    fn desired_rescue_controllers_widens_controllers_when_skip_range_fault_is_latched() {
        let rescue_controller = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let blackhole_controller = Principal::from_text("77deu-baaaa-aaaar-qb6za-cai").unwrap();
        let self_id = Principal::anonymous();

        let desired = desired_rescue_controllers(
            10_000,
            true,
            Some(blackhole_controller),
            Some(9_000),
            rescue_controller,
            false,
            true,
            self_id,
        )
        .expect("skip-range fault should not error")
        .expect("armed blackhole mode should produce a controller set");

        let mut expected = vec![blackhole_controller, rescue_controller, self_id];
        expected.sort_by(|a, b| a.to_text().cmp(&b.to_text()));
        expected.dedup();
        assert_eq!(desired, expected);
    }

    #[test]
    fn desired_rescue_controllers_returns_error_when_armed_without_blackhole_controller() {
        let err = desired_rescue_controllers(
            10_000,
            true,
            None,
            Some(9_000),
            Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap(),
            false,
            true,
            Principal::anonymous(),
        )
        .expect_err("armed blackhole mode without a controller should error");
        assert_eq!(err, 3107);
    }

    #[test]
    fn interrupted_multi_page_skip_candidate_resumes_and_persists_single_range() {
        let now_secs = 10_250;
        let mut job = ActivePayoutJob::new(9, 10_000, 10_000, 1_000_000_000, now_secs * 1_000_000_000);
        job.next_start = None;
        let _cfg = set_active_job(now_secs, job);

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        let first_page = GetAccountIdentifierTransactionsResponse {
            balance: 0,
            oldest_tx_id: Some(1),
            transactions: txs.iter().take(PAGE_SIZE as usize).cloned().collect(),
        };
        let interrupted_index = ScriptedIndex::new(vec![IndexResponseStep::Ok(first_page), IndexResponseStep::Err]);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(!run_ready(process_payout(
            &ledger,
            &interrupted_index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let interrupted_job = state::with_state(|st| st.active_payout_job.clone()).expect("job should remain active after retryable index failure");
        assert_eq!(interrupted_job.next_start, Some(PAGE_SIZE));
        assert_eq!(interrupted_job.skip_candidate_start_tx_id, Some(1));
        assert_eq!(interrupted_job.skip_candidate_end_tx_id, Some(PAGE_SIZE));
        assert_eq!(interrupted_job.skip_candidate_tx_count, PAGE_SIZE);
        assert!(state::list_skip_ranges().is_empty());

        let resuming_index = RecordingIndex::new(txs);
        assert!(run_ready(process_payout(
            &ledger,
            &resuming_index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![SkipRange {
                start_tx_id: 1,
                end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
            }]
        );
        assert_eq!(resuming_index.starts().first().copied(), Some(Some(PAGE_SIZE)));
    }

    #[test]
    fn no_transfer_breaks_skip_span_so_only_long_barren_sides_are_persisted() {
        let now_secs = 10_300;
        let mut job = ActivePayoutJob::new(11, 10_000, 10_000, 1_000_000_000_000, now_secs * 1_000_000_000);
        job.next_start = None;
        let _cfg = set_active_job(now_secs, job);

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let mut txs: Vec<_> = (1..=MIN_SKIP_RANGE_TX_COUNT)
            .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None))
            .collect();
        txs.push(commitment_tx(
            MIN_SKIP_RANGE_TX_COUNT + 1,
            &staking_id,
            crate::MIN_MIN_TX_E8S,
            Some(beneficiary.to_text().into_bytes()),
        ));
        txs.extend(
            ((MIN_SKIP_RANGE_TX_COUNT + 2)..=(2 * MIN_SKIP_RANGE_TX_COUNT + 1))
                .map(|id| commitment_tx(id, &staking_id, crate::MIN_MIN_TX_E8S.saturating_sub(1), None)),
        );
        let index = RecordingIndex::new(txs);
        let ledger = ScriptedLedger::new(vec![]);
        let cmc = ScriptedCmc::new(vec![]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert_eq!(
            state::list_skip_ranges(),
            vec![
                SkipRange {
                    start_tx_id: 1,
                    end_tx_id: MIN_SKIP_RANGE_TX_COUNT,
                },
                SkipRange {
                    start_tx_id: MIN_SKIP_RANGE_TX_COUNT + 2,
                    end_tx_id: 2 * MIN_SKIP_RANGE_TX_COUNT + 1,
                },
            ]
        );
        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.ignored_under_threshold, 2 * MIN_SKIP_RANGE_TX_COUNT);
        assert_eq!(summary.topped_up_count, 0);
    }

    #[test]
    fn process_payout_uses_weighted_effective_denom_and_ignores_post_boundary_tx_ids() {
        let now_secs = 2_000;
        let mut job = ActivePayoutJob::new(77, 10_000, 100_000_000, 1_900_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(1);
        job.configure_round_accounting(
            Some(0),
            Some(1_000_000_000),
            Some(1),
            100_000_000_000,
            Some(2),
            1_000_000_000,
            false,
        );
        let _cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let beneficiary_c = Principal::from_text("rkp4c-7iaaa-aaaaa-aaaca-cai").unwrap();
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 1_000_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            commitment_tx_at(2, &staking_id, 900_000_000, Some(beneficiary_b.to_text().into_bytes()), 90_000_000_000),
            // Same timestamp as tx 2 on purpose: tx-id, not timestamp, defines the round range.
            commitment_tx_at(3, &staking_id, 900_000_000, Some(beneficiary_c.to_text().into_bytes()), 90_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 1_900_000_000, vec![11, 12]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(1_090_000_000));
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.failed_topups, 0);
        assert_eq!(summary.ambiguous_topups, 0);
        assert_eq!(summary.pot_remaining_e8s, 1);
        assert_eq!(ledger.transfer_amounts(), vec![91_733_119, 8_246_880]);
        assert_eq!(cmc.call_count(), 2);
    }

    #[test]
    fn new_payout_job_uses_funding_transfer_as_round_end_boundary() {
        let now_secs = 5_000;
        let mut cfg = test_config();
        let funding_source = Account {
            owner: Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai").unwrap(),
            subaccount: None,
        };
        cfg.funding_source_account = funding_source.clone();
        let mut st = state::State::new(cfg.clone(), now_secs);
        st.current_round_start_time_nanos = Some(1_000_000_000);
        st.current_round_start_staking_balance_e8s = Some(100_000_000);
        st.current_round_start_latest_tx_id = Some(10);
        state::clear_skip_ranges();
        state::set_state(st);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&funding_source);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let funding_timestamp_nanos = 100_000_000_000;
        let funding_amount_e8s = 100_000_000;
        let index = RecordingIndex::new(vec![
            funding_tx_at(20, &funding_source_id, &payout_id, funding_amount_e8s, funding_timestamp_nanos),
            commitment_tx_at(21, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), 20_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, funding_amount_e8s, 200_000_000, vec![51]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let (summary, active_job, round_start_staking_balance_e8s, last_processed_funding_tx_id) = state::with_state(|st| {
            (
                st.last_summary.clone(),
                st.active_payout_job.clone(),
                st.current_round_start_staking_balance_e8s,
                st.last_processed_funding_tx_id,
            )
        });
        assert!(active_job.is_none());
        let summary = summary.expect("summary should be finalized");
        assert_eq!(summary.pot_start_e8s, funding_amount_e8s);
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(summary.topped_up_count, 0);
        assert_eq!(round_start_staking_balance_e8s, Some(100_000_000));
        assert_eq!(last_processed_funding_tx_id, Some(20));
    }

    #[test]
    fn effective_denom_scan_advances_past_descending_post_boundary_pages() {
        let now_secs = 5_100;
        let mut cfg = test_config();
        let funding_source = Account {
            owner: Principal::from_text("uccpi-cqaaa-aaaar-qby3q-cai").unwrap(),
            subaccount: None,
        };
        cfg.funding_source_account = funding_source.clone();
        let st = state::State::new(cfg.clone(), now_secs);
        state::clear_skip_ranges();
        state::set_state(st);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&funding_source);
        let pre_funding_beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let post_funding_beneficiary = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let mut txs = Vec::new();
        for id in (1001..=(1001 + PAGE_SIZE)).rev() {
            txs.push(commitment_tx_at(
                id,
                &staking_id,
                100_000_000,
                Some(post_funding_beneficiary.to_text().into_bytes()),
                20_000_000_000,
            ));
        }
        txs.push(funding_tx_at(1000, &funding_source_id, &payout_id, 100_000_000, 100_000_000_000));
        txs.push(commitment_tx_at(
            999,
            &staking_id,
            100_000_000,
            Some(pre_funding_beneficiary.to_text().into_bytes()),
            0,
        ));
        let index = ExclusiveIndex::new(txs);
        let ledger = BalanceRecordingLedger::new(
            10_000,
            100_000_000,
            (PAGE_SIZE + 2) * 100_000_000,
            vec![61],
        );
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let (summary, active_job, round_start_staking_balance_e8s, last_processed_funding_tx_id) = state::with_state(|st| {
            (
                st.last_summary.clone().expect("summary should be finalized"),
                st.active_payout_job.clone(),
                st.current_round_start_staking_balance_e8s,
                st.last_processed_funding_tx_id,
            )
        });
        assert!(active_job.is_none());
        assert_eq!(summary.pot_start_e8s, 100_000_000);
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
        assert_eq!(cmc.call_count(), 1);
        assert_eq!(round_start_staking_balance_e8s, Some(100_000_000));
        assert_eq!(last_processed_funding_tx_id, Some(1000));
        assert!(
            index.starts().contains(&Some(1002)),
            "scanner should advance past a full descending page of post-boundary txs"
        );
    }

    #[test]
    fn funding_tranche_scan_does_not_process_newer_candidate_when_page_cap_hides_older_unprocessed_transfer() {
        let payout_id = "payout-account".to_string();
        let funding_source_id = "funding-source".to_string();
        let mut steps = Vec::new();
        let mut next_id = MAX_INDEX_PAGES_PER_LATEST_SCAN * PAGE_SIZE + 1_000;
        for page_idx in 0..MAX_INDEX_PAGES_PER_LATEST_SCAN {
            let mut transactions = Vec::new();
            for _ in 0..PAGE_SIZE {
                transactions.push(if page_idx == 0 && transactions.is_empty() {
                    funding_tx_at(next_id, &funding_source_id, &payout_id, 100_000_000, 10_000_000_000)
                } else {
                    funding_tx_at(next_id, "unrelated-source", &payout_id, 100_000_000, 10_000_000_000)
                });
                next_id -= 1;
            }
            steps.push(IndexResponseStep::Ok(GetAccountIdentifierTransactionsResponse {
                balance: 0,
                oldest_tx_id: Some(11),
                transactions,
            }));
        }
        let index = ScriptedIndex::new(steps);

        state::set_state(state::State::new(test_config(), 0));
        let discovery = run_ready(discover_oldest_unprocessed_funding_tranche(
            &index,
            payout_id,
            funding_source_id,
            Some(10),
            10_000,
        ));

        assert!(
            matches!(discovery, FundingDiscovery::InProgress),
            "page-cap exhaustion must not process a newer funding candidate before proving no older unprocessed tranche is hidden past the cap"
        );
        assert!(state::with_state(|st| st.active_funding_scan.is_some()));
    }

    #[test]
    fn funding_discovery_persists_cursor_across_page_cap_and_eventually_finds_oldest_tranche() {
        let now_secs = 4_200;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let spam_count = MAX_FUNDING_SCAN_PAGES_PER_TICK * PAGE_SIZE + 1;
        let mut txs = Vec::new();
        for offset in 0..spam_count {
            let id = spam_count + 20 - offset;
            txs.push(funding_tx_at(id, "public-spammer", &payout_id, 100_000_000, 10_000_000_000));
        }
        txs.push(funding_tx_at(10, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000));
        txs.push(commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), 0));
        let index = ExclusiveIndex::new(txs);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 100_000_000, vec![91]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        let scan_after_first_tick = state::with_state(|st| st.active_funding_scan.clone()).expect("scan should persist");
        assert!(scan_after_first_tick.cursor.is_some());
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), None);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs + 1,
        )));
        assert!(state::with_state(|st| st.active_funding_scan.is_none()));
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), Some(10));
        let summary = state::with_state(|st| st.last_summary.clone().expect("summary should be finalized"));
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(100_000_000));
    }

    #[test]
    fn non_funding_payout_account_spam_cannot_permanently_block_next_disburser_funding_tranche() {
        let now_secs = 4_300;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let spam_count = MAX_FUNDING_SCAN_PAGES_PER_TICK * PAGE_SIZE + 25;
        let mut txs = Vec::new();
        for offset in 0..spam_count {
            let id = spam_count + 20 - offset;
            txs.push(funding_tx_at(id, "public-spammer", &payout_id, 100_000_000, 10_000_000_000));
        }
        txs.push(funding_tx_at(10, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000));
        txs.push(commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), 0));
        let index = ExclusiveIndex::new(txs);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 100_000_000, vec![101]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        for tick in 0..3 {
            assert!(run_ready(process_payout(
                &ledger,
                &index,
                &cmc,
                &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
                100_000_000_000,
                now_secs + tick,
            )));
            if state::with_state(|st| st.last_processed_funding_tx_id) == Some(10) {
                break;
            }
        }
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), Some(10));
        assert_eq!(state::with_state(|st| st.active_funding_scan.clone()), None);
    }

    #[test]
    fn funding_discovery_resets_scan_when_last_processed_cursor_changes() {
        state::set_state(state::State::new(test_config(), 0));
        state::with_state_mut(|st| {
            st.last_processed_funding_tx_id = Some(2);
            st.active_funding_scan = Some(state::FundingScanState {
                anchor_last_processed_funding_tx_id: Some(1),
                cursor: Some(500),
                candidate: Some(state::FundingTrancheState {
                    tx_id: 300,
                    timestamp_nanos: 1,
                    amount_e8s: 100_000_000,
                }),
            });
        });

        let payout_id = "payout-account".to_string();
        let funding_source_id = "funding-source".to_string();
        let mut transactions = Vec::new();
        let spam_count = MAX_FUNDING_SCAN_PAGES_PER_TICK * PAGE_SIZE + 1;
        for id in (1_000..(1_000 + spam_count)).rev() {
            transactions.push(funding_tx_at(id, "public-spammer", &payout_id, 100_000_000, 10_000_000_000));
        }
        let index = ExclusiveIndex::new(transactions);

        let discovery = run_ready(discover_oldest_unprocessed_funding_tranche(
            &index,
            payout_id,
            funding_source_id,
            Some(2),
            10_000,
        ));
        assert!(matches!(discovery, FundingDiscovery::InProgress));
        let scan = state::with_state(|st| st.active_funding_scan.clone()).expect("scan should persist");
        assert_eq!(scan.anchor_last_processed_funding_tx_id, Some(2));
        assert!(scan.cursor.is_some());
        assert_eq!(scan.candidate, None);
    }

    #[test]
    fn funding_discovery_does_not_process_new_head_transfer_before_older_unprocessed_candidate() {
        state::set_state(state::State::new(test_config(), 0));
        state::with_state_mut(|st| {
            st.active_funding_scan = Some(state::FundingScanState {
                anchor_last_processed_funding_tx_id: None,
                cursor: Some(100),
                candidate: Some(state::FundingTrancheState {
                    tx_id: 50,
                    timestamp_nanos: 5,
                    amount_e8s: 100_000_000,
                }),
            });
        });

        let payout_id = "payout-account".to_string();
        let funding_source_id = "funding-source".to_string();
        let index = ExclusiveIndex::new(vec![
            funding_tx_at(200, &funding_source_id, &payout_id, 200_000_000, 20_000_000_000),
            funding_tx_at(50, &funding_source_id, &payout_id, 100_000_000, 5_000_000_000),
        ]);
        let discovery = run_ready(discover_oldest_unprocessed_funding_tranche(
            &index,
            payout_id,
            funding_source_id,
            None,
            10_000,
        ));
        assert_eq!(
            discovery,
            FundingDiscovery::Found(FundingTranche {
                tx_id: 50,
                timestamp_nanos: 5,
                amount_e8s: 100_000_000,
            })
        );
    }

    #[test]
    fn funding_discovery_treats_missing_timestamp_on_qualifying_funding_transfer_as_unreadable() {
        state::set_state(state::State::new(test_config(), 0));
        let payout_id = "payout-account".to_string();
        let funding_source_id = "funding-source".to_string();
        let index = ExclusiveIndex::new(vec![
            funding_tx_at(20, &funding_source_id, &payout_id, 200_000_000, 20_000_000_000),
            funding_tx_without_timestamp(10, &funding_source_id, &payout_id, 100_000_000),
        ]);

        let discovery = run_ready(discover_oldest_unprocessed_funding_tranche(
            &index,
            payout_id,
            funding_source_id,
            None,
            10_000,
        ));

        assert_eq!(
            discovery,
            FundingDiscovery::Unreadable(FundingDiscoveryUnreadableReason::QualifyingFundingTransferMissingTimestamp)
        );
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), None);
    }

    #[test]
    fn funding_tranche_balance_mismatch_latches_forced_rescue_without_creating_job() {
        let now_secs = 4_350;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.last_processed_funding_tx_id = Some(5));

        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let index = ExclusiveIndex::new(vec![
            funding_tx_at(10, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 50_000_000, 100_000_000, vec![111]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));

        let (active_job, last_processed_funding_tx_id, forced_rescue_reason) = state::with_state(|st| {
            (
                st.active_payout_job.clone(),
                st.last_processed_funding_tx_id,
                st.forced_rescue_reason.clone(),
            )
        });
        assert!(active_job.is_none());
        assert_eq!(last_processed_funding_tx_id, Some(5));
        assert_eq!(
            forced_rescue_reason,
            Some(ForcedRescueReason::FundingTrancheBalanceMismatch)
        );
        assert!(ledger.transfer_amounts().is_empty());
        assert_eq!(cmc.call_count(), 0);
    }

    #[test]
    fn unreadable_qualifying_funding_transfer_latches_forced_rescue() {
        let now_secs = 4_360;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));

        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let index = ExclusiveIndex::new(vec![
            funding_tx_at(20, &funding_source_id, &payout_id, 200_000_000, 30_000_000_000),
            funding_tx_without_timestamp(10, &funding_source_id, &payout_id, 100_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 300_000_000, 100_000_000, vec![112]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));

        let (active_job, last_processed_funding_tx_id, forced_rescue_reason) = state::with_state(|st| {
            (
                st.active_payout_job.clone(),
                st.last_processed_funding_tx_id,
                st.forced_rescue_reason.clone(),
            )
        });
        assert!(active_job.is_none());
        assert_eq!(last_processed_funding_tx_id, None);
        assert_eq!(
            forced_rescue_reason,
            Some(ForcedRescueReason::FundingDiscoveryUnreadable)
        );
        assert!(ledger.transfer_amounts().is_empty());
        assert_eq!(cmc.call_count(), 0);
    }

    #[test]
    fn transient_index_error_does_not_immediately_latch_funding_rescue() {
        let now_secs = 4_370;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg, now_secs));
        let index = ScriptedIndex::new(vec![IndexResponseStep::Err]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 100_000_000, vec![113]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(!run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));

        assert_eq!(state::with_state(|st| st.forced_rescue_reason.clone()), None);
        assert!(state::with_state(|st| st.active_payout_job.is_none()));
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), None);
        assert!(ledger.transfer_amounts().is_empty());
        assert_eq!(cmc.call_count(), 0);
    }

    #[test]
    fn process_payout_still_pays_pre_round_commitments_after_effective_denom_prescan() {
        let now_secs = 2_500;
        let mut job = ActivePayoutJob::new(78, 10_000, 100_000_000, 1_400_000_000, now_secs * 1_000_000_000);
        job.next_start = Some(1);
        job.configure_round_accounting(
            Some(10_000_000_000),
            Some(1_400_000_000),
            Some(1),
            100_000_000_000,
            Some(1),
            1_400_000_000,
            false,
        );
        let _cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = { let account = state::with_state(|st| st.config.staking_account.clone()); account_identifier_text_for_account(&account) };
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 400_000_000, Some(beneficiary.to_text().into_bytes()), 0),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 1_400_000_000, vec![31, 32]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be finalized");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(1_400_000_000));
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(summary.remainder_to_self_e8s, 71_418_572);
        assert_eq!(ledger.transfer_amounts(), vec![28_561_428, 71_418_572]);
    }


    #[test]
    fn first_strict_tranche_records_next_round_snapshot_and_noops_without_funding() {
        let now_secs = 3_000;
        let cfg = test_config();
        let st = state::State::new(cfg.clone(), now_secs);
        state::clear_skip_ranges();
        state::set_state(st);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let no_funding_index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), now_secs * 1_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![21, 22]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &no_funding_index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));
        assert!(state::with_state(|st| st.last_summary.is_none()));
        assert!(state::with_state(|st| st.active_payout_job.is_none()));

        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), now_secs * 1_000_000_000),
            funding_tx_at(2, &funding_source_id, &payout_id, 100_000_000, now_secs * 1_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![21, 22]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone()).expect("summary should be recorded");
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(summary.remainder_to_self_e8s, 0);
        assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
        let (round_start_time_nanos, round_start_staking_balance_e8s, round_start_latest_tx_id) = state::with_state(|st| (
            st.current_round_start_time_nanos,
            st.current_round_start_staking_balance_e8s,
            st.current_round_start_latest_tx_id,
        ));
        assert_eq!(round_start_time_nanos, Some(now_secs * 1_000_000_000));
        assert_eq!(round_start_staking_balance_e8s, Some(100_000_000));
        assert_eq!(round_start_latest_tx_id, Some(2));
    }

    #[test]
    fn strict_tranche_job_creation_never_creates_legacy_no_start_nonzero_baseline_shape() {
        let now_nanos = 3_000_000_000_000;
        state::clear_skip_ranges();
        state::set_state(state::State::new(test_config(), 3_000));

        ensure_active_job_with_boundary(
            now_nanos,
            10_000,
            100_000_000,
            250_000_000,
            100_000_000_000,
            Some(20),
            Some(FundingTranche {
                tx_id: 21,
                timestamp_nanos: 100_000_000_000,
                amount_e8s: 100_000_000,
            }),
        );

        let genesis_job = state::with_state(|st| st.active_payout_job.clone().expect("genesis job"));
        assert_eq!(genesis_job.round_start_time_nanos, None);
        assert_eq!(genesis_job.round_start_latest_tx_id, None);
        assert_eq!(genesis_job.round_start_staking_balance_e8s, Some(0));
        assert_eq!(genesis_job.effective_denom_scan_complete, Some(false));
        assert_ne!(genesis_job.round_start_staking_balance_e8s, Some(250_000_000));

        let mut st = state::State::new(test_config(), 3_001);
        st.current_round_start_time_nanos = Some(100_000_000_000);
        st.current_round_start_staking_balance_e8s = Some(250_000_000);
        st.current_round_start_latest_tx_id = Some(20);
        state::set_state(st);

        ensure_active_job_with_boundary(
            now_nanos + 1,
            10_000,
            120_000_000,
            300_000_000,
            200_000_000_000,
            Some(30),
            Some(FundingTranche {
                tx_id: 31,
                timestamp_nanos: 200_000_000_000,
                amount_e8s: 120_000_000,
            }),
        );

        let later_job = state::with_state(|st| st.active_payout_job.clone().expect("later job"));
        assert_eq!(later_job.round_start_time_nanos, Some(100_000_000_000));
        assert_eq!(later_job.round_start_staking_balance_e8s, Some(250_000_000));
        assert!(
            !(later_job.round_start_time_nanos.is_none()
                && later_job.round_start_staking_balance_e8s != Some(0)),
            "strict job creation must not produce the no-start/nonzero-baseline shape"
        );
    }

    #[test]
    fn first_strict_tranche_excludes_post_funding_commitment_from_effective_denominator() {
        let now_secs = 4_000;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            funding_tx_at(2, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000),
            commitment_tx_at(3, &staking_id, 100_000_000, Some(beneficiary_b.to_text().into_bytes()), 21_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![71]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));

        let (summary, round_start_staking_balance_e8s, last_processed_funding_tx_id) = state::with_state(|st| {
            (
                st.last_summary.clone().expect("summary should be finalized"),
                st.current_round_start_staking_balance_e8s,
                st.last_processed_funding_tx_id,
            )
        });
        assert_eq!(summary.pot_start_e8s, 100_000_000);
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(summary.topped_up_count, 1);
        assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);
        assert_eq!(cmc.call_count(), 1);
        assert_eq!(round_start_staking_balance_e8s, Some(100_000_000));
        assert_eq!(last_processed_funding_tx_id, Some(2));
    }

    #[test]
    fn strict_tranche_job_pot_matches_funding_amount() {
        state::clear_skip_ranges();
        state::set_state(state::State::new(test_config(), 3_200));

        ensure_active_job_with_boundary(
            3_200_000_000_000,
            10_000,
            125_000_000,
            250_000_000,
            100_000_000_000,
            Some(20),
            Some(FundingTranche {
                tx_id: 21,
                timestamp_nanos: 100_000_000_000,
                amount_e8s: 125_000_000,
            }),
        );

        let job = state::with_state(|st| st.active_payout_job.clone().expect("strict job"));
        assert_eq!(job.pot_start_e8s, job.funding_amount_e8s.unwrap());
    }

    #[test]
    fn first_strict_tranche_post_funding_commitment_becomes_eligible_for_second_tranche() {
        let now_secs = 4_100;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let txs = vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            funding_tx_at(2, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000),
            commitment_tx_at(3, &staking_id, 100_000_000, Some(beneficiary_b.to_text().into_bytes()), 21_000_000_000),
            funding_tx_at(4, &funding_source_id, &payout_id, 100_000_000, 40_000_000_000),
        ];
        let index = RecordingIndex::new(txs.clone());
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![81]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));
        let first_summary = state::with_state(|st| st.last_summary.clone().expect("first summary"));
        assert_eq!(first_summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(first_summary.topped_up_count, 1);

        let index = RecordingIndex::new(txs);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![82, 83, 84]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok, CmcStep::Ok]);
        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs + 1,
        )));

        let second_summary = state::with_state(|st| st.last_summary.clone().expect("second summary"));
        assert!(second_summary.effective_denom_staking_balance_e8s.unwrap_or(0) > 100_000_000);
        assert_eq!(second_summary.topped_up_count, 2);
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), Some(4));
        assert_eq!(cmc.call_count(), 2);
    }

    #[test]
    fn pre_funding_but_unrecognized_commitment_is_excluded_from_current_tranche_and_weighted_in_next() {
        let now_secs = 4_400;
        let cfg = test_config();
        state::clear_skip_ranges();
        state::set_state(state::State::new(cfg.clone(), now_secs));
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let payout_id = account_identifier_text_for_account(&payout_account());
        let funding_source_id = account_identifier_text_for_account(&cfg.funding_source_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let txs = vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            commitment_tx_at(2, &staking_id, 100_000_000, Some(beneficiary_b.to_text().into_bytes()), 15_000_000_000),
            funding_tx_at(3, &funding_source_id, &payout_id, 100_000_000, 20_000_000_000),
            funding_tx_at(4, &funding_source_id, &payout_id, 100_000_000, 40_000_000_000),
        ];
        let index = RecordingIndex::new(txs.clone());
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![201]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);
        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs,
        )));

        let first_summary = state::with_state(|st| st.last_summary.clone().expect("first summary"));
        assert_eq!(first_summary.effective_denom_staking_balance_e8s, Some(100_000_000));
        assert_eq!(first_summary.topped_up_count, 1);
        assert_eq!(ledger.transfer_amounts(), vec![99_990_000]);

        let index = RecordingIndex::new(txs);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![202, 203, 204]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok, CmcStep::Ok]);
        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance, &crate::clients::canister_info::NoopCanisterStatusClient,
            100_000_000_000,
            now_secs + 1,
        )));

        let second_summary = state::with_state(|st| st.last_summary.clone().expect("second summary"));
        assert_eq!(second_summary.effective_denom_staking_balance_e8s, Some(175_000_000));
        assert_eq!(second_summary.topped_up_count, 2);
        assert_eq!(second_summary.pot_remaining_e8s, 1);
        assert_eq!(ledger.transfer_amounts(), vec![57_132_857, 42_847_142]);
        assert_eq!(state::with_state(|st| st.last_processed_funding_tx_id), Some(4));
    }

    #[test]
    fn denominator_and_payout_amounts_stay_consistent_for_commitments_recognized_between_boundaries() {
        let now_secs = 4_500;
        let mut job = ActivePayoutJob::new(79, 10_000, 100_000_000, 200_000_000, now_secs * 1_000_000_000);
        job.configure_round_accounting(
            Some(20_000_000_000),
            Some(100_000_000),
            Some(3),
            40_000_000_000,
            Some(4),
            100_000_000,
            false,
        );
        let cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(10));

        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let beneficiary_a = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let beneficiary_b = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary_a.to_text().into_bytes()), 0),
            commitment_tx_at(2, &staking_id, 100_000_000, Some(beneficiary_b.to_text().into_bytes()), 15_000_000_000),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 200_000_000, vec![211, 212, 213]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok, CmcStep::Ok, CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        let summary = state::with_state(|st| st.last_summary.clone().expect("summary"));
        let gross_outflow = summary.pot_start_e8s.saturating_sub(summary.pot_remaining_e8s);
        assert!(gross_outflow <= summary.pot_start_e8s);
        assert_eq!(summary.effective_denom_staking_balance_e8s, Some(175_000_000));
        assert_eq!(summary.topped_up_count, 2);
        assert_eq!(summary.pot_remaining_e8s, 1);
    }

    #[test]
    fn payout_hot_path_refuses_to_send_when_gross_outflow_would_exceed_tranche_pot() {
        let now_secs = 4_600;
        let beneficiary = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai").unwrap();
        let mut job = ActivePayoutJob::new(80, 10_000, 100_000_000, 100_000_000, now_secs * 1_000_000_000);
        job.configure_round_accounting(None, Some(0), None, 100_000_000_000, Some(2), 100_000_000, true);
        job.gross_outflow_e8s = 90_000_000;
        let cfg = set_active_job(now_secs, job);
        state::with_state_mut(|st| st.config.stake_recognition_delay_seconds = Some(0));
        let staking_id = account_identifier_text_for_account(&cfg.staking_account);
        let index = RecordingIndex::new(vec![
            commitment_tx_at(1, &staking_id, 100_000_000, Some(beneficiary.to_text().into_bytes()), 0),
        ]);
        let ledger = BalanceRecordingLedger::new(10_000, 100_000_000, 100_000_000, vec![221]);
        let cmc = ScriptedCmc::new(vec![CmcStep::Ok]);

        assert!(run_ready(process_payout(
            &ledger,
            &index,
            &cmc,
            &NoopGovernance,
            &crate::clients::canister_info::NoopCanisterStatusClient,
            now_secs * 1_000_000_000,
            now_secs,
        )));

        assert!(ledger.transfer_amounts().is_empty());
        assert_eq!(cmc.call_count(), 0);
        assert!(state::with_state(|st| st.active_payout_job.is_some()));
        assert_eq!(
            state::with_state(|st| st.forced_rescue_reason.clone()),
            Some(ForcedRescueReason::AccountingInvariantBroken)
        );
    }

}
