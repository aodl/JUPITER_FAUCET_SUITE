use super::*;
#[cfg(test)]
// Scheduler tests use explicit copied principals/accounts to keep fixture setup readable.
#[allow(clippy::clone_on_copy, clippy::module_inception)]
mod tests {
    use super::*;
    use crate::clients::index::{GetAccountIdentifierTransactionsResponse, IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId, Tokens};
    use crate::state::{ActiveCyclesSweep, Config, State};
    use async_trait::async_trait;
    use candid::Principal;
    use futures::executor::block_on;
    use icrc_ledger_types::icrc1::account::Account;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;

    fn principal(text: &str) -> candid::Principal {
        candid::Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account { owner: principal("aaaaa-aa"), subaccount: None }
    }

    fn configure_state(max_index_pages_per_tick: u32) -> String {
        let account = sample_account();
        let staking_id = account_identifier_text_for_account(&account);
        state::set_state(State::new(
            Config {
                staking_account: account,
                output_source_account: Account { owner: principal("uccpi-cqaaa-aaaar-qby3q-cai"), subaccount: None },
                output_account: Account { owner: principal("acjuz-liaaa-aaaar-qb4qq-cai"), subaccount: None },
                rewards_account: Account { owner: principal("alk7f-5aaaa-aaaar-qb4ra-cai"), subaccount: None },
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                cmc_canister_id: Some(principal("rkp4c-7iaaa-aaaaa-aaaca-cai")),
                faucet_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
                blackhole_canister_id: principal("77deu-baaaa-aaaar-qb6za-cai"),
                sns_wasm_canister_id: principal("qaa6y-5yaaa-aaaaa-aaafa-cai"),
                xrc_canister_id: principal("uf6dk-hyaaa-aaaaq-qaaaq-cai"),
                enable_sns_tracking: false,
                scan_interval_seconds: 600,
                cycles_interval_seconds: 604800,
                min_tx_e8s: 100,
                max_cycles_entries_per_canister: 100,
                max_commitment_entries_per_canister: 100,
                max_index_pages_per_tick,
                max_canisters_per_cycles_tick: 25,
            },
            0,
        ));
        staking_id
    }

    fn transfer_to_staking_memo_tx(id: u64, staking_id: &str, memo: Vec<u8>, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(memo),
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }

