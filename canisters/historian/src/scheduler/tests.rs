use super::*;
#[cfg(test)]
// Scheduler tests use explicit copied principals/accounts to keep fixture setup readable.
#[allow(clippy::clone_on_copy, clippy::module_inception)]
mod tests {
    use super::*;
    use crate::clients::index::{
        GetAccountIdentifierTransactionsResponse, IndexOperation, IndexTimeStamp, IndexTransaction,
        IndexTransactionWithId, Tokens,
    };
    use crate::state::{ActiveCyclesSweep, Config, State};
    use async_trait::async_trait;
    use candid::Principal;
    use futures::channel::oneshot;
    use futures::executor::block_on;
    use futures::FutureExt;
    use icrc_ledger_types::icrc1::account::Account;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    fn principal(text: &str) -> candid::Principal {
        candid::Principal::from_text(text).unwrap()
    }

    fn sample_account() -> Account {
        Account {
            owner: principal("aaaaa-aa"),
            subaccount: None,
        }
    }

    fn configure_state(max_index_pages_per_tick: u32) -> String {
        state::clear_commitment_route_rollups();
        let account = sample_account();
        let staking_id = account_identifier_text_for_account(&account);
        state::set_state(State::new(
            Config {
                staking_account: account,
                output_source_account: Account {
                    owner: principal("uccpi-cqaaa-aaaar-qby3q-cai"),
                    subaccount: None,
                },
                output_account: Account {
                    owner: principal("acjuz-liaaa-aaaar-qb4qq-cai"),
                    subaccount: None,
                },
                rewards_account: Account {
                    owner: principal("alk7f-5aaaa-aaaar-qb4ra-cai"),
                    subaccount: None,
                },
                ledger_canister_id: principal("ryjl3-tyaaa-aaaaa-aaaba-cai"),
                index_canister_id: principal("qhbym-qaaaa-aaaaa-aaafq-cai"),
                cmc_canister_id: Some(principal("rkp4c-7iaaa-aaaaa-aaaca-cai")),
                faucet_canister_id: Some(principal("acjuz-liaaa-aaaar-qb4qq-cai")),
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
                relay_factory_enabled: false,
                relay_setup_min_e8s: 300_000_000,
                relay_initial_cycles: 2_000_000_000_000,
                relay_cycle_safety_margin_e8s: 5_000_000,
                relay_min_subaccount_one_seed_e8s: 100_020_000,
                self_service_relay_interval_seconds: 86400,
                canonical_relay_canister_id: Some(crate::mainnet_relay_id()),
                canonical_relay_targets: crate::mainnet_canonical_relay_targets(),
            },
            0,
        ));
        staking_id
    }