    fn transfer_to_staking_tx(id: u64, staking_id: &str, beneficiary: candid::Principal, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: Some(beneficiary.to_text().into_bytes()),
                operation: IndexOperation::Transfer {
                    to: staking_id.to_string(),
                    fee: Tokens::new(10_000),
                    from: "sender".into(),
                    amount: Tokens::new(amount_e8s),
                    spender: None,
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }


    fn transfer_between_accounts_tx(id: u64, from: &str, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
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

    fn transfer_from_between_accounts_tx(id: u64, from: &str, to: &str, amount_e8s: u64, timestamp_nanos: u64) -> IndexTransactionWithId {
        IndexTransactionWithId {
            id,
            transaction: IndexTransaction {
                memo: 0,
                icrc1_memo: None,
                operation: IndexOperation::TransferFrom {
                    to: to.to_string(),
                    fee: Tokens::new(10_000),
                    from: from.to_string(),
                    amount: Tokens::new(amount_e8s),
                    spender: "spender".into(),
                },
                created_at_time: None,
                timestamp: Some(IndexTimeStamp { timestamp_nanos }),
            },
        }
    }
    struct MockIndexClient {
        pages: Mutex<VecDeque<GetAccountIdentifierTransactionsResponse>>,
        calls: Mutex<Vec<(String, Option<u64>, u64)>>,
    }

    impl MockIndexClient {
        fn new(pages: Vec<GetAccountIdentifierTransactionsResponse>) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, Option<u64>, u64)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl IndexClient for MockIndexClient {
        async fn get_account_identifier_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.calls.lock().unwrap().push((account_identifier, start, max_results));
            Ok(self
                .pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(GetAccountIdentifierTransactionsResponse {
                    balance: 0,
                    transactions: Vec::new(),
                    oldest_tx_id: None,
                }))
        }
    }


    struct MockSnsWasmClient {
        responses: Mutex<VecDeque<Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError>>>,
        calls: Mutex<u32>,
    }

    impl MockSnsWasmClient {
        fn new(responses: Vec<Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: Mutex::new(0),
            }
        }

        fn calls(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl SnsWasmClient for MockSnsWasmClient {
        async fn list_deployed_snses(&self) -> Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError> {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(crate::clients::sns_wasm::ListDeployedSnsesResponse { instances: Vec::new() }))
        }
    }

    struct MockSnsRootClient {
        responses: Mutex<BTreeMap<Principal, crate::clients::sns_root::GetSnsCanistersSummaryResponse>>,
        calls: Mutex<Vec<Principal>>,
    }

    impl MockSnsRootClient {
        fn new(responses: BTreeMap<Principal, crate::clients::sns_root::GetSnsCanistersSummaryResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Principal> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SnsRootClient for MockSnsRootClient {
        async fn get_sns_canisters_summary(&self, root_id: Principal) -> Result<crate::clients::sns_root::GetSnsCanistersSummaryResponse, crate::clients::ClientError> {
            self.calls.lock().unwrap().push(root_id);
            self.responses
                .lock()
                .unwrap()
                .get(&root_id)
                .cloned()
                .ok_or_else(|| crate::clients::ClientError::Call(format!("missing summary for {}", root_id)))
        }
    }

    fn sns_summary(canister_id: Principal, cycles: u64) -> SnsCanisterSummary {
        SnsCanisterSummary {
            canister_id: Some(canister_id),
            status: Some(crate::clients::sns_root::SnsCanisterStatus { cycles: Some(Nat::from(cycles)) }),
        }
    }


    struct MockBlackholeClient;

    #[async_trait]
    impl BlackholeClient for MockBlackholeClient {
        async fn canister_status(&self, canister_id: Principal) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, crate::clients::ClientError> {
            Ok(crate::clients::blackhole::BlackholeCanisterStatus {
                cycles: Nat::from(0u64),
                settings: crate::clients::blackhole::BlackholeSettings { controllers: vec![canister_id] },
                memory_size: None,
                memory_metrics: None,
            })
        }
    }

    struct RecordingBlackholeClient {
        cycles: u64,
        calls: Mutex<Vec<Principal>>,
    }

    impl RecordingBlackholeClient {
        fn new(cycles: u64) -> Self {
            Self { cycles, calls: Mutex::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<Principal> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl BlackholeClient for RecordingBlackholeClient {
        async fn canister_status(&self, canister_id: Principal) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, crate::clients::ClientError> {
            self.calls.lock().unwrap().push(canister_id);
            Ok(crate::clients::blackhole::BlackholeCanisterStatus {
                cycles: Nat::from(self.cycles),
                settings: crate::clients::blackhole::BlackholeSettings { controllers: vec![canister_id] },
                memory_size: None,
                memory_metrics: None,
            })
        }
    }

    struct FailingBlackholeClient {
        calls: Mutex<Vec<Principal>>,
    }

    impl FailingBlackholeClient {
        fn new() -> Self {
            Self { calls: Mutex::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<Principal> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl BlackholeClient for FailingBlackholeClient {
        async fn canister_status(
            &self,
            canister_id: Principal,
        ) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, crate::clients::ClientError> {
            self.calls.lock().unwrap().push(canister_id);
            Err(crate::clients::ClientError::Call("blackhole status unavailable".into()))
        }
    }

    struct RecordingGovernanceClient {
        calls: Mutex<Vec<[u8; 32]>>,
    }

    impl RecordingGovernanceClient {
        fn new() -> Self {
            Self { calls: Mutex::new(Vec::new()) }
        }

        fn calls(&self) -> Vec<[u8; 32]> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl GovernanceClient for RecordingGovernanceClient {
        async fn claim_or_refresh_neuron_by_subaccount(&self, subaccount: [u8; 32]) -> Result<(), crate::clients::ClientError> {
            self.calls.lock().unwrap().push(subaccount);
            Ok(())
        }
    }

    struct MockXrcClient {
        responses: Mutex<VecDeque<Result<crate::clients::IcpXdrRate, crate::clients::ClientError>>>,
        calls: Mutex<u32>,
    }

    impl MockXrcClient {
        fn new(responses: Vec<Result<crate::clients::IcpXdrRate, crate::clients::ClientError>>) -> Self {
            Self { responses: Mutex::new(VecDeque::from(responses)), calls: Mutex::new(0) }
        }

        fn success(rate: u64, decimals: u32, timestamp: u64) -> Self {
            Self::new(vec![Ok(crate::clients::IcpXdrRate { rate, decimals, timestamp })])
        }

        fn calls(&self) -> u32 { *self.calls.lock().unwrap() }
    }

    #[async_trait]
    impl ExchangeRateClient for MockXrcClient {
        async fn get_icp_xdr_rate(&self) -> Result<crate::clients::IcpXdrRate, crate::clients::ClientError> {
            *self.calls.lock().unwrap() += 1;
            self.responses.lock().unwrap().pop_front().unwrap_or_else(|| Err(crate::clients::ClientError::Call("missing XRC mock response".into())))
        }
    }


    #[test]
    fn icp_xdr_rate_refresh_caches_success_for_one_day() {
        configure_state(10);
        let xrc = MockXrcClient::new(vec![
            Ok(crate::clients::IcpXdrRate { rate: 720_000_000, decimals: 8, timestamp: 1_000 }),
            Ok(crate::clients::IcpXdrRate { rate: 735_000_000, decimals: 8, timestamp: 2_000 }),
        ]);

        block_on(refresh_icp_xdr_rate_if_due(10_000, &xrc)).unwrap();
        state::with_state(|st| {
            let snapshot = st.icp_xdr_rate.as_ref().expect("rate should be cached");
            assert_eq!(snapshot.rate, 720_000_000);
            assert_eq!(snapshot.decimals, 8);
            assert_eq!(snapshot.timestamp, 1_000);
            assert_eq!(snapshot.fetched_at_ts, 10_000);
            assert_eq!(st.last_icp_xdr_rate_error, None);
        });
        assert_eq!(xrc.calls(), 1);

        block_on(refresh_icp_xdr_rate_if_due(10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS - 1, &xrc)).unwrap();
        assert_eq!(xrc.calls(), 1, "fresh daily cache should suppress another XRC call");

        block_on(refresh_icp_xdr_rate_if_due(10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS, &xrc)).unwrap();
        state::with_state(|st| {
            let snapshot = st.icp_xdr_rate.as_ref().expect("rate should be refreshed");
            assert_eq!(snapshot.rate, 735_000_000);
            assert_eq!(snapshot.timestamp, 2_000);
            assert_eq!(snapshot.fetched_at_ts, 10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS);
        });
        assert_eq!(xrc.calls(), 2);
    }

    #[test]
    fn icp_xdr_rate_refresh_records_errors_without_clearing_last_good_rate() {
        configure_state(10);
        let xrc = MockXrcClient::new(vec![
            Ok(crate::clients::IcpXdrRate { rate: 720_000_000, decimals: 8, timestamp: 1_000 }),
            Err(crate::clients::ClientError::Call("NotEnoughCycles".into())),
        ]);

        block_on(refresh_icp_xdr_rate_if_due(10_000, &xrc)).unwrap();
        let err = block_on(refresh_icp_xdr_rate_if_due(10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS, &xrc)).unwrap_err();
        assert!(err.contains("NotEnoughCycles"));
        state::with_state(|st| {
            let snapshot = st.icp_xdr_rate.as_ref().expect("last good rate should remain cached");
            assert_eq!(snapshot.rate, 720_000_000);
            assert_eq!(st.last_icp_xdr_rate_error.as_deref(), Some("inter-canister call failed: NotEnoughCycles"));
        });
        assert_eq!(xrc.calls(), 2);

        block_on(refresh_icp_xdr_rate_if_due(10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS + 1, &xrc)).unwrap();
        assert_eq!(xrc.calls(), 2, "failed XRC refresh should be throttled for one day to prevent a cycle drain");

        block_on(refresh_icp_xdr_rate_if_due(10_000 + (2 * ICP_XDR_RATE_CACHE_TTL_SECONDS), &xrc)).unwrap_err();
        assert_eq!(xrc.calls(), 3, "retry should only happen after failed-attempt TTL expires");
    }

    #[test]
    fn sns_discovery_chunks_across_ticks_and_resumes_from_persisted_state() {
        let _staking_id = configure_state(10);
        let root_a = candid::Principal::from_slice(&[1]);
        let root_b = candid::Principal::from_slice(&[2]);
        let root_c = candid::Principal::from_slice(&[3]);
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10;
            st.config.max_canisters_per_cycles_tick = 2;
            st.last_sns_discovery_ts = 0;
            st.active_sns_discovery = None;
        });
        let sns_wasm = MockSnsWasmClient::new(vec![Ok(crate::clients::sns_wasm::ListDeployedSnsesResponse {
            instances: vec![
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_b.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_a.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_b.clone()) },
                crate::clients::sns_wasm::DeployedSns { root_canister_id: Some(root_c.clone()) },
            ],
        })]);
        let mut summaries = BTreeMap::new();
        summaries.insert(root_a.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_a.clone(), 10)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        summaries.insert(root_b.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_b.clone(), 20)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        summaries.insert(root_c.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_c.clone(), 30)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        let sns_root = MockSnsRootClient::new(summaries);

        block_on(process_sns_discovery(123, 100, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            let active = st.active_sns_discovery.as_ref().expect("discovery should remain in progress after first batch");
            assert_eq!(active.root_canister_ids, vec![root_a.clone(), root_b.clone(), root_c.clone()]);
            assert_eq!(active.next_index, 2);
            assert_eq!(st.last_sns_discovery_ts, 0);
            assert!(st.distinct_canisters.contains(&root_a));
            assert!(st.distinct_canisters.contains(&root_b));
            assert!(!st.distinct_canisters.contains(&root_c));
        });
        assert_eq!(sns_wasm.calls(), 1);
        assert_eq!(sns_root.calls(), vec![root_a.clone(), root_b.clone()]);

        block_on(process_sns_discovery(456, 101, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 101);
            assert!(st.distinct_canisters.contains(&root_c));
            let history = st.cycles_history.get(&root_c).expect("cycles history for final root");
            assert_eq!(history.last().map(|sample| sample.cycles), Some(30));
        });
        assert_eq!(sns_wasm.calls(), 1, "deployed SNS roots should be fetched only once per discovery sweep");
        assert_eq!(sns_root.calls(), vec![root_a.clone(), root_b.clone(), root_c.clone()]);
    }

    #[test]
    fn active_sns_discovery_resumes_even_when_interval_is_not_due() {
        let _staking_id = configure_state(10);
        let root_a = candid::Principal::from_slice(&[1]);
        let root_b = candid::Principal::from_slice(&[2]);
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10_000;
            st.config.max_canisters_per_cycles_tick = 1;
            st.last_sns_discovery_ts = 9_999;
            st.active_sns_discovery = Some(ActiveSnsDiscovery {
                started_at_ts_nanos: 55,
                root_canister_ids: vec![root_a.clone(), root_b.clone()],
                next_index: 1,
            });
            st.last_completed_cycles_sweep_ts = 10_000;
        });
        let index = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse { balance: 0, transactions: Vec::new(), oldest_tx_id: None }]);
        let blackhole = MockBlackholeClient;
        let sns_wasm = MockSnsWasmClient::new(vec![]);
        let mut summaries = BTreeMap::new();
        summaries.insert(root_b.clone(), crate::clients::sns_root::GetSnsCanistersSummaryResponse { root: Some(sns_summary(root_b.clone(), 44)), governance: None, ledger: None, swap: None, index: None, dapps: Vec::new(), archives: Vec::new() });
        let sns_root = MockSnsRootClient::new(summaries);
        let governance = RecordingGovernanceClient::new();

        let xrc = MockXrcClient::success(720_000_000, 8, 9_900);
        block_on(run_main_tick_with_clients(999, 10_000, &index, &blackhole, &sns_wasm, &sns_root, &governance, &xrc)).unwrap();
        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 10_000);
            assert!(st.distinct_canisters.contains(&root_b));
        });
        assert_eq!(sns_wasm.calls(), 0, "resumed discovery should not refetch deployed SNS roots");
        assert_eq!(sns_root.calls(), vec![root_b.clone()]);
    }

    #[test]
    fn indexing_single_qualifying_commitment_updates_counts() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000)],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(42));
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.last_index_run_ts, Some(200));
            assert!(st.canister_sources.get(&beneficiary).unwrap().contains(&CanisterSource::MemoCommitment));
        });
    }

    #[test]
    fn new_qualifying_commitment_enqueues_initial_cycles_probe_without_resetting_full_sweep() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let existing = principal("acjuz-liaaa-aaaar-qb4qq-cai");
        state::with_state_mut(|st| {
            st.last_completed_cycles_sweep_ts = 10_000;
            st.config.cycles_interval_seconds = 3_600;
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 55,
                canisters: vec![existing],
                next_index: 0,
            });
        });
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000)],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 20_000)).unwrap();

        state::with_state(|st| {
            let active = st.active_cycles_sweep.as_ref().expect("active sweep should not be reset");
            assert_eq!(active.started_at_ts_nanos, 55);
            assert_eq!(active.canisters, vec![existing]);
            assert_eq!(active.next_index, 0);
            assert_eq!(st.last_completed_cycles_sweep_ts, 10_000);
            assert_eq!(st.initial_cycles_probe_queue, vec![beneficiary]);
        });
    }

    #[test]
    fn initial_cycles_probe_queue_probes_only_queued_canister() {
        let _staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let existing = principal("acjuz-liaaa-aaaar-qb4qq-cai");
        let staking_subaccount = [7u8; 32];
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.config.staking_account.subaccount = Some(staking_subaccount);
            st.last_completed_cycles_sweep_ts = 10_000;
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 55,
                canisters: vec![existing],
                next_index: 0,
            });
            st.distinct_canisters.insert(beneficiary);
            st.canister_sources.insert(
                beneficiary,
                std::iter::once(CanisterSource::MemoCommitment).collect(),
            );
            st.commitment_history.insert(
                beneficiary,
                vec![crate::state::CommitmentSample {
                    tx_id: 42,
                    timestamp_nanos: Some(123_000_000_000),
                    amount_e8s: 150,
                    counts_toward_faucet: true,
                }],
            );
            st.initial_cycles_probe_queue.push(beneficiary);
        });
        let blackhole = RecordingBlackholeClient::new(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(999_000_000_000, 999, &blackhole, &governance)).unwrap();

        state::with_state(|st| {
            assert_eq!(blackhole.calls(), vec![beneficiary]);
            assert_eq!(governance.calls(), vec![staking_subaccount], "targeted registration probe should refresh the staking neuron directly via NNS governance");
            assert!(st.initial_cycles_probe_queue.is_empty());
            assert_eq!(st.last_completed_cycles_sweep_ts, 10_000);
            assert!(st.active_cycles_sweep.is_some(), "targeted first probe should not disturb active full sweep");
            assert_eq!(
                st.cycles_history
                    .get(&beneficiary)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(777)
            );
        });
    }

    #[test]
    fn cycles_probe_falls_back_from_original_to_configured_blackhole() {
        configure_state(10);
        let canister_id = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let original = FailingBlackholeClient::new();
        let configured = RecordingBlackholeClient::new(888);
        let blackhole = FallbackBlackholeClient::new(&original, &configured);

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            canister_id,
            100,
            None,
            &blackhole,
        ))
        .unwrap();

        state::with_state(|st| {
            assert_eq!(original.calls(), vec![canister_id]);
            assert_eq!(configured.calls(), vec![canister_id]);
            assert_eq!(
                st.cycles_history
                    .get(&canister_id)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(888)
            );
            let meta = st.per_canister_meta.get(&canister_id).expect("probe metadata should be recorded");
            assert_eq!(
                meta.last_cycles_probe_result,
                Some(CyclesProbeResult::Ok(CyclesSampleSource::BlackholeStatus))
            );
        });
    }

    #[test]
    fn original_blackhole_fallback_is_only_for_secure_mainnet_blackhole() {
        assert!(should_try_original_blackhole_first(principal(
            "77deu-baaaa-aaaar-qb6za-cai"
        )));
        assert!(!should_try_original_blackhole_first(principal(
            "e3mmv-5qaaa-aaaah-aadma-cai"
        )));
        assert!(!should_try_original_blackhole_first(principal(
            "ryjl3-tyaaa-aaaaa-aaaba-cai"
        )));
    }

    #[test]
    fn cycles_sweep_targets_memo_canisters_with_lazy_stable_commitment_history() {
        let _staking_id = configure_state(10);
        let self_id = principal("aaaaa-aa");
        let beneficiary = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(beneficiary);
            st.canister_sources.insert(
                beneficiary,
                crate::logic::merge_sources(None, CanisterSource::MemoCommitment),
            );
            st.commitment_history.insert(
                beneficiary,
                vec![crate::state::CommitmentSample {
                    tx_id: 91,
                    timestamp_nanos: Some(910_000_000_000),
                    amount_e8s: 100_000_000,
                    counts_toward_faucet: true,
                }],
            );
        });

        let restored = state::restore_state_from_stable().expect("expected stable root state");
        assert!(restored.commitment_history.is_empty());
        state::set_state_root_only(restored);

        state::with_state(|st| {
            assert_eq!(build_cycles_sweep_canisters(st, self_id), vec![self_id, beneficiary]);
        });
    }

    #[test]
    fn indexing_duplicate_tx_does_not_double_count() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let tx = transfer_to_staking_tx(42, &staking_id, beneficiary, 150, 123_000_000_000);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![tx.clone()],
                oldest_tx_id: Some(42),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![tx],
                oldest_tx_id: Some(42),
            },
        ]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();
        block_on(process_commitment_indexing(&mock, 201)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
        });
    }

    #[test]
    fn indexing_duplicate_raw_icp_and_neuron_txs_do_not_double_count() {
        let staking_id = configure_state(10);
        let raw_canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let raw_memo = format!("{}.vault42", raw_canister.to_text().replace('-', ""));
        let raw_tx = transfer_to_staking_memo_tx(42, &staking_id, raw_memo.into_bytes(), 150, 123_000_000_000);
        let neuron_tx = transfer_to_staking_memo_tx(43, &staking_id, b"42.local.memo".to_vec(), 160, 124_000_000_000);

        apply_indexed_commitment_tx(&raw_tx, &staking_id, 100, 200);
        apply_indexed_commitment_tx(&neuron_tx, &staking_id, 100, 200);
        apply_indexed_commitment_tx(&raw_tx, &staking_id, 100, 201);
        apply_indexed_commitment_tx(&neuron_tx, &staking_id, 100, 201);

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_neuron_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.raw_icp_commitment_history.get(&raw_canister).unwrap().len(), 1);
            assert_eq!(st.neuron_commitment_history.get(&42).unwrap().len(), 1);
        });

        let restored = state::restore_state_from_stable().expect("expected stable root state");
        assert!(restored.raw_icp_commitment_history.is_empty());
        assert!(restored.neuron_commitment_history.is_empty());
        state::set_state_root_only(restored);

        apply_indexed_commitment_tx(&raw_tx, &staking_id, 100, 202);
        apply_indexed_commitment_tx(&neuron_tx, &staking_id, 100, 202);

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.raw_icp_commitment_history.get(&raw_canister).unwrap().len(), 1);
            assert_eq!(st.neuron_commitment_history.get(&42).unwrap().len(), 1);
        });
    }

    #[test]
    fn indexing_uses_cursor_and_keeps_recent_commitments_descending() {
        let staking_id = configure_state(1);
        let first_canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let second_canister = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(10, &staking_id, first_canister, 100, 100_000_000_000)],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(11, &staking_id, second_canister, 200, 300_000_000_000)],
                oldest_tx_id: Some(11),
            },
        ]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();
        block_on(process_commitment_indexing(&mock, 201)).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, None);
        assert_eq!(calls[1].1, Some(10));

        state::with_state(|st| {
            let recent = st.recent_commitments.as_ref().unwrap();
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].tx_id, 11);
            assert_eq!(recent[1].tx_id, 10);
            assert_eq!(st.last_indexed_staking_tx_id, Some(11));
        });
    }

    #[test]
    fn route_indexing_counts_only_protocol_routed_output_and_rewards_and_resumes_across_ticks() {
        let _staking_id = configure_state(10);
        let (source, output, rewards) = state::with_state(|st| (
            st.config.output_source_account.clone(),
            st.config.output_account.clone(),
            st.config.rewards_account.clone(),
        ));
        let source_id = account_identifier_text_for_account(&source);
        let output_id = account_identifier_text_for_account(&output);
        let rewards_id = account_identifier_text_for_account(&rewards);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_between_accounts_tx(10, &source_id, &output_id, 111_000_000, 10),
                    transfer_between_accounts_tx(11, "third-party", &output_id, 999_000_000, 11),
                ],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_between_accounts_tx(20, &source_id, &rewards_id, 22_000_000, 20),
                    transfer_between_accounts_tx(21, "third-party", &rewards_id, 333_000_000, 21),
                ],
                oldest_tx_id: Some(20),
            },
        ]);

        block_on(process_route_indexing(100, 200, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.total_rewards_e8s, Some(0));
            assert_eq!(st.last_indexed_output_tx_id, Some(11));
            assert_eq!(st.last_indexed_rewards_tx_id, None);
            let active = st.active_route_sweep.as_ref().expect("route sweep should continue to rewards");
            assert_eq!(active.next_index, 1);
        });

        block_on(process_route_indexing(101, 201, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.total_rewards_e8s, Some(22_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(11));
            assert_eq!(st.last_indexed_rewards_tx_id, Some(21));
            assert!(st.active_route_sweep.is_none());
            assert_eq!(st.last_completed_route_sweep_ts, Some(201));
        });

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, output_id);
        assert_eq!(calls[1].0, rewards_id);
    }

    #[test]
    fn route_indexing_counts_transfer_from_and_skips_repeated_cursor_without_double_counting() {
        let _staking_id = configure_state(1);
        let (source, output, rewards) = state::with_state(|st| (
            st.config.output_source_account.clone(),
            st.config.output_account.clone(),
            st.config.rewards_account.clone(),
        ));
        let source_id = account_identifier_text_for_account(&source);
        let output_id = account_identifier_text_for_account(&output);
        let rewards_id = account_identifier_text_for_account(&rewards);
        let filler: Vec<_> = (11..(10 + PAGE_SIZE))
            .map(|id| transfer_between_accounts_tx(id, "third-party", &output_id, 1_000, id))
            .collect();
        let mut first_page = vec![transfer_from_between_accounts_tx(10, &source_id, &output_id, 111_000_000, 10)];
        first_page.extend(filler);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: first_page,
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_from_between_accounts_tx(10 + PAGE_SIZE - 1, &source_id, &output_id, 999_000_000, 20),
                    transfer_between_accounts_tx(10 + PAGE_SIZE, &source_id, &output_id, 22_000_000, 21),
                    transfer_between_accounts_tx(10 + PAGE_SIZE + 1, "third-party", &output_id, 333_000_000, 22),
                ],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![transfer_between_accounts_tx(30, &source_id, &rewards_id, 5_000_000, 30)],
                oldest_tx_id: Some(30),
            },
        ]);

        block_on(process_route_indexing(100, 200, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(10 + PAGE_SIZE - 1));
            assert_eq!(st.active_route_sweep.as_ref().map(|active| active.next_index), Some(0));
        });

        block_on(process_route_indexing(101, 201, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000), "repeated cursor tx should be skipped while the new routed transfer is counted once");
            assert_eq!(st.last_indexed_output_tx_id, Some(10 + PAGE_SIZE + 1));
            assert_eq!(st.active_route_sweep.as_ref().map(|active| active.next_index), Some(1));
        });

        block_on(process_route_indexing(102, 202, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000));
            assert_eq!(st.total_rewards_e8s, Some(5_000_000));
            assert_eq!(st.last_indexed_rewards_tx_id, Some(30));
            assert!(st.active_route_sweep.is_none());
        });
    }


    #[test]
    fn non_monotonic_commitment_page_latches_fault_and_stops_indexing() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(50);
            st.oldest_indexed_staking_tx_id = Some(50);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(true);
        });
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 300,
            transactions: vec![
                transfer_to_staking_tx(51, &staking_id, beneficiary, 150, 124_000_000_000),
                transfer_to_staking_tx(49, &staking_id, beneficiary, 150, 123_000_000_000),
            ],
            oldest_tx_id: Some(49),
        }]);

        let err = block_on(process_commitment_indexing(&mock, 200)).unwrap_err();
        assert!(err.contains("non-monotonic"));
        state::with_state(|st| {
            let fault = st.commitment_index_fault.as_ref().expect("fault should be latched");
            assert_eq!(fault.observed_at_ts, 200);
            assert_eq!(fault.last_cursor_tx_id, Some(51));
            assert_eq!(fault.offending_tx_id, 49);
            assert_eq!(st.last_indexed_staking_tx_id, Some(51));
            assert_eq!(st.last_index_run_ts, Some(0));
        });
    }

    #[test]
    fn commitment_index_fault_clears_automatically_once_index_order_recovers() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(50);
            st.oldest_indexed_staking_tx_id = Some(50);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(true);
        });
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 150,
                transactions: vec![
                transfer_to_staking_tx(51, &staking_id, beneficiary, 150, 124_000_000_000),
                transfer_to_staking_tx(49, &staking_id, beneficiary, 150, 123_000_000_000),
            ],
                oldest_tx_id: Some(49),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 450,
                transactions: vec![
                    transfer_to_staking_tx(51, &staking_id, beneficiary, 300, 124_000_000_000),
                    transfer_to_staking_tx(52, &staking_id, beneficiary, 450, 125_000_000_000),
                ],
                oldest_tx_id: Some(51),
            },
        ]);

        let err = block_on(process_commitment_indexing(&mock, 200)).unwrap_err();
        assert!(err.contains("non-monotonic"));
        state::with_state(|st| {
            let fault = st.commitment_index_fault.as_ref().expect("fault should be latched");
            assert_eq!(fault.observed_at_ts, 200);
            assert_eq!(fault.last_cursor_tx_id, Some(51));
            assert_eq!(fault.offending_tx_id, 49);
            assert_eq!(st.last_indexed_staking_tx_id, Some(51));
        });

        block_on(process_commitment_indexing(&mock, 201)).unwrap();
        state::with_state(|st| {
            assert!(st.commitment_index_fault.is_none(), "fault should auto-clear after a clean retry");
            assert_eq!(st.last_indexed_staking_tx_id, Some(52));
            assert_eq!(st.last_index_run_ts, Some(201));
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(2));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 52);
        });
    }

    #[test]
    fn indexing_retains_non_qualifying_and_invalid_memo_commitments_in_separate_recent_lists_without_registering_under_threshold_canisters() {
        let staking_id = configure_state(10);
        let qualifying = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let low_amount = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 410,
            transactions: vec![
                transfer_to_staking_tx(42, &staking_id, qualifying, 150, 123_000_000_000),
                transfer_to_staking_tx(43, &staking_id, low_amount, 50, 124_000_000_000),
                transfer_to_staking_memo_tx(44, &staking_id, b"not-a-principal".to_vec(), 210, 125_000_000_000),
            ],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(
                st.recent_under_threshold_commitments
                    .as_ref()
                    .map(|items| items.len()),
                Some(1),
            );
            assert_eq!(st.recent_invalid_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.recent_under_threshold_commitments.as_ref().unwrap()[0].tx_id, 43);
            assert!(!st.canister_sources.contains_key(&low_amount));
            assert!(!st.distinct_canisters.contains(&low_amount));
            assert!(!st.commitment_history.contains_key(&low_amount));
            let invalid = &st.recent_invalid_commitments.as_ref().unwrap()[0];
            assert_eq!(invalid.tx_id, 44);
            assert_eq!(invalid.memo_text, crate::logic::INVALID_MEMO_PLACEHOLDER);
        });
    }

    #[test]
    fn indexing_caps_under_threshold_recent_list_without_registering_distinct_memo_beneficiaries() {
        let staking_id = configure_state(10);
        let pages = vec![GetAccountIdentifierTransactionsResponse {
            balance: 10_000,
            transactions: (1..=105)
                .map(|tx_id| {
                    let canister = candid::Principal::from_slice(&[1, (tx_id % 251 + 1) as u8]);
                    transfer_to_staking_tx(
                        tx_id,
                        &staking_id,
                        canister,
                        5,
                        tx_id * 1_000_000_000,
                    )
                })
                .collect(),
            oldest_tx_id: Some(1),
        }];
        let mock = MockIndexClient::new(pages);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            let recent = st
                .recent_under_threshold_commitments
                .as_ref()
                .expect("under-threshold recent list should exist");
            assert_eq!(recent.len(), MAX_RECENT_UNDER_THRESHOLD_COMMITMENTS);
            assert_eq!(recent[0].tx_id, 105);
            assert_eq!(recent.last().map(|item| item.tx_id), Some(6));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(0));
            assert_eq!(st.canister_sources.len(), 0);
            assert_eq!(st.distinct_canisters.len(), 0);
            assert!(st.commitment_history.is_empty());
            assert_eq!(st.qualifying_commitment_count, Some(0));
        });
    }

    #[test]
    fn indexing_registers_new_qualifying_canisters_without_pruning_existing_beneficiaries() {
        let staking_id = configure_state(10);
        let existing = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(existing);
            st.canister_sources
                .insert(existing, crate::logic::merge_sources(None, CanisterSource::MemoCommitment));
            st.commitment_history.insert(
                existing,
                vec![crate::state::CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 100,
                    counts_toward_faucet: true,
                }],
            );
            st.qualifying_commitment_count = Some(1);
        });
        let new_canister = candid::Principal::from_slice(&[251, 251, 251]);
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(9_999, &staking_id, new_canister, 150, 123_000_000_000)],
            oldest_tx_id: Some(9_999),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().map(|items| items.len()), Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 9_999);
            assert!(st.canister_sources.contains_key(&new_canister));
            assert!(st.commitment_history.contains_key(&new_canister));
            assert!(st.distinct_canisters.contains(&new_canister));
            assert!(st.distinct_canisters.contains(&existing));
        });
    }


}