    #[test]
    fn main_tick_repairs_memo_registered_summary_index_drift() {
        configure_state(10);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(canister);
            st.canister_tracking_reasons.insert(
                canister,
                std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
            );
            st.commitment_history.insert(
                canister,
                vec![crate::state::CommitmentSample {
                    tx_id: 1,
                    timestamp_nanos: Some(1_000_000_000),
                    amount_e8s: 150,
                    counts_toward_faucet: true,
                }],
            );
            st.memo_registered_canister_summaries_cache = None;
            st.memo_registered_canister_summaries_total_desc_index = Some(vec![canister]);
        });

        let index = MockIndexClient::new(Vec::new());
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0);
        let sns_wasm = MockSnsWasmClient::new(Vec::new());
        let sns_root = MockSnsRootClient::new(BTreeMap::new());
        let governance = RecordingGovernanceClient::new();
        let xrc = MockXrcClient::success(720_000_000, 8, 9_900);

        block_on(run_main_tick_with_clients(
            100_000_000_000,
            100,
            &index,
            &cycles_probe,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
            &|| 100,
        ))
        .unwrap();

        state::with_state(|st| {
            assert!(crate::memo_registered_canister_summary_index_is_valid(st));
            let page = crate::memo_registered_canister_summaries_total_desc_page(st, 0, 10)
                .expect("main-tick maintenance should repair the indexed summary page");
            assert_eq!(page.total, 1);
            assert_eq!(page.items[0].canister_id, canister);
            assert_eq!(st.commitment_history[&canister][0].amount_e8s, 150);
        });
    }

    fn transfer_to_staking_memo_tx(
        id: u64,
        staking_id: &str,
        memo: Vec<u8>,
        amount_e8s: u64,
        timestamp_nanos: u64,
    ) -> IndexTransactionWithId {
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

    fn transfer_to_staking_tx(
        id: u64,
        staking_id: &str,
        beneficiary: candid::Principal,
        amount_e8s: u64,
        timestamp_nanos: u64,
    ) -> IndexTransactionWithId {
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

    fn transfer_between_accounts_tx(
        id: u64,
        from: &str,
        to: &str,
        amount_e8s: u64,
        timestamp_nanos: u64,
    ) -> IndexTransactionWithId {
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

    fn transfer_from_between_accounts_tx(
        id: u64,
        from: &str,
        to: &str,
        amount_e8s: u64,
        timestamp_nanos: u64,
    ) -> IndexTransactionWithId {
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
        responses: Mutex<
            VecDeque<Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError>>,
        >,
        calls: Mutex<Vec<(String, Option<u64>, u64)>>,
    }

    struct DelayedIndexClient {
        response: Mutex<
            Option<
                oneshot::Receiver<
                    Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError>,
                >,
            >,
        >,
        calls: Mutex<u32>,
    }

    struct LeaseClockIndexClient {
        pages: Mutex<VecDeque<GetAccountIdentifierTransactionsResponse>>,
        clock: Arc<AtomicU64>,
        attempt_refresh_on_call: u32,
        first_response_clock: u64,
        refresh_attempt_clock: u64,
        calls: Mutex<u32>,
        refresh_acquired: AtomicBool,
    }

    impl LeaseClockIndexClient {
        fn new(
            pages: Vec<GetAccountIdentifierTransactionsResponse>,
            clock: Arc<AtomicU64>,
            attempt_refresh_on_call: u32,
            first_response_clock: u64,
            refresh_attempt_clock: u64,
        ) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                clock,
                attempt_refresh_on_call,
                first_response_clock,
                refresh_attempt_clock,
                calls: Mutex::new(0),
                refresh_acquired: AtomicBool::new(false),
            }
        }

        fn now(&self) -> u64 {
            self.clock.load(Ordering::SeqCst)
        }

        fn calls(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl IndexClient for LeaseClockIndexClient {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            let call = {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                *calls
            };
            if call == 1 {
                self.clock
                    .store(self.first_response_clock, Ordering::SeqCst);
            }
            if call == self.attempt_refresh_on_call {
                self.clock
                    .store(self.refresh_attempt_clock, Ordering::SeqCst);
                let acquired = CommitmentIndexGuard::acquire(
                    self.now(),
                    state::CommitmentIndexLeaseOwner::EndowmentRefresh,
                );
                self.refresh_acquired
                    .store(acquired.is_some(), Ordering::SeqCst);
                drop(acquired);
            }
            Ok(self
                .pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| index_page(Vec::new())))
        }
    }

    struct ClockAdvancingXrc<'a> {
        clock: &'a AtomicU64,
    }

    #[async_trait]
    impl ExchangeRateClient for ClockAdvancingXrc<'_> {
        async fn get_icp_xdr_rate(
            &self,
        ) -> Result<crate::clients::IcpXdrRate, crate::clients::ClientError> {
            self.clock.store(190, Ordering::SeqCst);
            Ok(crate::clients::IcpXdrRate {
                rate: 720_000_000,
                decimals: 8,
                timestamp: 100,
            })
        }
    }

    impl DelayedIndexClient {
        fn new() -> (
            Self,
            oneshot::Sender<
                Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError>,
            >,
        ) {
            let (sender, receiver) = oneshot::channel();
            (
                Self {
                    response: Mutex::new(Some(receiver)),
                    calls: Mutex::new(0),
                },
                sender,
            )
        }
    }

    impl MockIndexClient {
        fn new(pages: Vec<GetAccountIdentifierTransactionsResponse>) -> Self {
            Self {
                responses: Mutex::new(pages.into_iter().map(Ok).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn scripted(
            responses: Vec<
                Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError>,
            >,
        ) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, Option<u64>, u64)> {
            self.calls.lock().unwrap().clone()
        }
    }

    fn index_page(
        transactions: Vec<IndexTransactionWithId>,
    ) -> GetAccountIdentifierTransactionsResponse {
        GetAccountIdentifierTransactionsResponse {
            balance: 0,
            oldest_tx_id: transactions.iter().map(|tx| tx.id).min(),
            transactions,
        }
    }

    fn route_rollup(route: crate::CommitmentRoute) -> state::CommitmentRouteRollup {
        let key = state::CommitmentRouteKey::from_public(&route).expect("valid route");
        state::get_commitment_route_rollup(&key)
    }

    fn paged_commitment_ids(canister_id: Principal, descending: bool, raw_icp: bool) -> Vec<u64> {
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let args = crate::GetCommitmentHistoryArgs {
                canister_id,
                start_after_tx_id: cursor,
                limit: Some(1),
                descending: Some(descending),
            };
            let page = if raw_icp {
                crate::read_model::get_raw_icp_commitment_history(args)
            } else {
                crate::read_model::get_commitment_history(args)
            };
            ids.extend(page.items.iter().map(|item| item.tx_id));
            let Some(next) = page.next_start_after_tx_id else {
                break;
            };
            cursor = Some(next);
        }
        ids
    }

    fn paged_neuron_commitment_ids(neuron_id: u64, descending: bool) -> Vec<u64> {
        let mut cursor = None;
        let mut ids = Vec::new();
        loop {
            let page = crate::read_model::get_neuron_commitment_history(
                crate::GetNeuronCommitmentHistoryArgs {
                    neuron_id,
                    start_after_tx_id: cursor,
                    limit: Some(1),
                    descending: Some(descending),
                },
            );
            ids.extend(page.items.iter().map(|item| item.tx_id));
            let Some(next) = page.next_start_after_tx_id else {
                break;
            };
            cursor = Some(next);
        }
        ids
    }

    #[async_trait]
    impl IndexClient for MockIndexClient {
        async fn get_account_identifier_transactions(
            &self,
            account_identifier: String,
            start: Option<u64>,
            max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            self.calls
                .lock()
                .unwrap()
                .push((account_identifier, start, max_results));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(GetAccountIdentifierTransactionsResponse {
                        balance: 0,
                        transactions: Vec::new(),
                        oldest_tx_id: None,
                    })
                })
        }
    }

    #[async_trait]
    impl IndexClient for DelayedIndexClient {
        async fn get_account_identifier_transactions(
            &self,
            _account_identifier: String,
            _start: Option<u64>,
            _max_results: u64,
        ) -> Result<GetAccountIdentifierTransactionsResponse, crate::clients::ClientError> {
            *self.calls.lock().unwrap() += 1;
            let receiver = {
                self.response
                    .lock()
                    .unwrap()
                    .take()
                    .expect("one delayed response")
            };
            receiver.await.expect("delayed sender must resolve")
        }
    }

    struct MockSnsWasmClient {
        responses: Mutex<
            VecDeque<
                Result<
                    crate::clients::sns_wasm::ListDeployedSnsesResponse,
                    crate::clients::ClientError,
                >,
            >,
        >,
        calls: Mutex<u32>,
    }

    impl MockSnsWasmClient {
        fn new(
            responses: Vec<
                Result<
                    crate::clients::sns_wasm::ListDeployedSnsesResponse,
                    crate::clients::ClientError,
                >,
            >,
        ) -> Self {
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
        async fn list_deployed_snses(
            &self,
        ) -> Result<crate::clients::sns_wasm::ListDeployedSnsesResponse, crate::clients::ClientError>
        {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(crate::clients::sns_wasm::ListDeployedSnsesResponse {
                        instances: Vec::new(),
                    })
                })
        }
    }

    struct MockSnsRootClient {
        responses: Mutex<BTreeMap<Principal, jupiter_ic_clients::sns::ListSnsCanistersResponse>>,
        calls: Mutex<Vec<Principal>>,
    }

    impl MockSnsRootClient {
        fn new(
            responses: BTreeMap<Principal, jupiter_ic_clients::sns::ListSnsCanistersResponse>,
        ) -> Self {
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
        async fn list_sns_canisters(
            &self,
            root_id: Principal,
        ) -> Result<jupiter_ic_clients::sns::ListSnsCanistersResponse, crate::clients::ClientError>
        {
            self.calls.lock().unwrap().push(root_id);
            self.responses
                .lock()
                .unwrap()
                .get(&root_id)
                .cloned()
                .ok_or_else(|| {
                    crate::clients::ClientError::Call(format!("missing membership for {}", root_id))
                })
        }
    }

    #[derive(Clone)]
    enum ProbeResponse {
        Ok(u128),
        Err(String),
    }

    struct RecordingCyclesProbeClient {
        self_cycles: Mutex<BTreeMap<Principal, u128>>,
        default_blackhole_response: Mutex<ProbeResponse>,
        blackhole_responses_by_target: Mutex<BTreeMap<Principal, ProbeResponse>>,
        root_responses: Mutex<BTreeMap<Principal, ProbeResponse>>,
        swap_responses: Mutex<BTreeMap<Principal, ProbeResponse>>,
        blackhole_calls: Mutex<Vec<(Principal, Principal)>>,
        root_calls: Mutex<Vec<(Principal, Principal)>>,
        swap_calls: Mutex<Vec<Principal>>,
    }

    impl RecordingCyclesProbeClient {
        fn blackhole(cycles: u128) -> Self {
            Self {
                self_cycles: Mutex::new(BTreeMap::new()),
                default_blackhole_response: Mutex::new(ProbeResponse::Ok(cycles)),
                blackhole_responses_by_target: Mutex::new(BTreeMap::new()),
                root_responses: Mutex::new(BTreeMap::new()),
                swap_responses: Mutex::new(BTreeMap::new()),
                blackhole_calls: Mutex::new(Vec::new()),
                root_calls: Mutex::new(Vec::new()),
                swap_calls: Mutex::new(Vec::new()),
            }
        }

        fn failing_blackhole(message: &str) -> Self {
            let client = Self::blackhole(0);
            *client.default_blackhole_response.lock().unwrap() =
                ProbeResponse::Err(message.to_string());
            client
        }

        fn with_self_cycles(self, target: Principal, cycles: u128) -> Self {
            self.self_cycles.lock().unwrap().insert(target, cycles);
            self
        }

        fn with_blackhole_target_response(
            self,
            target: Principal,
            response: ProbeResponse,
        ) -> Self {
            self.blackhole_responses_by_target
                .lock()
                .unwrap()
                .insert(target, response);
            self
        }

        fn with_root_response(self, root: Principal, response: ProbeResponse) -> Self {
            self.root_responses.lock().unwrap().insert(root, response);
            self
        }

        fn with_swap_response(self, swap: Principal, response: ProbeResponse) -> Self {
            self.swap_responses.lock().unwrap().insert(swap, response);
            self
        }

        fn blackhole_targets(&self) -> Vec<Principal> {
            self.blackhole_calls
                .lock()
                .unwrap()
                .iter()
                .map(|(_, target)| *target)
                .collect()
        }

        fn root_calls(&self) -> Vec<(Principal, Principal)> {
            self.root_calls.lock().unwrap().clone()
        }

        fn swap_calls(&self) -> Vec<Principal> {
            self.swap_calls.lock().unwrap().clone()
        }
    }

    impl CyclesProbeClient for RecordingCyclesProbeClient {
        async fn self_cycles(&self, target: Principal) -> Option<u128> {
            self.self_cycles.lock().unwrap().get(&target).copied()
        }

        async fn direct_canister_status(
            &self,
            _target: Principal,
        ) -> Result<
            jupiter_ic_clients::cycles_probe::DirectCanisterStatusObservation,
            jupiter_ic_clients::ClientError,
        > {
            Err(jupiter_ic_clients::ClientError::Call(
                "direct canister_status unavailable in scheduler mock".to_string(),
            ))
        }

        async fn blackhole_cycles(
            &self,
            probe_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.blackhole_calls
                .lock()
                .unwrap()
                .push((probe_canister_id, target_canister_id));
            let response = self
                .blackhole_responses_by_target
                .lock()
                .unwrap()
                .get(&target_canister_id)
                .cloned()
                .unwrap_or_else(|| self.default_blackhole_response.lock().unwrap().clone());
            match response {
                ProbeResponse::Ok(cycles) => Ok(cycles),
                ProbeResponse::Err(message) => Err(jupiter_ic_clients::ClientError::Call(message)),
            }
        }

        async fn list_deployed_snses(
            &self,
        ) -> Result<
            jupiter_ic_clients::sns::ListDeployedSnsesResponse,
            jupiter_ic_clients::ClientError,
        > {
            Ok(jupiter_ic_clients::sns::ListDeployedSnsesResponse::default())
        }

        async fn canister_info_controllers(
            &self,
            _target: Principal,
        ) -> Result<Vec<Principal>, jupiter_ic_clients::ClientError> {
            Ok(Vec::new())
        }

        async fn list_sns_canisters(
            &self,
            _root_canister_id: Principal,
        ) -> Result<
            jupiter_ic_clients::sns::ListSnsCanistersResponse,
            jupiter_ic_clients::ClientError,
        > {
            Ok(jupiter_ic_clients::sns::ListSnsCanistersResponse::default())
        }

        async fn sns_root_cycles(
            &self,
            root_canister_id: Principal,
            target_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.root_calls
                .lock()
                .unwrap()
                .push((root_canister_id, target_canister_id));
            match self
                .root_responses
                .lock()
                .unwrap()
                .get(&root_canister_id)
                .cloned()
                .unwrap_or_else(|| ProbeResponse::Err("missing root response".to_string()))
            {
                ProbeResponse::Ok(cycles) => Ok(cycles),
                ProbeResponse::Err(message) => Err(jupiter_ic_clients::ClientError::Call(message)),
            }
        }

        async fn sns_swap_cycles(
            &self,
            swap_canister_id: Principal,
        ) -> Result<u128, jupiter_ic_clients::ClientError> {
            self.swap_calls.lock().unwrap().push(swap_canister_id);
            match self
                .swap_responses
                .lock()
                .unwrap()
                .get(&swap_canister_id)
                .cloned()
                .unwrap_or_else(|| ProbeResponse::Err("missing swap response".to_string()))
            {
                ProbeResponse::Ok(cycles) => Ok(cycles),
                ProbeResponse::Err(message) => Err(jupiter_ic_clients::ClientError::Call(message)),
            }
        }
    }

    struct RecordingGovernanceClient {
        calls: Mutex<Vec<[u8; 32]>>,
    }

    impl RecordingGovernanceClient {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<[u8; 32]> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl GovernanceClient for RecordingGovernanceClient {
        async fn claim_or_refresh_neuron_by_subaccount(
            &self,
            subaccount: [u8; 32],
        ) -> Result<(), crate::clients::ClientError> {
            self.calls.lock().unwrap().push(subaccount);
            Ok(())
        }
    }

    struct MockXrcClient {
        responses: Mutex<VecDeque<Result<crate::clients::IcpXdrRate, crate::clients::ClientError>>>,
        calls: Mutex<u32>,
    }

    impl MockXrcClient {
        fn new(
            responses: Vec<Result<crate::clients::IcpXdrRate, crate::clients::ClientError>>,
        ) -> Self {
            Self {
                responses: Mutex::new(VecDeque::from(responses)),
                calls: Mutex::new(0),
            }
        }

        fn success(rate: u64, decimals: u32, timestamp: u64) -> Self {
            Self::new(vec![Ok(crate::clients::IcpXdrRate {
                rate,
                decimals,
                timestamp,
            })])
        }

        fn calls(&self) -> u32 {
            *self.calls.lock().unwrap()
        }
    }

    #[async_trait]
    impl ExchangeRateClient for MockXrcClient {
        async fn get_icp_xdr_rate(
            &self,
        ) -> Result<crate::clients::IcpXdrRate, crate::clients::ClientError> {
            *self.calls.lock().unwrap() += 1;
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Err(crate::clients::ClientError::Call(
                        "missing XRC mock response".into(),
                    ))
                })
        }
    }

    #[test]
    fn icp_xdr_rate_refresh_caches_success_for_one_day() {
        configure_state(10);
        let xrc = MockXrcClient::new(vec![
            Ok(crate::clients::IcpXdrRate {
                rate: 720_000_000,
                decimals: 8,
                timestamp: 1_000,
            }),
            Ok(crate::clients::IcpXdrRate {
                rate: 735_000_000,
                decimals: 8,
                timestamp: 2_000,
            }),
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

        block_on(refresh_icp_xdr_rate_if_due(
            10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS - 1,
            &xrc,
        ))
        .unwrap();
        assert_eq!(
            xrc.calls(),
            1,
            "fresh daily cache should suppress another XRC call"
        );

        block_on(refresh_icp_xdr_rate_if_due(
            10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS,
            &xrc,
        ))
        .unwrap();
        state::with_state(|st| {
            let snapshot = st.icp_xdr_rate.as_ref().expect("rate should be refreshed");
            assert_eq!(snapshot.rate, 735_000_000);
            assert_eq!(snapshot.timestamp, 2_000);
            assert_eq!(
                snapshot.fetched_at_ts,
                10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS
            );
        });
        assert_eq!(xrc.calls(), 2);
    }

    #[test]
    fn icp_xdr_rate_refresh_records_errors_without_clearing_last_good_rate() {
        configure_state(10);
        let xrc = MockXrcClient::new(vec![
            Ok(crate::clients::IcpXdrRate {
                rate: 720_000_000,
                decimals: 8,
                timestamp: 1_000,
            }),
            Err(crate::clients::ClientError::Call("NotEnoughCycles".into())),
        ]);

        block_on(refresh_icp_xdr_rate_if_due(10_000, &xrc)).unwrap();
        let err = block_on(refresh_icp_xdr_rate_if_due(
            10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS,
            &xrc,
        ))
        .unwrap_err();
        assert!(err.contains("NotEnoughCycles"));
        state::with_state(|st| {
            let snapshot = st
                .icp_xdr_rate
                .as_ref()
                .expect("last good rate should remain cached");
            assert_eq!(snapshot.rate, 720_000_000);
            assert_eq!(
                st.last_icp_xdr_rate_error.as_deref(),
                Some("inter-canister call failed: NotEnoughCycles")
            );
        });
        assert_eq!(xrc.calls(), 2);

        block_on(refresh_icp_xdr_rate_if_due(
            10_000 + ICP_XDR_RATE_CACHE_TTL_SECONDS + 1,
            &xrc,
        ))
        .unwrap();
        assert_eq!(
            xrc.calls(),
            2,
            "failed XRC refresh should be throttled for one day to prevent a cycle drain"
        );

        block_on(refresh_icp_xdr_rate_if_due(
            10_000 + (2 * ICP_XDR_RATE_CACHE_TTL_SECONDS),
            &xrc,
        ))
        .unwrap_err();
        assert_eq!(
            xrc.calls(),
            3,
            "retry should only happen after failed-attempt TTL expires"
        );
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
        let sns_wasm = MockSnsWasmClient::new(vec![Ok(
            crate::clients::sns_wasm::ListDeployedSnsesResponse {
                instances: vec![
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_b.clone()),
                    },
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_a.clone()),
                    },
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_b.clone()),
                    },
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_c.clone()),
                    },
                ],
            },
        )]);
        let mut summaries = BTreeMap::new();
        summaries.insert(
            root_a.clone(),
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root_a.clone()),
                ..Default::default()
            },
        );
        summaries.insert(
            root_b.clone(),
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root_b.clone()),
                ..Default::default()
            },
        );
        summaries.insert(
            root_c.clone(),
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root_c.clone()),
                ..Default::default()
            },
        );
        let sns_root = MockSnsRootClient::new(summaries);

        block_on(process_sns_discovery(123, 100, &sns_wasm, &sns_root)).unwrap();
        state::with_state(|st| {
            let active = st
                .active_sns_discovery
                .as_ref()
                .expect("discovery should remain in progress after first batch");
            assert_eq!(
                active.root_canister_ids,
                vec![root_a.clone(), root_b.clone(), root_c.clone()]
            );
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
            assert!(!st.cycles_history.contains_key(&root_c));
            assert_eq!(
                st.per_canister_meta
                    .get(&root_c)
                    .and_then(|meta| meta.last_cycles_probe_result.as_ref()),
                None
            );
        });
        assert_eq!(
            sns_wasm.calls(),
            1,
            "deployed SNS roots should be fetched only once per discovery sweep"
        );
        assert_eq!(
            sns_root.calls(),
            vec![root_a.clone(), root_b.clone(), root_c.clone()]
        );
    }

    #[test]
    fn sns_membership_queue_and_frozen_sweep_do_not_double_probe_across_ticks() {
        configure_state(10);
        let root = candid::Principal::from_slice(&[1]);
        let governance_id = candid::Principal::from_slice(&[2]);
        let ledger_id = candid::Principal::from_slice(&[3]);
        let swap_id = candid::Principal::from_slice(&[4]);
        let index_id = candid::Principal::from_slice(&[5]);
        let dapp_id = candid::Principal::from_slice(&[6]);
        let expected_members = [root, governance_id, ledger_id, swap_id, index_id, dapp_id];
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10;
            st.config.max_canisters_per_cycles_tick = 2;
            st.last_sns_discovery_ts = 0;
            st.last_completed_cycles_sweep_ts = 0;
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 99_000_000_000,
                canisters: expected_members.to_vec(),
                next_index: 0,
            });
        });
        let sns_wasm = MockSnsWasmClient::new(vec![Ok(
            crate::clients::sns_wasm::ListDeployedSnsesResponse {
                instances: vec![crate::clients::sns_wasm::DeployedSns {
                    root_canister_id: Some(root),
                }],
            },
        )]);
        let sns_root = MockSnsRootClient::new(BTreeMap::from([(
            root,
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root),
                governance: Some(governance_id),
                ledger: Some(ledger_id),
                swap: Some(swap_id),
                index: Some(index_id),
                dapps: vec![dapp_id],
                ..Default::default()
            },
        )]));
        let index = MockIndexClient::new(Vec::new());
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();
        let xrc = MockXrcClient::success(720_000_000, 8, 9_900);

        for now_secs in 100..=102 {
            block_on(run_main_tick_with_clients(
                now_secs * 1_000_000_000,
                now_secs,
                &index,
                &cycles_probe,
                &sns_wasm,
                &sns_root,
                &governance,
                &xrc,
                &|| now_secs,
            ))
            .unwrap();
        }

        let calls = cycles_probe.blackhole_targets();
        state::with_state(|st| {
            assert!(st.initial_cycles_probe_queue.is_empty());
            assert!(st.active_cycles_sweep.is_none());
            for member in expected_members {
                assert_eq!(
                    calls.iter().filter(|called| **called == member).count(),
                    1,
                    "SNS member {} should be probed once",
                    member.to_text()
                );
                assert_eq!(
                    st.cycles_history.get(&member).map(Vec::len),
                    Some(1),
                    "SNS member {} should have one history sample",
                    member.to_text()
                );
            }
        });
    }

    #[test]
    fn sns_discovery_skips_failing_root_membership_and_continues_batch() {
        let _staking_id = configure_state(10);
        let root_a = candid::Principal::from_slice(&[1]);
        let root_b = candid::Principal::from_slice(&[2]);
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.max_canisters_per_cycles_tick = 2;
            st.last_sns_discovery_ts = 0;
        });
        let sns_wasm = MockSnsWasmClient::new(vec![Ok(
            crate::clients::sns_wasm::ListDeployedSnsesResponse {
                instances: vec![
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_a),
                    },
                    crate::clients::sns_wasm::DeployedSns {
                        root_canister_id: Some(root_b),
                    },
                ],
            },
        )]);
        let mut summaries = BTreeMap::new();
        summaries.insert(
            root_b,
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root_b),
                ..Default::default()
            },
        );
        let sns_root = MockSnsRootClient::new(summaries);

        block_on(process_sns_discovery(123, 100, &sns_wasm, &sns_root)).unwrap();

        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 100);
            assert!(st.distinct_canisters.contains(&root_a));
            assert!(st.distinct_canisters.contains(&root_b));
        });
        assert_eq!(sns_root.calls(), vec![root_a, root_b]);
    }

    #[test]
    fn global_sns_discovery_failure_does_not_block_cycle_processing() {
        let _staking_id = configure_state(10);
        let initial_target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let sweep_target = principal("acjuz-liaaa-aaaar-qb4qq-cai");
        let staking_subaccount = [9u8; 32];
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.config.cycles_interval_seconds = 10;
            st.config.max_canisters_per_cycles_tick = 10;
            st.config.staking_account.subaccount = Some(staking_subaccount);
            st.last_sns_discovery_ts = 0;
            st.last_completed_cycles_sweep_ts = 0;
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 123_000_000_000,
                canisters: vec![sweep_target],
                next_index: 0,
            });
            st.distinct_canisters.insert(initial_target);
            st.distinct_canisters.insert(sweep_target);
            st.canister_tracking_reasons.insert(
                initial_target,
                std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
            );
            st.canister_tracking_reasons.insert(
                sweep_target,
                std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
            );
            st.commitment_history.insert(
                initial_target,
                vec![crate::state::CommitmentSample {
                    tx_id: 10,
                    timestamp_nanos: Some(10),
                    amount_e8s: 150,
                    counts_toward_faucet: true,
                }],
            );
            st.commitment_history.insert(
                sweep_target,
                vec![crate::state::CommitmentSample {
                    tx_id: 11,
                    timestamp_nanos: Some(11),
                    amount_e8s: 150,
                    counts_toward_faucet: true,
                }],
            );
            st.initial_cycles_probe_queue.push(initial_target);
        });
        let index = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: Vec::new(),
            oldest_tx_id: None,
        }]);
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0)
            .with_blackhole_target_response(initial_target, ProbeResponse::Ok(111))
            .with_blackhole_target_response(sweep_target, ProbeResponse::Ok(222));
        let sns_wasm = MockSnsWasmClient::new(vec![Err(crate::clients::ClientError::Call(
            "SNS-W unavailable".into(),
        ))]);
        let sns_root = MockSnsRootClient::new(BTreeMap::new());
        let governance = RecordingGovernanceClient::new();
        let xrc = MockXrcClient::success(720_000_000, 8, 9_900);

        block_on(run_main_tick_with_clients(
            123_000_000_000,
            123,
            &index,
            &cycles_probe,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
            &|| 123,
        ))
        .unwrap();

        state::with_state(|st| {
            assert_eq!(
                st.cycles_history
                    .get(&initial_target)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(111)
            );
            assert_eq!(
                st.cycles_history
                    .get(&sweep_target)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(222)
            );
        });
    }

    #[test]
    fn unsupported_route_pagination_does_not_block_unrelated_scheduled_maintenance() {
        for (route_index, route_name) in [(0_u64, "output"), (1_u64, "rewards")] {
            let _staking_id = configure_state(10);
            let initial_target = principal("jufzc-caaaa-aaaar-qb5da-cai");
            let sweep_target = principal("acjuz-liaaa-aaaar-qb4qq-cai");
            state::with_state_mut(|st| {
                st.config.enable_sns_tracking = true;
                st.config.cycles_interval_seconds = 10;
                st.config.max_canisters_per_cycles_tick = 10;
                st.last_sns_discovery_ts = 0;
                st.last_completed_cycles_sweep_ts = 0;
                st.commitment_index_lock_expires_at_ts = Some(999);
                st.commitment_index_lock_owner =
                    Some(crate::state::CommitmentIndexLeaseOwner::Scheduled);
                st.active_route_sweep = Some(ActiveRouteSweep {
                    started_at_ts_nanos: 123_000_000_000,
                    next_index: route_index,
                });
                st.output_route_index_descending = Some(route_index != 0);
                st.rewards_route_index_descending = Some(route_index != 1);
                st.last_indexed_output_tx_id = Some(41);
                st.last_indexed_rewards_tx_id = Some(42);
                st.total_output_e8s = Some(410);
                st.total_rewards_e8s = Some(420);
                st.active_cycles_sweep = Some(ActiveCyclesSweep {
                    started_at_ts_nanos: 122_000_000_000,
                    canisters: vec![sweep_target],
                    next_index: 0,
                });
                st.distinct_canisters.insert(initial_target);
                st.distinct_canisters.insert(sweep_target);
                st.canister_tracking_reasons.insert(
                    initial_target,
                    std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
                );
                st.canister_tracking_reasons.insert(
                    sweep_target,
                    std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
                );
                st.commitment_history.insert(
                    initial_target,
                    vec![crate::state::CommitmentSample {
                        tx_id: 10,
                        timestamp_nanos: Some(10),
                        amount_e8s: 150,
                        counts_toward_faucet: true,
                    }],
                );
                st.commitment_history.insert(
                    sweep_target,
                    vec![crate::state::CommitmentSample {
                        tx_id: 11,
                        timestamp_nanos: Some(11),
                        amount_e8s: 150,
                        counts_toward_faucet: true,
                    }],
                );
                st.initial_cycles_probe_queue.push(initial_target);
            });

            let index = MockIndexClient::new(Vec::new());
            let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
            let sns_wasm = MockSnsWasmClient::new(vec![Ok(
                crate::clients::sns_wasm::ListDeployedSnsesResponse {
                    instances: Vec::new(),
                },
            )]);
            let sns_root = MockSnsRootClient::new(BTreeMap::new());
            let governance = RecordingGovernanceClient::new();
            let xrc = MockXrcClient::success(720_000_000, 8, 9_900);

            block_on(run_main_tick_with_clients(
                123_000_000_000,
                123,
                &index,
                &cycles_probe,
                &sns_wasm,
                &sns_root,
                &governance,
                &xrc,
                &|| 123,
            ))
            .unwrap_or_else(|err| panic!("{route_name} degradation aborted the tick: {err}"));

            assert!(
                index.calls().is_empty(),
                "invalid {route_name} route must not call Index"
            );
            assert_eq!(sns_wasm.calls(), 1, "due SNS discovery must remain live");
            assert_eq!(
                cycles_probe.blackhole_targets(),
                vec![initial_target, sweep_target],
                "initial and active-sweep cycles probes must remain live",
            );
            assert!(
                state::with_state(crate::read_model::route_index_fault)
                    .as_deref()
                    .is_some_and(|message| message.contains(route_name)),
                "the degraded route must be visible in public status",
            );
            state::with_state(|st| {
                assert_eq!(st.last_indexed_output_tx_id, Some(41));
                assert_eq!(st.last_indexed_rewards_tx_id, Some(42));
                assert_eq!(st.total_output_e8s, Some(410));
                assert_eq!(st.total_rewards_e8s, Some(420));
                assert_eq!(
                    st.active_route_sweep.as_ref().map(|sweep| sweep.next_index),
                    Some(route_index),
                    "the degraded route must remain selected for a later retry",
                );
                assert_eq!(
                    if route_index == 0 {
                        st.output_route_index_descending
                    } else {
                        st.rewards_route_index_descending
                    },
                    Some(false),
                    "unsupported state must remain observable",
                );
                assert!(st.initial_cycles_probe_queue.is_empty());
                assert!(st.active_cycles_sweep.is_none());
            });

            state::with_state_mut(|st| {
                st.active_cycles_sweep = Some(ActiveCyclesSweep {
                    started_at_ts_nanos: 132_000_000_000,
                    canisters: vec![initial_target, sweep_target],
                    next_index: 0,
                });
            });
            block_on(run_main_tick_with_clients(
                133_000_000_000,
                133,
                &index,
                &cycles_probe,
                &sns_wasm,
                &sns_root,
                &governance,
                &xrc,
                &|| 133,
            ))
            .unwrap_or_else(|err| {
                panic!("later tick remained blocked by {route_name} degradation: {err}")
            });
            assert!(index.calls().is_empty());
            assert_eq!(sns_wasm.calls(), 2, "later SNS discovery must remain live");
            assert_eq!(
                cycles_probe.blackhole_targets().len(),
                4,
                "the later due sweep must probe both still-tracked canisters",
            );
            state::with_state(|st| {
                assert_eq!(
                    st.active_route_sweep.as_ref().map(|sweep| sweep.next_index),
                    Some(route_index),
                );
                assert_eq!(st.last_completed_cycles_sweep_ts, 133);
            });
        }
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
        let index = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: Vec::new(),
            oldest_tx_id: None,
        }]);
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0);
        let sns_wasm = MockSnsWasmClient::new(vec![]);
        let mut summaries = BTreeMap::new();
        summaries.insert(
            root_b.clone(),
            jupiter_ic_clients::sns::ListSnsCanistersResponse {
                root: Some(root_b.clone()),
                ..Default::default()
            },
        );
        let sns_root = MockSnsRootClient::new(summaries);
        let governance = RecordingGovernanceClient::new();

        let xrc = MockXrcClient::success(720_000_000, 8, 9_900);
        block_on(run_main_tick_with_clients(
            999,
            10_000,
            &index,
            &cycles_probe,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
            &|| 10_000,
        ))
        .unwrap();
        state::with_state(|st| {
            assert!(st.active_sns_discovery.is_none());
            assert_eq!(st.last_sns_discovery_ts, 10_000);
            assert!(st.distinct_canisters.contains(&root_b));
        });
        assert_eq!(
            sns_wasm.calls(),
            0,
            "resumed discovery should not refetch deployed SNS roots"
        );
        assert_eq!(sns_root.calls(), vec![root_b.clone()]);
    }

    #[test]
    fn indexing_single_qualifying_commitment_updates_counts() {
        let staking_id = configure_state(10);
        let beneficiary = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 150,
            transactions: vec![transfer_to_staking_tx(
                42,
                &staking_id,
                beneficiary,
                150,
                123_000_000_000,
            )],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(42));
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(st.last_index_run_ts, Some(200));
            assert!(st
                .canister_tracking_reasons
                .get(&beneficiary)
                .unwrap()
                .contains(&CanisterTrackingReason::MemoCommitment));
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
            transactions: vec![transfer_to_staking_tx(
                42,
                &staking_id,
                beneficiary,
                150,
                123_000_000_000,
            )],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 20_000)).unwrap();

        state::with_state(|st| {
            let active = st
                .active_cycles_sweep
                .as_ref()
                .expect("active sweep should not be reset");
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
            st.canister_tracking_reasons.insert(
                beneficiary,
                std::iter::once(CanisterTrackingReason::MemoCommitment).collect(),
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
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            999_000_000_000,
            999,
            &cycles_probe,
            &governance,
        ))
        .unwrap();

        state::with_state(|st| {
            assert_eq!(cycles_probe.blackhole_targets(), vec![beneficiary]);
            assert_eq!(governance.calls(), vec![staking_subaccount], "targeted registration probe should refresh the staking neuron directly via NNS governance");
            assert!(st.initial_cycles_probe_queue.is_empty());
            assert_eq!(st.last_completed_cycles_sweep_ts, 10_000);
            assert!(
                st.active_cycles_sweep.is_some(),
                "targeted first probe should not disturb active full sweep"
            );
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
    fn initial_probe_satisfies_same_target_in_existing_frozen_sweep() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.distinct_canisters.insert(target);
            st.canister_tracking_reasons.insert(
                target,
                std::iter::once(CanisterTrackingReason::RelayTarget).collect(),
            );
            st.initial_cycles_probe_queue.push(target);
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 123_500_000_000,
                canisters: vec![target],
                next_index: 0,
            });
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            124_000_000_000,
            124,
            &cycles_probe,
            &governance,
        ))
        .unwrap();
        block_on(process_cycles_sweep(124_000_000_000, 124, &cycles_probe)).unwrap();

        state::with_state(|st| {
            assert_eq!(cycles_probe.blackhole_targets(), vec![target]);
            assert_eq!(st.cycles_history.get(&target).map(Vec::len), Some(1));
            assert!(st.initial_cycles_probe_queue.is_empty());
            assert!(st.active_cycles_sweep.is_none());
        });
    }

    #[test]
    fn initial_cycles_probe_for_relay_target_does_not_refresh_staking_neuron() {
        let _staking_id = configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.config.staking_account.subaccount = Some([7u8; 32]);
            st.distinct_canisters.insert(target);
            st.canister_tracking_reasons.insert(
                target,
                std::iter::once(CanisterTrackingReason::RelayTarget).collect(),
            );
            st.initial_cycles_probe_queue.push(target);
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            999_000_000_000,
            999,
            &cycles_probe,
            &governance,
        ))
        .unwrap();

        assert_eq!(cycles_probe.blackhole_targets(), vec![target]);
        assert!(governance.calls().is_empty());
    }

    #[test]
    fn initial_cycles_probe_for_relay_instance_does_not_refresh_staking_neuron() {
        let _staking_id = configure_state(10);
        let relay = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.config.staking_account.subaccount = Some([7u8; 32]);
            st.distinct_canisters.insert(relay);
            st.canister_tracking_reasons.insert(
                relay,
                std::iter::once(CanisterTrackingReason::RelayInstance).collect(),
            );
            st.initial_cycles_probe_queue.push(relay);
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            999_000_000_000,
            999,
            &cycles_probe,
            &governance,
        ))
        .unwrap();

        assert_eq!(cycles_probe.blackhole_targets(), vec![relay]);
        assert!(governance.calls().is_empty());
    }

    #[test]
    fn initial_cycles_probe_for_sns_discovery_does_not_refresh_staking_neuron() {
        let _staking_id = configure_state(10);
        let target = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.config.staking_account.subaccount = Some([7u8; 32]);
            st.distinct_canisters.insert(target);
            st.canister_tracking_reasons.insert(
                target,
                std::iter::once(CanisterTrackingReason::SnsDiscovery).collect(),
            );
            st.initial_cycles_probe_queue.push(target);
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            999_000_000_000,
            999,
            &cycles_probe,
            &governance,
        ))
        .unwrap();

        assert_eq!(cycles_probe.blackhole_targets(), vec![target]);
        assert!(governance.calls().is_empty());
    }

    #[test]
    fn failed_target_does_not_prevent_later_cycles_sweep_target() {
        configure_state(10);
        let target_a = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let target_b = principal("acjuz-liaaa-aaaar-qb4qq-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 2;
            st.active_cycles_sweep = Some(ActiveCyclesSweep {
                started_at_ts_nanos: 123_000_000_000,
                canisters: vec![target_a, target_b],
                next_index: 0,
            });
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0)
            .with_blackhole_target_response(target_a, ProbeResponse::Err("target a down".into()))
            .with_blackhole_target_response(target_b, ProbeResponse::Ok(222));

        block_on(process_cycles_sweep(999_000_000_000, 999, &cycles_probe)).unwrap();

        state::with_state(|st| {
            assert!(st.active_cycles_sweep.is_none());
            assert!(!st.cycles_history.contains_key(&target_a));
            assert_eq!(
                st.per_canister_meta
                    .get(&target_a)
                    .and_then(|meta| meta.last_cycles_probe_result.clone()),
                Some(CyclesProbeResult::Error(
                    "no cycles probe route could observe target; previous errors: direct route failed: inter-canister call failed: direct canister_status unavailable in scheduler mock; blackhole e3mmv-5qaaa-aaaah-aadma-cai failed: inter-canister call failed: target a down; blackhole 77deu-baaaa-aaaar-qb6za-cai failed: inter-canister call failed: target a down"
                        .into()
                ))
            );
            assert_eq!(
                st.cycles_history
                    .get(&target_b)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(222)
            );
        });
    }

    #[test]
    fn failed_cached_route_is_removed() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        state::with_state_mut(|st| {
            st.cached_cycles_probe_routes.insert(
                target,
                CyclesProbeRoute::SnsRoot {
                    root_canister_id: root,
                },
            );
        });
        let cycles_probe = RecordingCyclesProbeClient::failing_blackhole("not controller")
            .with_root_response(root, ProbeResponse::Err("stale root".into()));

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            target,
            100,
            &cycles_probe,
        ))
        .unwrap();

        state::with_state(|st| {
            assert!(!st.cached_cycles_probe_routes.contains_key(&target));
            assert_eq!(cycles_probe.root_calls(), vec![(root, target)]);
            assert!(matches!(
                st.per_canister_meta
                    .get(&target)
                    .and_then(|meta| meta.last_cycles_probe_result.as_ref()),
                Some(CyclesProbeResult::Error(message)) if message.contains("stale root")
            ));
        });
    }

    #[test]
    fn newly_successful_route_replaces_cache() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let stale_root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let blackhole = jupiter_ic_clients::constants::thirteen_node_blackhole_canister_id();
        state::with_state_mut(|st| {
            st.cached_cycles_probe_routes.insert(
                target,
                CyclesProbeRoute::SnsRoot {
                    root_canister_id: stale_root,
                },
            );
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(444)
            .with_root_response(stale_root, ProbeResponse::Err("stale root".into()));

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            target,
            100,
            &cycles_probe,
        ))
        .unwrap();

        state::with_state(|st| {
            assert_eq!(
                st.cached_cycles_probe_routes.get(&target),
                Some(&CyclesProbeRoute::Blackhole {
                    canister_id: blackhole
                })
            );
            assert_eq!(
                st.cycles_history
                    .get(&target)
                    .and_then(|history| history.last())
                    .map(|sample| sample.cycles),
                Some(444)
            );
        });
    }

    #[test]
    fn direct_self_balance_records_self_canister_source() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0).with_self_cycles(target, 777);

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            target,
            100,
            &cycles_probe,
        ))
        .unwrap();

        state::with_state(|st| {
            let sample = st
                .cycles_history
                .get(&target)
                .and_then(|history| history.last())
                .expect("self cycles sample");
            assert_eq!(sample.source, CyclesSampleSource::SelfCanister);
        });
    }

    #[test]
    fn sns_root_success_records_root_status_source() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        state::with_state_mut(|st| {
            st.cached_cycles_probe_routes.insert(
                target,
                CyclesProbeRoute::SnsRoot {
                    root_canister_id: root,
                },
            );
        });
        let cycles_probe = RecordingCyclesProbeClient::failing_blackhole("not controller")
            .with_root_response(root, ProbeResponse::Ok(888));

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            target,
            100,
            &cycles_probe,
        ))
        .unwrap();

        state::with_state(|st| {
            let sample = st
                .cycles_history
                .get(&target)
                .and_then(|history| history.last())
                .expect("SNS root sample");
            assert_eq!(sample.source, CyclesSampleSource::SnsRootStatus);
        });
    }

    #[test]
    fn sns_swap_success_records_swap_status_source() {
        configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let root = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        let swap = principal("acjuz-liaaa-aaaar-qb4qq-cai");
        state::with_state_mut(|st| {
            st.cached_cycles_probe_routes.insert(
                target,
                CyclesProbeRoute::SnsSwap {
                    root_canister_id: root,
                    swap_canister_id: swap,
                },
            );
        });
        let cycles_probe = RecordingCyclesProbeClient::failing_blackhole("not controller")
            .with_swap_response(swap, ProbeResponse::Ok(999));

        block_on(probe_and_record_cycles(
            123_000_000_000,
            123,
            target,
            100,
            &cycles_probe,
        ))
        .unwrap();

        state::with_state(|st| {
            let sample = st
                .cycles_history
                .get(&target)
                .and_then(|history| history.last())
                .expect("SNS swap sample");
            assert_eq!(sample.source, CyclesSampleSource::SnsSwapStatus);
            assert_eq!(cycles_probe.swap_calls(), vec![swap]);
        });
    }

    #[test]
    fn cycles_sweep_targets_memo_canisters_with_lazy_stable_commitment_history() {
        let _staking_id = configure_state(10);
        let self_id = principal("aaaaa-aa");
        let beneficiary = principal("uccpi-cqaaa-aaaar-qby3q-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(beneficiary);
            st.canister_tracking_reasons.insert(
                beneficiary,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::MemoCommitment),
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
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id, beneficiary]
            );
        });
    }

    #[test]
    fn cycles_sweep_includes_active_self_service_target_without_source() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let relay = principal("u2qkp-aqaaa-aaaar-qb7ea-cai");
        state::with_state_mut(|st| {
            crate::mark_active_relay_tracked(st, target, relay, Some(123));
        });

        state::with_state(|st| {
            assert!(st.canister_tracking_reasons[&target]
                .contains(&CanisterTrackingReason::RelayTarget));
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id, target, relay]
            );
        });
    }

    #[test]
    fn cycles_sweep_includes_sns_discovery_when_sns_tracking_enabled() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let sns_canister = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.distinct_canisters.insert(sns_canister);
            st.canister_tracking_reasons.insert(
                sns_canister,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::SnsDiscovery),
            );
        });

        state::with_state(|st| {
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id, sns_canister]
            );
        });
    }

    #[test]
    fn cycles_sweep_includes_sns_discovery_mixed_with_relay_target_when_sns_tracking_enabled() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = true;
            st.distinct_canisters.insert(target);
            let reasons =
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::SnsDiscovery);
            st.canister_tracking_reasons.insert(
                target,
                crate::logic::merge_tracking_reasons(
                    Some(&reasons),
                    CanisterTrackingReason::RelayTarget,
                ),
            );
        });

        state::with_state(|st| {
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id, target]
            );
        });
    }

    #[test]
    fn cycles_sweep_includes_retained_sns_discovery_when_sns_tracking_disabled() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let sns_canister = principal("qaa6y-5yaaa-aaaaa-aaafa-cai");
        state::with_state_mut(|st| {
            st.config.enable_sns_tracking = false;
            st.distinct_canisters.insert(sns_canister);
            st.canister_tracking_reasons.insert(
                sns_canister,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::SnsDiscovery),
            );
        });

        state::with_state(|st| {
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id, sns_canister]
            );
        });
    }

    #[test]
    fn cycles_sweep_excludes_same_tick_initial_probe_target() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.distinct_canisters.insert(target);
            st.canister_tracking_reasons.insert(
                target,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::RelayTarget),
            );
            st.per_canister_meta
                .entry(target)
                .or_default()
                .last_cycles_probe_ts = Some(123);
        });

        state::with_state(|st| {
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id]
            );
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 124),
                vec![self_id, target]
            );
        });
    }

    #[test]
    fn cycles_sweep_excludes_target_after_initial_probe_in_same_second() {
        configure_state(10);
        let self_id = principal("aaaaa-aa");
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.config.max_canisters_per_cycles_tick = 1;
            st.distinct_canisters.insert(target);
            st.canister_tracking_reasons.insert(
                target,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::RelayTarget),
            );
            st.initial_cycles_probe_queue.push(target);
        });
        let cycles_probe = RecordingCyclesProbeClient::blackhole(777);
        let governance = RecordingGovernanceClient::new();

        block_on(process_initial_cycles_probe_queue(
            123_456_789_000,
            123,
            &cycles_probe,
            &governance,
        ))
        .unwrap();

        state::with_state(|st| {
            assert_eq!(cycles_probe.blackhole_targets(), vec![target]);
            assert_eq!(
                st.per_canister_meta
                    .get(&target)
                    .and_then(|meta| meta.last_cycles_probe_ts),
                Some(123)
            );
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 123),
                vec![self_id]
            );
            assert_eq!(
                build_cycles_sweep_canisters(st, self_id, 124),
                vec![self_id, target]
            );
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
        let raw_tx = transfer_to_staking_memo_tx(
            42,
            &staking_id,
            raw_memo.into_bytes(),
            150,
            123_000_000_000,
        );
        let neuron_tx = transfer_to_staking_memo_tx(
            43,
            &staking_id,
            b"42.local.memo".to_vec(),
            160,
            124_000_000_000,
        );

        apply_indexed_commitment_tx(&raw_tx, &staking_id, 100, 200);
        apply_indexed_commitment_tx(&neuron_tx, &staking_id, 100, 200);
        apply_indexed_commitment_tx(&raw_tx, &staking_id, 100, 201);
        apply_indexed_commitment_tx(&neuron_tx, &staking_id, 100, 201);

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(st.recent_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(st.recent_neuron_commitments.as_ref().unwrap().len(), 1);
            assert_eq!(
                st.raw_icp_commitment_history
                    .get(&raw_canister)
                    .unwrap()
                    .len(),
                1
            );
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
            assert_eq!(
                st.raw_icp_commitment_history
                    .get(&raw_canister)
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(st.neuron_commitment_history.get(&42).unwrap().len(), 1);
        });

        let raw_key = state::CommitmentRouteKey::from_public(&crate::CommitmentRoute::RawIcp {
            destination_canister_id: raw_canister,
            memo: b"vault42".to_vec(),
        })
        .unwrap();
        let neuron_key =
            state::CommitmentRouteKey::from_public(&crate::CommitmentRoute::NeuronStake {
                neuron_id: 42,
                memo: Some(b"local.memo".to_vec()),
            })
            .unwrap();
        assert_eq!(
            state::get_commitment_route_rollup(&raw_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 150,
            }
        );
        assert_eq!(
            state::get_commitment_route_rollup(&neuron_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 160,
            }
        );
    }

    #[test]
    fn exact_route_rollups_aggregate_independently_of_retained_history_caps() {
        let staking_id = configure_state(10);
        state::with_state_mut(|st| st.config.max_commitment_entries_per_canister = 1);
        let canister_a = Principal::from_slice(&[41]);
        let canister_b = Principal::from_slice(&[42]);
        let route_memos = [
            canister_a.to_text().into_bytes(),
            format!("{}.", canister_a.to_text()).into_bytes(),
            format!("{}.target-1", canister_a.to_text()).into_bytes(),
            b"100".to_vec(),
            b"100.".to_vec(),
            b"100.target-1".to_vec(),
        ];
        let mut id = 1;
        for memo in route_memos {
            for amount in [101, 102, 103] {
                apply_indexed_commitment_tx(
                    &transfer_to_staking_memo_tx(id, &staking_id, memo.clone(), amount, id),
                    &staking_id,
                    100,
                    200,
                );
                id += 1;
            }
        }

        let routes = [
            crate::CommitmentRoute::CyclesTopUp {
                canister_id: canister_a,
            },
            crate::CommitmentRoute::RawIcp {
                destination_canister_id: canister_a,
                memo: Vec::new(),
            },
            crate::CommitmentRoute::RawIcp {
                destination_canister_id: canister_a,
                memo: b"target-1".to_vec(),
            },
            crate::CommitmentRoute::NeuronStake {
                neuron_id: 100,
                memo: None,
            },
            crate::CommitmentRoute::NeuronStake {
                neuron_id: 100,
                memo: Some(Vec::new()),
            },
            crate::CommitmentRoute::NeuronStake {
                neuron_id: 100,
                memo: Some(b"target-1".to_vec()),
            },
        ];
        for route in routes {
            let key = state::CommitmentRouteKey::from_public(&route).unwrap();
            assert_eq!(
                state::get_commitment_route_rollup(&key),
                state::CommitmentRouteRollup {
                    qualifying_commitment_count: 3,
                    total_qualifying_committed_e8s: 306,
                },
                "{route:?}"
            );
        }
        state::with_state(|st| {
            assert_eq!(st.commitment_history.get(&canister_a).unwrap().len(), 1);
            assert_eq!(
                st.raw_icp_commitment_history
                    .get(&canister_a)
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(st.neuron_commitment_history.get(&100).unwrap().len(), 1);
        });
        assert_eq!(state::commitment_route_rollup_entry_count(), 6);

        for (memo, amount) in [
            (format!("{}.", canister_b.to_text()).into_bytes(), 99),
            (b"101.".to_vec(), 99),
            (b"not-a-route".to_vec(), 500),
        ] {
            apply_indexed_commitment_tx(
                &transfer_to_staking_memo_tx(id, &staking_id, memo, amount, id),
                &staking_id,
                100,
                200,
            );
            id += 1;
        }
        let unrelated = transfer_to_staking_memo_tx(
            id,
            "another-account",
            format!("{}.", canister_b.to_text()).into_bytes(),
            500,
            id,
        );
        apply_indexed_commitment_tx(&unrelated, &staking_id, 100, 200);
        assert_eq!(state::commitment_route_rollup_entry_count(), 6);
    }

    #[test]
    fn indexing_uses_cursor_and_keeps_recent_commitments_descending() {
        let staking_id = configure_state(1);
        let first_canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let second_canister = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(
                    10,
                    &staking_id,
                    first_canister,
                    100,
                    100_000_000_000,
                )],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 300,
                transactions: vec![transfer_to_staking_tx(
                    11,
                    &staking_id,
                    second_canister,
                    200,
                    300_000_000_000,
                )],
                oldest_tx_id: Some(11),
            },
        ]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();
        block_on(process_commitment_indexing(&mock, 201)).unwrap();

        let calls = mock.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].1, None);
        assert_eq!(
            calls[1].1, None,
            "newest-first catch-up samples the current head before walking older pages"
        );

        state::with_state(|st| {
            let recent = st.recent_commitments.as_ref().unwrap();
            assert_eq!(recent.len(), 2);
            assert_eq!(recent[0].tx_id, 11);
            assert_eq!(recent[1].tx_id, 10);
            assert_eq!(st.last_indexed_staking_tx_id, Some(11));
        });
    }

    #[test]
    fn empty_and_single_initial_pages_persist_the_configured_newest_first_contract() {
        for page in [
            index_page(Vec::new()),
            index_page(vec![IndexTransactionWithId {
                id: 42,
                transaction: IndexTransaction {
                    memo: 0,
                    icrc1_memo: None,
                    operation: IndexOperation::Mint {
                        to: "irrelevant".to_string(),
                        amount: Tokens::new(1),
                    },
                    created_at_time: None,
                    timestamp: None,
                },
            }]),
        ] {
            configure_state(1);
            let index = MockIndexClient::new(vec![page]);
            block_on(process_commitment_indexing(&index, 200)).unwrap();
            assert_eq!(index.calls()[0].1, None);
            assert_eq!(
                state::with_state(|st| st.staking_index_descending),
                Some(true)
            );
        }
    }

    #[test]
    fn persisted_ascending_staking_state_fails_closed_without_an_index_call() {
        let _staking_id = configure_state(1);
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(42);
            st.oldest_indexed_staking_tx_id = Some(42);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(true);
            st.commitment_route_rollups_complete_from_genesis = Some(true);
        });
        let index = MockIndexClient::new(vec![index_page(Vec::new())]);
        let error = block_on(process_commitment_indexing(&index, 200)).unwrap_err();
        assert!(error.contains("unsupported persisted ascending"));
        assert!(index.calls().is_empty());
        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(42));
            assert_eq!(st.staking_index_descending, Some(false));
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(false)
            );
            assert!(st.commitment_index_fault.is_some());
        });
    }

    #[test]
    fn persisted_ascending_output_and_rewards_state_fail_closed_without_index_calls() {
        configure_state(1);
        let index = MockIndexClient::new(vec![index_page(Vec::new())]);
        state::with_state_mut(|st| {
            st.output_route_index_descending = Some(false);
            st.active_route_sweep = Some(ActiveRouteSweep {
                started_at_ts_nanos: 100,
                next_index: 0,
            });
        });
        let output_error = block_on(process_route_indexing(100, 200, &index)).unwrap_err();
        assert!(output_error.contains("unsupported persisted ascending output"));
        assert!(index.calls().is_empty());

        state::with_state_mut(|st| {
            st.output_route_index_descending = Some(true);
            st.rewards_route_index_descending = Some(false);
            st.active_route_sweep = Some(ActiveRouteSweep {
                started_at_ts_nanos: 100,
                next_index: 1,
            });
        });
        let rewards_error = block_on(process_route_indexing(100, 201, &index)).unwrap_err();
        assert!(rewards_error.contains("unsupported persisted ascending rewards"));
        assert!(index.calls().is_empty());
    }

    #[test]
    fn descending_commitment_catch_up_resumes_unfinished_interval_before_new_head_scan() {
        let staking_id = configure_state(1);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(10);
            st.oldest_indexed_staking_tx_id = Some(1);
            st.staking_index_descending = Some(true);
            st.staking_backfill_complete = Some(true);
            st.commitment_route_rollups_complete_from_genesis = Some(true);
        });

        let newest_page = index_page(
            (512..=1_011)
                .rev()
                .map(|id| {
                    transfer_to_staking_tx(id, &staking_id, canister, 100, id * 1_000_000_000)
                })
                .collect(),
        );
        block_on(process_commitment_indexing(
            &MockIndexClient::new(vec![newest_page]),
            200,
        ))
        .unwrap();
        state::with_state_mut(|st| st.config.max_index_pages_per_tick = 2);

        let older_page = index_page(
            (12..=511)
                .rev()
                .map(|id| {
                    transfer_to_staking_tx(id, &staking_id, canister, 100, id * 1_000_000_000)
                })
                .collect(),
        );
        let oldest_page = index_page(vec![transfer_to_staking_tx(
            11,
            &staking_id,
            canister,
            100,
            11_000_000_000,
        )]);
        let resumed = MockIndexClient::new(vec![older_page, oldest_page]);
        block_on(process_commitment_indexing(&resumed, 201)).unwrap();

        assert_eq!(resumed.calls()[0].1, Some(512));
        assert_eq!(
            route_rollup(crate::CommitmentRoute::CyclesTopUp {
                canister_id: canister,
            }),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1_001,
                total_qualifying_committed_e8s: 100_100,
            }
        );
        let ascending = paged_commitment_ids(canister, false, false);
        assert_eq!(ascending, (912..=1_011).collect::<Vec<_>>());
        assert_eq!(
            paged_commitment_ids(canister, true, false),
            ascending.iter().rev().copied().collect::<Vec<_>>()
        );
        state::with_state(|st| {
            assert_eq!(
                st.per_canister_meta
                    .get(&canister)
                    .and_then(|meta| meta.last_commitment_ts),
                Some(1_011),
            );
        });

        let restored = state::restore_state_from_stable().expect("catch-up state reloads");
        state::set_state_root_only(restored);
        assert_eq!(
            paged_commitment_ids(canister, false, false),
            (912..=1_011).collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_endowment_history_retains_newest_ids_for_all_configured_caps() {
        let cycles = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let raw = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let processing_order: Vec<u64> = (512..=1_011)
            .chain(12..=511)
            .chain(std::iter::once(11))
            .collect();

        for cap in [1, 2, 100] {
            let staking_id = configure_state(1);
            state::with_state_mut(|st| st.config.max_commitment_entries_per_canister = cap);
            let _batch = state::begin_persistence_batch();
            for tx_id in &processing_order {
                apply_indexed_commitment_tx(
                    &transfer_to_staking_tx(
                        *tx_id,
                        &staking_id,
                        cycles,
                        100,
                        tx_id * 1_000_000_000,
                    ),
                    &staking_id,
                    100,
                    2_000,
                );
                apply_indexed_commitment_tx(
                    &transfer_to_staking_memo_tx(
                        2_000 + tx_id,
                        &staking_id,
                        format!("{}.", raw.to_text()).into_bytes(),
                        100,
                        tx_id * 1_000_000_000,
                    ),
                    &staking_id,
                    100,
                    2_000,
                );
                apply_indexed_commitment_tx(
                    &transfer_to_staking_memo_tx(
                        4_000 + tx_id,
                        &staking_id,
                        b"42.memo".to_vec(),
                        100,
                        tx_id * 1_000_000_000,
                    ),
                    &staking_id,
                    100,
                    2_000,
                );
            }
            drop(_batch);

            let retained = u64::from(cap.min(1_001));
            let expected_cycles = (1_012 - retained..=1_011).collect::<Vec<_>>();
            let expected_raw = expected_cycles
                .iter()
                .map(|tx_id| tx_id + 2_000)
                .collect::<Vec<_>>();
            let expected_neuron = expected_cycles
                .iter()
                .map(|tx_id| tx_id + 4_000)
                .collect::<Vec<_>>();
            assert_eq!(paged_commitment_ids(cycles, false, false), expected_cycles);
            assert_eq!(paged_commitment_ids(raw, false, true), expected_raw);
            assert_eq!(paged_neuron_commitment_ids(42, false), expected_neuron);
            assert_eq!(
                paged_commitment_ids(cycles, true, false),
                expected_cycles.iter().rev().copied().collect::<Vec<_>>()
            );
            assert_eq!(
                paged_commitment_ids(raw, true, true),
                expected_raw.iter().rev().copied().collect::<Vec<_>>()
            );
            assert_eq!(
                paged_neuron_commitment_ids(42, true),
                expected_neuron.iter().rev().copied().collect::<Vec<_>>()
            );
            for route in [
                crate::CommitmentRoute::CyclesTopUp {
                    canister_id: cycles,
                },
                crate::CommitmentRoute::RawIcp {
                    destination_canister_id: raw,
                    memo: Vec::new(),
                },
                crate::CommitmentRoute::NeuronStake {
                    neuron_id: 42,
                    memo: Some(b"memo".to_vec()),
                },
            ] {
                assert_eq!(route_rollup(route).qualifying_commitment_count, 1_001);
            }

            let restored = state::restore_state_from_stable().expect("bounded histories reload");
            state::set_state_root_only(restored);
            assert_eq!(paged_commitment_ids(cycles, false, false), expected_cycles);
            assert_eq!(paged_commitment_ids(raw, false, true), expected_raw);
            assert_eq!(paged_neuron_commitment_ids(42, false), expected_neuron);
        }
    }

    #[test]
    fn endowment_refresh_commits_all_endowment_routes_before_returning() {
        let staking_id = configure_state(10);
        let cycles_target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let raw_target = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let page = index_page(vec![
            transfer_to_staking_tx(9, &staking_id, cycles_target, 101, 9),
            transfer_to_staking_tx(8, &staking_id, cycles_target, 100, 8),
            transfer_to_staking_tx(7, &staking_id, cycles_target, 99, 7),
            transfer_to_staking_memo_tx(
                6,
                &staking_id,
                format!("{}.", raw_target.to_text()).into_bytes(),
                101,
                6,
            ),
            transfer_to_staking_memo_tx(
                5,
                &staking_id,
                format!("{}.", raw_target.to_text()).into_bytes(),
                100,
                5,
            ),
            transfer_to_staking_memo_tx(
                4,
                &staking_id,
                format!("{}.", raw_target.to_text()).into_bytes(),
                99,
                4,
            ),
            transfer_to_staking_memo_tx(3, &staking_id, b"42.memo".to_vec(), 101, 3),
            transfer_to_staking_memo_tx(2, &staking_id, b"42.memo".to_vec(), 100, 2),
            transfer_to_staking_memo_tx(1, &staking_id, b"42.memo".to_vec(), 99, 1),
        ]);
        let index = MockIndexClient::new(vec![page]);

        let response = block_on(refresh_endowments_with_client(&index, 100));

        assert_eq!(response.outcome, crate::RefreshEndowmentsOutcome::Updated);
        assert_eq!(response.progress.newly_indexed_qualifying_endowments, 6);
        assert!(response.progress.complete_from_genesis);
        assert_eq!(
            endowment_transaction_status(9).status,
            crate::ExpectedEndowmentStatus::KnownIndexed
        );
        let summaries =
            crate::get_commitment_route_summaries(crate::GetCommitmentRouteSummariesArgs {
                routes: vec![
                    crate::CommitmentRoute::CyclesTopUp {
                        canister_id: cycles_target,
                    },
                    crate::CommitmentRoute::RawIcp {
                        destination_canister_id: raw_target,
                        memo: Vec::new(),
                    },
                    crate::CommitmentRoute::NeuronStake {
                        neuron_id: 42,
                        memo: Some(b"memo".to_vec()),
                    },
                ],
            });
        assert!(summaries.complete_from_genesis);
        assert_eq!(summaries.revision, Some(response.progress.revision));
        assert_eq!(
            summaries
                .items
                .iter()
                .map(|item| item.total_qualifying_committed_e8s)
                .collect::<Vec<_>>(),
            vec![201, 201, 201]
        );
        assert!(summaries
            .items
            .iter()
            .all(|item| item.qualifying_commitment_count == 2));
        state::with_state(|st| {
            assert!(st.commitment_history.is_empty());
            assert!(st.raw_icp_commitment_history.is_empty());
            assert!(st.neuron_commitment_history.is_empty());
            assert_eq!(
                st.recent_under_threshold_commitments
                    .as_ref()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                st.recent_under_threshold_neuron_commitments
                    .as_ref()
                    .unwrap()
                    .len(),
                1
            );
        });
    }

    #[test]
    fn endowment_refresh_commits_at_most_one_page_and_finishes_the_pinned_interval_before_new_arrivals(
    ) {
        let staking_id = configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(10);
            st.oldest_indexed_staking_tx_id = Some(1);
            st.staking_index_descending = Some(true);
            st.staking_backfill_complete = Some(true);
            st.commitment_route_rollups_complete_from_genesis = Some(true);
        });

        let first = MockIndexClient::new(vec![index_page(
            (512..=1_011)
                .rev()
                .map(|id| transfer_to_staking_tx(id, &staking_id, target, 100, id))
                .collect(),
        )]);
        let first_response = block_on(refresh_endowments_with_client(&first, 100));
        assert_eq!(first.calls().len(), 1);
        assert_eq!(
            first_response.outcome,
            crate::RefreshEndowmentsOutcome::IncompleteProgress
        );
        assert_eq!(
            first_response.progress.newly_indexed_qualifying_endowments,
            500
        );
        assert_eq!(
            first_response.progress.committed_head_staking_tx_id,
            Some(10)
        );
        assert_eq!(
            first_response.progress.observed_head_staking_tx_id,
            Some(1_011)
        );
        assert_eq!(first_response.progress.next_staking_start_tx_id, Some(512));
        assert_eq!(
            endowment_transaction_status(11).status,
            crate::ExpectedEndowmentStatus::NotYetObserved,
            "an ID inside the unread part of a pinned interval remains pending"
        );

        // Transaction 1,012 arrives after the interval was pinned. The next
        // accepted request must continue below 512 instead of restarting at it.
        let second = MockIndexClient::new(vec![index_page(
            (12..=511)
                .rev()
                .map(|id| transfer_to_staking_tx(id, &staking_id, target, 100, id))
                .collect(),
        )]);
        let second_response = block_on(refresh_endowments_with_client(&second, 160));
        assert_eq!(second.calls()[0].1, Some(512));
        assert_eq!(
            second_response.outcome,
            crate::RefreshEndowmentsOutcome::IncompleteProgress
        );
        assert_eq!(
            second_response.progress.newly_indexed_qualifying_endowments,
            500
        );
        assert_eq!(second_response.progress.next_staking_start_tx_id, Some(12));

        let third = MockIndexClient::new(vec![index_page(vec![transfer_to_staking_tx(
            11,
            &staking_id,
            target,
            100,
            11,
        )])]);
        let third_response = block_on(refresh_endowments_with_client(&third, 220));
        assert_eq!(third.calls()[0].1, Some(12));
        assert_eq!(
            third_response.outcome,
            crate::RefreshEndowmentsOutcome::Updated
        );
        assert_eq!(
            third_response.progress.committed_head_staking_tx_id,
            Some(1_011)
        );
        assert!(third_response.progress.complete_from_genesis);

        let fourth = MockIndexClient::new(vec![index_page(vec![
            transfer_to_staking_tx(1_012, &staking_id, target, 100, 1_012),
            transfer_to_staking_tx(1_011, &staking_id, target, 100, 1_011),
        ])]);
        let fourth_response = block_on(refresh_endowments_with_client(&fourth, 280));
        assert_eq!(fourth.calls()[0].1, None);
        assert_eq!(
            fourth_response.outcome,
            crate::RefreshEndowmentsOutcome::Updated
        );
        assert_eq!(
            fourth_response.progress.committed_head_staking_tx_id,
            Some(1_012)
        );
        assert_eq!(
            route_rollup(crate::CommitmentRoute::CyclesTopUp {
                canister_id: target,
            }),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1_002,
                total_qualifying_committed_e8s: 100_200,
            }
        );
    }

    #[test]
    fn descending_endowment_refresh_preserves_a_latched_fault_and_reports_partial_success() {
        let staking_id = configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let fault = state::CommitmentIndexFault {
            observed_at_ts: 50,
            last_cursor_tx_id: Some(10),
            offending_tx_id: 9,
            message: "operator review required".into(),
        };
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(10);
            st.oldest_indexed_staking_tx_id = Some(1);
            st.staking_index_descending = Some(true);
            st.staking_backfill_complete = Some(true);
            st.commitment_route_rollups_complete_from_genesis = Some(true);
            st.commitment_index_fault = Some(fault.clone());
        });
        let index = MockIndexClient::new(vec![index_page(vec![
            transfer_to_staking_tx(50_000, &staking_id, target, 100, 50_000),
            transfer_to_staking_tx(10, &staking_id, target, 100, 10),
        ])]);

        let response = block_on(refresh_endowments_with_client(&index, 100));

        assert_eq!(
            response.outcome,
            crate::RefreshEndowmentsOutcome::IncompleteProgress
        );
        assert_eq!(response.progress.newly_indexed_qualifying_endowments, 1);
        assert_eq!(response.progress.commitment_index_fault, Some(fault));
        assert!(!response.progress.complete_from_genesis);
        assert_eq!(response.progress.committed_head_staking_tx_id, Some(50_000));
    }

    #[test]
    fn endowment_refresh_denials_are_cheap_and_do_not_slide_backoff() {
        let _staking_id = configure_state(10);
        let first = MockIndexClient::new(vec![index_page(Vec::new())]);
        let response = block_on(refresh_endowments_with_client(&first, 100));
        assert_eq!(
            response.outcome,
            crate::RefreshEndowmentsOutcome::NoQualifyingChange
        );
        assert_eq!(response.progress.retry_after_ts, Some(160));
        assert_eq!(first.calls().len(), 1);

        let denied = MockIndexClient::new(Vec::new());
        let root_before_denials = candid::encode_one(
            state::restore_state_from_stable().expect("accepted attempt persisted root state"),
        )
        .unwrap();
        for now_secs in [101, 120, 159] {
            let response = block_on(refresh_endowments_with_client(&denied, now_secs));
            assert_eq!(
                response.outcome,
                crate::RefreshEndowmentsOutcome::RateLimited
            );
            assert_eq!(response.progress.retry_after_ts, Some(160));
        }
        assert!(denied.calls().is_empty());
        assert_eq!(
            candid::encode_one(
                state::restore_state_from_stable().expect("denials preserve stable root state")
            )
            .unwrap(),
            root_before_denials,
            "update-level denials create no caller/transaction records or stable-root writes",
        );

        let second = MockIndexClient::new(vec![index_page(Vec::new())]);
        let response = block_on(refresh_endowments_with_client(&second, 160));
        assert_eq!(
            response.outcome,
            crate::RefreshEndowmentsOutcome::NoQualifyingChange
        );
        assert_eq!(response.progress.retry_after_ts, Some(280));
        assert_eq!(second.calls().len(), 1);
    }

    #[test]
    fn busy_and_rate_limited_refreshes_skip_the_index() {
        let _staking_id = configure_state(10);
        let scheduled =
            CommitmentIndexGuard::acquire(100, state::CommitmentIndexLeaseOwner::Scheduled)
                .unwrap();
        let index = MockIndexClient::new(Vec::new());
        let busy = block_on(refresh_endowments_with_client(&index, 101));
        assert_eq!(busy.outcome, crate::RefreshEndowmentsOutcome::Busy);
        assert!(index.calls().is_empty());
        drop(scheduled);

        state::with_state_mut(|st| st.endowment_refresh_next_allowed_ts = 200);
        let rate_limited = block_on(refresh_endowments_with_client(&index, 150));
        assert_eq!(
            rate_limited.outcome,
            crate::RefreshEndowmentsOutcome::RateLimited
        );
        assert!(index.calls().is_empty());
    }

    #[test]
    fn one_refresh_reserves_global_admission_from_concurrent_callers() {
        let _staking_id = configure_state(10);
        let (first_index, sender) = DelayedIndexClient::new();
        let mut first = Box::pin(refresh_endowments_with_client(&first_index, 100));
        assert!(first.as_mut().now_or_never().is_none());
        assert_eq!(*first_index.calls.lock().unwrap(), 1);

        let second_index = MockIndexClient::new(Vec::new());
        let second = block_on(refresh_endowments_with_client(&second_index, 100));
        assert_eq!(second.outcome, crate::RefreshEndowmentsOutcome::Busy);
        assert!(second_index.calls().is_empty());

        sender.send(Ok(index_page(Vec::new()))).unwrap();
        let completed = block_on(first);
        assert_eq!(
            completed.outcome,
            crate::RefreshEndowmentsOutcome::NoQualifyingChange
        );
        assert_eq!(*first_index.calls.lock().unwrap(), 1);
    }

    #[test]
    fn transaction_status_query_distinguishes_invalid_pending_and_unretained_evidence() {
        let staking_id = configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let index = MockIndexClient::new(vec![index_page(vec![
            transfer_to_staking_memo_tx(100, &staking_id, b"invalid".to_vec(), 100, 100),
            transfer_to_staking_tx(99, &staking_id, target, 100, 99),
        ])]);
        let _ = block_on(refresh_endowments_with_client(&index, 100));
        assert_eq!(
            endowment_transaction_status(100).status,
            crate::ExpectedEndowmentStatus::ObservedNotQualifying
        );

        assert_eq!(
            endowment_transaction_status(101).status,
            crate::ExpectedEndowmentStatus::NotYetObserved
        );
        assert_eq!(
            endowment_transaction_status(98).status,
            crate::ExpectedEndowmentStatus::NotFoundInRetainedEvidence,
            "a larger observed global ID is not evidence that the hinted transfer qualified"
        );
    }

    #[test]
    fn endowment_refresh_backoff_caps_and_failed_outcalls_consume_attempts() {
        let _staking_id = configure_state(10);
        let schedule = [
            (100, 160),
            (160, 280),
            (280, 520),
            (520, 1_000),
            (1_000, 1_600),
            (1_600, 2_200),
        ];
        for (now_secs, expected_retry) in schedule {
            let index = MockIndexClient::new(vec![index_page(Vec::new())]);
            let response = block_on(refresh_endowments_with_client(&index, now_secs));
            assert_eq!(response.progress.retry_after_ts, Some(expected_retry));
            assert_eq!(index.calls().len(), 1);
        }

        let failed = MockIndexClient::scripted(vec![Err(crate::clients::ClientError::Call(
            "upstream unavailable".into(),
        ))]);
        let response = block_on(refresh_endowments_with_client(&failed, 2_200));
        assert!(matches!(
            response.outcome,
            crate::RefreshEndowmentsOutcome::UpstreamFailure { .. }
        ));
        assert_eq!(response.progress.retry_after_ts, Some(2_800));
        assert_eq!(failed.calls().len(), 1);

        state::with_state_mut(|st| st.endowment_refresh_next_allowed_ts = 0);
        let unicode_failure = MockIndexClient::scripted(vec![Err(
            crate::clients::ClientError::Call("\u{1f6a8}".repeat(600)),
        )]);
        let response = block_on(refresh_endowments_with_client(&unicode_failure, 3_000));
        let crate::RefreshEndowmentsOutcome::UpstreamFailure { message } = response.outcome else {
            panic!("expected bounded upstream failure")
        };
        assert!(
            message.len() <= 512,
            "diagnostics are bounded in encoded bytes"
        );
        assert!(message.is_char_boundary(message.len()));
    }

    #[test]
    fn busy_endowment_refresh_is_cheap_and_does_not_reserve_cooldown() {
        let _staking_id = configure_state(10);
        let scheduled =
            CommitmentIndexGuard::acquire(100, state::CommitmentIndexLeaseOwner::Scheduled)
                .unwrap();
        let denied = MockIndexClient::new(Vec::new());
        let response = block_on(refresh_endowments_with_client(&denied, 101));
        assert_eq!(response.outcome, crate::RefreshEndowmentsOutcome::Busy);
        assert!(denied.calls().is_empty());
        assert_eq!(
            state::with_state(|st| st.endowment_refresh_next_allowed_ts),
            0
        );
        drop(scheduled);
    }

    #[test]
    fn stale_endowment_refresh_callback_cannot_commit_or_release_the_timer_lease() {
        let staking_id = configure_state(10);
        let target = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let (index, sender) = DelayedIndexClient::new();
        let mut refresh = Box::pin(refresh_endowments_with_client(&index, 100));
        assert!(refresh.as_mut().now_or_never().is_none());
        assert_eq!(*index.calls.lock().unwrap(), 1);

        let scheduled =
            CommitmentIndexGuard::acquire(101, state::CommitmentIndexLeaseOwner::Scheduled)
                .unwrap();
        sender
            .send(Ok(index_page(vec![transfer_to_staking_tx(
                1,
                &staking_id,
                target,
                100,
                1,
            )])))
            .unwrap();
        let response = block_on(refresh);
        assert_eq!(response.outcome, crate::RefreshEndowmentsOutcome::Busy);
        assert_eq!(
            route_rollup(crate::CommitmentRoute::CyclesTopUp {
                canister_id: target,
            }),
            state::CommitmentRouteRollup::default()
        );
        assert!(scheduled.token().is_current());
        drop(scheduled);
    }

    #[test]
    fn scheduled_indexing_preempts_endowment_refresh_lease_without_stale_release() {
        let _staking_id = configure_state(10);
        let refresh =
            CommitmentIndexGuard::acquire(100, state::CommitmentIndexLeaseOwner::EndowmentRefresh)
                .unwrap();
        let refresh_token = refresh.token();
        let timer = CommitmentIndexGuard::acquire(101, state::CommitmentIndexLeaseOwner::Scheduled)
            .unwrap();
        assert!(!refresh_token.is_current());
        assert!(timer.token().is_current());

        drop(refresh);
        assert!(timer.token().is_current());
        drop(timer);
        state::with_state(|st| {
            assert_eq!(st.commitment_index_lock_expires_at_ts, Some(0));
            assert_eq!(st.commitment_index_lock_owner, None);
        });
    }

    #[test]
    fn lease_renewal_obeys_expiry_boundaries_and_cannot_touch_a_successor() {
        configure_state(1);
        let scheduled =
            CommitmentIndexGuard::acquire(100, state::CommitmentIndexLeaseOwner::Scheduled)
                .unwrap();
        let original = scheduled.token();
        let renewed = original.renew(174).unwrap();
        assert!(!original.is_current());
        assert!(renewed.is_current());
        assert!(CommitmentIndexGuard::acquire(
            248,
            state::CommitmentIndexLeaseOwner::EndowmentRefresh
        )
        .is_none());

        let successor =
            CommitmentIndexGuard::acquire(249, state::CommitmentIndexLeaseOwner::EndowmentRefresh)
                .expect("the exact expiry boundary permits stuck-owner recovery");
        assert!(renewed.renew(249).is_err());
        drop(scheduled);
        assert!(successor.token().is_current());
        drop(successor);
    }

    #[test]
    fn scheduled_multi_page_indexing_renews_lease_at_page_boundaries() {
        let staking_id = configure_state(2);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let first_page = index_page(
            (2..=(PAGE_SIZE + 1))
                .rev()
                .map(|id| transfer_to_staking_tx(id, &staking_id, canister, 100, id))
                .collect(),
        );
        let second_page = index_page(vec![transfer_to_staking_tx(
            1,
            &staking_id,
            canister,
            100,
            1,
        )]);
        let clock = Arc::new(AtomicU64::new(100));
        let index =
            LeaseClockIndexClient::new(vec![first_page, second_page], clock.clone(), 2, 160, 180);
        let scheduled =
            CommitmentIndexGuard::acquire(100, state::CommitmentIndexLeaseOwner::Scheduled)
                .unwrap();

        block_on(process_commitment_indexing_bounded(
            &index,
            100,
            2,
            Some(scheduled.token()),
            &|| index.now(),
        ))
        .unwrap();

        assert!(!index.refresh_acquired.load(Ordering::SeqCst));
        assert_eq!(
            state::with_state(|st| st.qualifying_commitment_count),
            Some(PAGE_SIZE + 1)
        );
    }

    #[test]
    fn main_tick_acquires_scheduled_index_lease_after_delayed_xrc() {
        let staking_id = configure_state(1);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let clock = Arc::new(AtomicU64::new(100));
        let index = LeaseClockIndexClient::new(
            vec![
                index_page(vec![transfer_to_staking_tx(
                    1,
                    &staking_id,
                    canister,
                    100,
                    1,
                )]),
                index_page(Vec::new()),
            ],
            clock.clone(),
            1,
            191,
            191,
        );
        let xrc = ClockAdvancingXrc { clock: &clock };
        let cycles_probe = RecordingCyclesProbeClient::blackhole(0);
        let sns_wasm = MockSnsWasmClient::new(Vec::new());
        let sns_root = MockSnsRootClient::new(BTreeMap::new());
        let governance = RecordingGovernanceClient::new();

        block_on(run_main_tick_with_clients(
            100_000_000_000,
            100,
            &index,
            &cycles_probe,
            &sns_wasm,
            &sns_root,
            &governance,
            &xrc,
            &|| clock.load(Ordering::SeqCst),
        ))
        .unwrap();

        assert!(!index.refresh_acquired.load(Ordering::SeqCst));
        assert_eq!(
            index.calls(),
            2,
            "route indexing continues after staking indexing"
        );
        assert_eq!(
            state::with_state(|st| st.qualifying_commitment_count),
            Some(1)
        );
        assert!(cycles_probe.blackhole_targets().contains(&canister));
    }

    #[test]
    fn newest_first_commitment_route_completeness_covers_empty_genesis() {
        let _staking_id = configure_state(10);
        assert_eq!(
            state::with_state(|st| st.commitment_route_rollups_complete_from_genesis),
            Some(false)
        );
        let empty = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: Vec::new(),
            oldest_tx_id: None,
        }]);
        block_on(process_commitment_indexing(&empty, 200)).unwrap();
        assert_eq!(
            state::with_state(|st| st.commitment_route_rollups_complete_from_genesis),
            Some(true)
        );
        block_on(process_commitment_indexing(&empty, 201)).unwrap();
        assert_eq!(
            state::with_state(|st| st.commitment_route_rollups_complete_from_genesis),
            Some(true)
        );
    }

    #[test]
    fn newest_first_genesis_backfill_resumes_after_second_page_failure() {
        let staking_id = configure_state(2);
        state::with_state_mut(|st| st.config.max_commitment_entries_per_canister = 1);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let route_key =
            state::CommitmentRouteKey::from_public(&crate::CommitmentRoute::CyclesTopUp {
                canister_id: canister,
            })
            .unwrap();
        let first_page = GetAccountIdentifierTransactionsResponse {
            balance: (PAGE_SIZE + 1) * 150,
            transactions: (2..=(PAGE_SIZE + 1))
                .rev()
                .map(|tx_id| transfer_to_staking_tx(tx_id, &staking_id, canister, 150, tx_id))
                .collect(),
            oldest_tx_id: Some(2),
        };
        let failing = MockIndexClient::scripted(vec![
            Ok(first_page),
            Err(crate::clients::ClientError::Call(
                "transient second-page failure".into(),
            )),
        ]);

        let err = block_on(process_commitment_indexing(&failing, 200)).unwrap_err();
        assert!(err.contains("transient second-page failure"));
        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(PAGE_SIZE + 1));
            assert_eq!(st.oldest_indexed_staking_tx_id, Some(2));
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(false)
            );
            let history = st.commitment_history.get(&canister).unwrap();
            assert_eq!(history.len(), 1);
            assert_eq!(history[0].tx_id, PAGE_SIZE + 1);
        });
        assert_eq!(
            state::get_commitment_route_rollup(&route_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: PAGE_SIZE,
                total_qualifying_committed_e8s: PAGE_SIZE * 150,
            }
        );
        assert_eq!(failing.calls()[1].1, Some(2));

        let retry = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: (PAGE_SIZE + 1) * 150,
            transactions: vec![transfer_to_staking_tx(1, &staking_id, canister, 150, 1)],
            oldest_tx_id: Some(1),
        }]);
        block_on(process_commitment_indexing(&retry, 201)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, Some(PAGE_SIZE + 1));
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(true)
            );
            assert_eq!(st.commitment_history.get(&canister).unwrap().len(), 1);
        });
        assert_eq!(retry.calls()[0].1, Some(2));
        assert_eq!(
            state::get_commitment_route_rollup(&route_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: PAGE_SIZE + 1,
                total_qualifying_committed_e8s: (PAGE_SIZE + 1) * 150,
            },
            "the failed second-page request resumes below the durable oldest cursor"
        );
    }

    #[test]
    fn debug_derived_state_reset_clears_route_rollups_before_genesis_reindex() {
        let staking_id = configure_state(1);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let route = crate::CommitmentRoute::CyclesTopUp {
            canister_id: canister,
        };
        let history = index_page(vec![transfer_to_staking_tx(
            1,
            &staking_id,
            canister,
            125,
            1,
        )]);

        block_on(process_commitment_indexing(
            &MockIndexClient::new(vec![history.clone()]),
            100,
        ))
        .unwrap();
        assert_eq!(
            route_rollup(route.clone()),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 125,
            }
        );

        crate::debug::reset_derived_state_for_debug();
        assert_eq!(route_rollup(route.clone()), Default::default());
        state::with_state(|st| {
            assert_eq!(st.last_indexed_staking_tx_id, None);
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(false)
            );
        });

        block_on(process_commitment_indexing(
            &MockIndexClient::new(vec![history]),
            200,
        ))
        .unwrap();
        assert_eq!(
            route_rollup(route),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 1,
                total_qualifying_committed_e8s: 125,
            }
        );
    }

    #[test]
    fn fresh_install_descending_index_marks_route_rollups_complete_only_at_genesis() {
        let staking_id = configure_state(1);
        state::with_state_mut(|st| st.config.max_commitment_entries_per_canister = 1);
        let canister = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let raw_empty = format!("{}.", canister.to_text()).into_bytes();
        let latest = 100 + PAGE_SIZE;
        let mut first_page: Vec<_> = (101..=latest)
            .rev()
            .map(|id| {
                let (memo, amount) = match id {
                    id if id == latest => (raw_empty.clone(), 150),
                    id if id == latest - 1 => (b"42.".to_vec(), 160),
                    id if id == latest - 2 => (raw_empty.clone(), 99),
                    101 => (raw_empty.clone(), 200),
                    _ => (b"invalid".to_vec(), 1),
                };
                transfer_to_staking_memo_tx(id, &staking_id, memo, amount, id)
            })
            .collect();
        assert_eq!(first_page.len(), PAGE_SIZE as usize);
        let first = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: std::mem::take(&mut first_page),
            oldest_tx_id: Some(1),
        }]);
        block_on(process_commitment_indexing(&first, 200)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.staking_index_descending, Some(true));
            assert_eq!(st.staking_backfill_complete, Some(false));
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(false)
            );
        });

        let restored = state::restore_state_from_stable().expect("partial backfill should persist");
        assert_eq!(
            restored.commitment_route_rollups_complete_from_genesis,
            Some(false)
        );
        state::set_state_root_only(restored);
        state::with_state_mut(|st| st.config.max_index_pages_per_tick = 2);
        let second = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: vec![
                transfer_to_staking_memo_tx(100, &staking_id, raw_empty.clone(), 170, 100),
                transfer_to_staking_memo_tx(99, &staking_id, b"42.".to_vec(), 180, 99),
            ],
            oldest_tx_id: Some(99),
        }]);
        block_on(process_commitment_indexing(&second, 201)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.staking_backfill_complete, Some(true));
            assert_eq!(
                st.commitment_route_rollups_complete_from_genesis,
                Some(true)
            );
        });
        let raw_key = state::CommitmentRouteKey::from_public(&crate::CommitmentRoute::RawIcp {
            destination_canister_id: canister,
            memo: Vec::new(),
        })
        .unwrap();
        let neuron_key =
            state::CommitmentRouteKey::from_public(&crate::CommitmentRoute::NeuronStake {
                neuron_id: 42,
                memo: Some(Vec::new()),
            })
            .unwrap();
        assert_eq!(
            state::get_commitment_route_rollup(&raw_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 3,
                total_qualifying_committed_e8s: 520,
            },
            "the exclusive Index cursor resumes immediately below the persisted oldest page"
        );
        assert_eq!(
            state::get_commitment_route_rollup(&neuron_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 2,
                total_qualifying_committed_e8s: 340,
            }
        );

        let third = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 0,
            transactions: vec![
                transfer_to_staking_memo_tx(latest + 1, &staking_id, raw_empty, 190, latest + 1),
                transfer_to_staking_memo_tx(latest, &staking_id, b"ignored".to_vec(), 1, latest),
            ],
            oldest_tx_id: Some(1),
        }]);
        block_on(process_commitment_indexing(&third, 202)).unwrap();
        assert_eq!(
            state::get_commitment_route_rollup(&raw_key),
            state::CommitmentRouteRollup {
                qualifying_commitment_count: 4,
                total_qualifying_committed_e8s: 710,
            }
        );
        assert_eq!(
            state::with_state(|st| st.commitment_route_rollups_complete_from_genesis),
            Some(true)
        );
    }

    #[test]
    fn route_indexing_counts_only_protocol_routed_output_and_rewards_and_resumes_across_ticks() {
        let _staking_id = configure_state(10);
        let (source, output, rewards) = state::with_state(|st| {
            (
                st.config.output_source_account.clone(),
                st.config.output_account.clone(),
                st.config.rewards_account.clone(),
            )
        });
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
            let active = st
                .active_route_sweep
                .as_ref()
                .expect("route sweep should continue to rewards");
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
    fn descending_output_catch_up_uses_original_boundary_across_multiple_pages() {
        let _staking_id = configure_state(3);
        let (source, output) =
            state::with_state(|st| (st.config.output_source_account, st.config.output_account));
        let source_id = account_identifier_text_for_account(&source);
        let output_id = account_identifier_text_for_account(&output);
        state::with_state_mut(|st| {
            st.last_indexed_output_tx_id = Some(10);
            st.oldest_indexed_output_tx_id = Some(1);
            st.output_route_index_descending = Some(true);
            st.output_route_backfill_complete = Some(true);
        });

        let newest_page = index_page(
            (512..=1_011)
                .rev()
                .map(|id| transfer_between_accounts_tx(id, &source_id, &output_id, 1, id))
                .collect(),
        );
        let older_page = index_page(
            (12..=511)
                .rev()
                .map(|id| transfer_between_accounts_tx(id, &source_id, &output_id, 1, id))
                .collect(),
        );
        let oldest_page = index_page(vec![transfer_between_accounts_tx(
            11, &source_id, &output_id, 1, 11,
        )]);
        let index = MockIndexClient::new(vec![newest_page, older_page, oldest_page]);

        block_on(process_route_indexing(100, 200, &index)).unwrap();

        assert_eq!(index.calls()[0].1, None);
        assert_eq!(index.calls()[1].1, Some(512));
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(1_001));
            assert_eq!(st.last_indexed_output_tx_id, Some(1_011));
        });
    }

    #[test]
    fn descending_output_catch_up_resumes_after_second_page_failure_and_restore() {
        let _staking_id = configure_state(2);
        let (source_id, output_id) = state::with_state(|st| {
            (
                account_identifier_text_for_account(&st.config.output_source_account),
                account_identifier_text_for_account(&st.config.output_account),
            )
        });
        state::with_state_mut(|st| {
            st.total_output_e8s = Some(0);
            st.last_indexed_output_tx_id = Some(10);
            st.oldest_indexed_output_tx_id = Some(1);
            st.output_route_index_descending = Some(true);
            st.output_route_backfill_complete = Some(true);
        });

        let first_page = index_page(
            (512..=1_011)
                .rev()
                .map(|id| transfer_between_accounts_tx(id, &source_id, &output_id, 1, id))
                .collect(),
        );
        let failing = MockIndexClient::scripted(vec![
            Ok(first_page),
            Err(crate::clients::ClientError::Call(
                "transient second output page failure".into(),
            )),
        ]);
        let error = block_on(process_route_indexing(100, 200, &failing)).unwrap_err();
        assert!(error.contains("transient second output page failure"));
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(500));
            assert_eq!(st.last_indexed_output_tx_id, Some(10));
            assert_eq!(
                st.active_output_catch_up
                    .as_ref()
                    .and_then(|progress| progress.next_start_tx_id),
                Some(512)
            );
        });

        let restored = state::restore_state_from_stable().expect("output continuation persists");
        state::set_state_root_only(restored);
        let retry = MockIndexClient::new(vec![
            index_page(
                (12..=511)
                    .rev()
                    .map(|id| transfer_between_accounts_tx(id, &source_id, &output_id, 1, id))
                    .collect(),
            ),
            index_page(vec![transfer_between_accounts_tx(
                11, &source_id, &output_id, 1, 11,
            )]),
        ]);
        block_on(process_route_indexing(101, 201, &retry)).unwrap();
        assert_eq!(retry.calls()[0].1, Some(512));
        assert_eq!(retry.calls()[1].1, Some(12));
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(1_001));
            assert_eq!(st.last_indexed_output_tx_id, Some(1_011));
            assert!(st.active_output_catch_up.is_none());
        });
    }

    #[test]
    fn descending_rewards_catch_up_resumes_after_failure_and_restore() {
        configure_state(2);
        let (source_id, rewards_id) = state::with_state(|st| {
            (
                account_identifier_text_for_account(&st.config.output_source_account),
                account_identifier_text_for_account(&st.config.rewards_account),
            )
        });
        state::with_state_mut(|st| {
            st.total_rewards_e8s = Some(0);
            st.last_indexed_rewards_tx_id = Some(10);
            st.oldest_indexed_rewards_tx_id = Some(1);
            st.rewards_route_index_descending = Some(true);
            st.rewards_route_backfill_complete = Some(true);
            st.active_route_sweep = Some(ActiveRouteSweep {
                started_at_ts_nanos: 100,
                next_index: 1,
            });
        });
        let first_page = index_page(
            (512..=1_011)
                .rev()
                .map(|id| transfer_between_accounts_tx(id, &source_id, &rewards_id, 1, id))
                .collect(),
        );
        let failing = MockIndexClient::scripted(vec![
            Ok(first_page),
            Err(crate::clients::ClientError::Call(
                "transient second rewards page failure".into(),
            )),
        ]);
        let error = block_on(process_route_indexing(100, 200, &failing)).unwrap_err();
        assert!(error.contains("transient second rewards page failure"));
        assert_eq!(state::with_state(|st| st.total_rewards_e8s), Some(500));

        let restored = state::restore_state_from_stable().expect("rewards continuation persists");
        state::set_state_root_only(restored);
        let retry = MockIndexClient::new(vec![
            index_page(
                (12..=511)
                    .rev()
                    .map(|id| transfer_between_accounts_tx(id, &source_id, &rewards_id, 1, id))
                    .collect(),
            ),
            index_page(vec![transfer_between_accounts_tx(
                11,
                &source_id,
                &rewards_id,
                1,
                11,
            )]),
        ]);
        block_on(process_route_indexing(101, 201, &retry)).unwrap();
        assert_eq!(retry.calls()[0].1, Some(512));
        assert_eq!(retry.calls()[1].1, Some(12));
        state::with_state(|st| {
            assert_eq!(st.total_rewards_e8s, Some(1_001));
            assert_eq!(st.last_indexed_rewards_tx_id, Some(1_011));
            assert!(st.active_rewards_catch_up.is_none());
        });
    }

    #[test]
    fn route_indexing_counts_transfer_from_and_skips_repeated_cursor_without_double_counting() {
        let _staking_id = configure_state(1);
        let (source, output, rewards) = state::with_state(|st| {
            (
                st.config.output_source_account.clone(),
                st.config.output_account.clone(),
                st.config.rewards_account.clone(),
            )
        });
        let source_id = account_identifier_text_for_account(&source);
        let output_id = account_identifier_text_for_account(&output);
        let rewards_id = account_identifier_text_for_account(&rewards);
        let mock = MockIndexClient::new(vec![
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![transfer_from_between_accounts_tx(
                    10,
                    &source_id,
                    &output_id,
                    111_000_000,
                    10,
                )],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![transfer_between_accounts_tx(
                    30,
                    &source_id,
                    &rewards_id,
                    5_000_000,
                    30,
                )],
                oldest_tx_id: Some(30),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![
                    transfer_between_accounts_tx(12, "third-party", &output_id, 333_000_000, 22),
                    transfer_between_accounts_tx(11, &source_id, &output_id, 22_000_000, 21),
                    transfer_from_between_accounts_tx(10, &source_id, &output_id, 999_000_000, 20),
                ],
                oldest_tx_id: Some(10),
            },
            GetAccountIdentifierTransactionsResponse {
                balance: 0,
                transactions: vec![transfer_between_accounts_tx(
                    30,
                    &source_id,
                    &rewards_id,
                    5_000_000,
                    30,
                )],
                oldest_tx_id: Some(30),
            },
        ]);

        block_on(process_route_indexing(100, 200, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(10));
            assert_eq!(
                st.active_route_sweep
                    .as_ref()
                    .map(|active| active.next_index),
                Some(1)
            );
        });

        block_on(process_route_indexing(101, 201, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(111_000_000));
            assert_eq!(st.total_rewards_e8s, Some(5_000_000));
            assert!(st.active_route_sweep.is_none());
        });

        block_on(process_route_indexing(102, 202, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000), "the repeated boundary transfer is skipped while the new routed transfer is counted once");
            assert_eq!(st.total_rewards_e8s, Some(5_000_000));
            assert_eq!(st.last_indexed_output_tx_id, Some(12));
            assert_eq!(
                st.active_route_sweep
                    .as_ref()
                    .map(|active| active.next_index),
                Some(1)
            );
        });

        block_on(process_route_indexing(103, 203, &mock)).unwrap();
        state::with_state(|st| {
            assert_eq!(st.total_output_e8s, Some(133_000_000));
            assert_eq!(st.total_rewards_e8s, Some(5_000_000));
            assert!(st.active_route_sweep.is_none());
        });
    }

    #[test]
    fn faulted_partially_applied_ascending_state_remains_latched_and_unchanged() {
        configure_state(10);
        state::with_state_mut(|st| {
            st.last_indexed_staking_tx_id = Some(50);
            st.oldest_indexed_staking_tx_id = Some(25);
            st.staking_index_descending = Some(false);
            st.staking_backfill_complete = Some(false);
            st.commitment_route_rollups_complete_from_genesis = Some(false);
            st.commitment_index_fault = Some(crate::state::CommitmentIndexFault {
                observed_at_ts: 150,
                last_cursor_tx_id: Some(49),
                offending_tx_id: 50,
                message: "pre-existing coverage fault".to_string(),
            });
        });
        let mock = MockIndexClient::new(vec![index_page(Vec::new())]);

        let err = block_on(process_commitment_indexing(&mock, 200)).unwrap_err();
        assert!(err.contains("unsupported persisted ascending"));
        assert!(mock.calls().is_empty());
        state::with_state(|st| {
            let fault = st
                .commitment_index_fault
                .as_ref()
                .expect("fault should be latched");
            assert_eq!(
                fault.observed_at_ts, 150,
                "the original fault remains latched"
            );
            assert_eq!(st.last_indexed_staking_tx_id, Some(50));
            assert_eq!(st.oldest_indexed_staking_tx_id, Some(25));
            assert_eq!(st.qualifying_commitment_count, Some(0));
        });
    }

    #[test]
    fn indexing_retains_non_qualifying_and_invalid_memo_commitments_in_separate_recent_lists_without_registering_under_threshold_canisters(
    ) {
        let staking_id = configure_state(10);
        let qualifying = principal("jufzc-caaaa-aaaar-qb5da-cai");
        let low_amount = principal("j5gs6-uiaaa-aaaar-qb5cq-cai");
        let mock = MockIndexClient::new(vec![GetAccountIdentifierTransactionsResponse {
            balance: 410,
            transactions: vec![
                transfer_to_staking_tx(42, &staking_id, qualifying, 150, 123_000_000_000),
                transfer_to_staking_tx(43, &staking_id, low_amount, 50, 124_000_000_000),
                transfer_to_staking_memo_tx(
                    44,
                    &staking_id,
                    b"not-a-principal".to_vec(),
                    210,
                    125_000_000_000,
                ),
            ],
            oldest_tx_id: Some(42),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(1));
            assert_eq!(
                st.recent_commitments.as_ref().map(|items| items.len()),
                Some(1)
            );
            assert_eq!(
                st.recent_under_threshold_commitments
                    .as_ref()
                    .map(|items| items.len()),
                Some(1),
            );
            assert_eq!(
                st.recent_invalid_commitments
                    .as_ref()
                    .map(|items| items.len()),
                Some(1)
            );
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 42);
            assert_eq!(
                st.recent_under_threshold_commitments.as_ref().unwrap()[0].tx_id,
                43
            );
            assert!(!st.canister_tracking_reasons.contains_key(&low_amount));
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
                    transfer_to_staking_tx(tx_id, &staking_id, canister, 5, tx_id * 1_000_000_000)
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
            assert_eq!(
                st.recent_commitments.as_ref().map(|items| items.len()),
                Some(0)
            );
            assert_eq!(st.canister_tracking_reasons.len(), 0);
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
            st.canister_tracking_reasons.insert(
                existing,
                crate::logic::merge_tracking_reasons(None, CanisterTrackingReason::MemoCommitment),
            );
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
            transactions: vec![transfer_to_staking_tx(
                9_999,
                &staking_id,
                new_canister,
                150,
                123_000_000_000,
            )],
            oldest_tx_id: Some(9_999),
        }]);

        block_on(process_commitment_indexing(&mock, 200)).unwrap();

        state::with_state(|st| {
            assert_eq!(st.qualifying_commitment_count, Some(2));
            assert_eq!(
                st.recent_commitments.as_ref().map(|items| items.len()),
                Some(1)
            );
            assert_eq!(st.recent_commitments.as_ref().unwrap()[0].tx_id, 9_999);
            assert!(st.canister_tracking_reasons.contains_key(&new_canister));
            assert!(st.commitment_history.contains_key(&new_canister));
            assert!(st.distinct_canisters.contains(&new_canister));
            assert!(st.distinct_canisters.contains(&existing));
        });
    }
}
