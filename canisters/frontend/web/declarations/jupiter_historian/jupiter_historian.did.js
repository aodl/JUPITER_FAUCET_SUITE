export const idlFactory = ({ IDL }) => {
  const Account = IDL.Record({
    'owner' : IDL.Principal,
    'subaccount' : IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const InitArgs = IDL.Record({
    'canonical_relay_canister_id' : IDL.Opt(IDL.Principal),
    'io_surplus_neuron_id' : IDL.Opt(IDL.Nat64),
    'min_tx_e8s' : IDL.Opt(IDL.Nat64),
    'cmc_canister_id' : IDL.Opt(IDL.Principal),
    'xrc_canister_id' : IDL.Opt(IDL.Principal),
    'relay_cycle_safety_margin_e8s' : IDL.Opt(IDL.Nat64),
    'relay_setup_min_e8s' : IDL.Opt(IDL.Nat64),
    'relay_factory_enabled' : IDL.Opt(IDL.Bool),
    'staking_account' : Account,
    'max_canisters_per_cycles_tick' : IDL.Opt(IDL.Nat32),
    'self_service_relay_interval_seconds' : IDL.Opt(IDL.Nat64),
    'enable_sns_tracking' : IDL.Opt(IDL.Bool),
    'relay_initial_cycles' : IDL.Opt(IDL.Nat),
    'relay_min_subaccount_one_seed_e8s' : IDL.Opt(IDL.Nat64),
    'max_index_pages_per_tick' : IDL.Opt(IDL.Nat32),
    'rewards_account' : IDL.Opt(Account),
    'max_commitment_entries_per_canister' : IDL.Opt(IDL.Nat32),
    'output_source_account' : IDL.Opt(Account),
    'cycles_interval_seconds' : IDL.Opt(IDL.Nat64),
    'max_cycles_entries_per_canister' : IDL.Opt(IDL.Nat32),
    'index_canister_id' : IDL.Opt(IDL.Principal),
    'faucet_canister_id' : IDL.Opt(IDL.Principal),
    'ledger_canister_id' : IDL.Opt(IDL.Principal),
    'output_account' : IDL.Opt(Account),
    'canonical_relay_targets' : IDL.Opt(IDL.Vec(IDL.Principal)),
    'scan_interval_seconds' : IDL.Opt(IDL.Nat64),
    'sns_wasm_canister_id' : IDL.Opt(IDL.Principal),
  });
  const FindCanistersByMemoPrefixArgs = IDL.Record({
    'limit' : IDL.Opt(IDL.Nat32),
    'prefix' : IDL.Text,
  });
  const CanisterTrackingReason = IDL.Variant({
    'SnsDiscovery' : IDL.Null,
    'RelayTarget' : IDL.Null,
    'RelayInstance' : IDL.Null,
    'MemoCommitment' : IDL.Null,
  });
  const CanisterPrefixMatch = IDL.Record({
    'last_cycles_probe_ts' : IDL.Opt(IDL.Nat64),
    'latest_cycles' : IDL.Opt(IDL.Nat),
    'total_qualifying_committed_e8s' : IDL.Nat64,
    'qualifying_commitment_count' : IDL.Nat64,
    'canister_id' : IDL.Principal,
    'tracking_reasons' : IDL.Vec(CanisterTrackingReason),
    'matched_prefix' : IDL.Text,
    'last_commitment_ts' : IDL.Opt(IDL.Nat64),
  });
  const FindCanistersByMemoPrefixResponse = IDL.Record({
    'truncated' : IDL.Bool,
    'items' : IDL.Vec(CanisterPrefixMatch),
  });
  const CanisterModuleHash = IDL.Record({
    'stable_memory_bytes' : IDL.Opt(IDL.Nat64),
    'controllers' : IDL.Opt(IDL.Vec(IDL.Principal)),
    'canister_id' : IDL.Principal,
    'heap_memory_bytes' : IDL.Opt(IDL.Nat64),
    'module_hash_hex' : IDL.Opt(IDL.Text),
    'total_memory_bytes' : IDL.Opt(IDL.Nat64),
  });
  const CyclesSampleSource = IDL.Variant({
    'SnsSwapStatus' : IDL.Null,
    'BlackholeStatus' : IDL.Null,
    'SnsRootStatus' : IDL.Null,
    'SelfCanister' : IDL.Null,
    'SnsRootSummary' : IDL.Null,
  });
  const CyclesProbeResult = IDL.Variant({
    'Ok' : CyclesSampleSource,
    'Error' : IDL.Text,
    'NotAvailable' : IDL.Null,
  });
  const CanisterMeta = IDL.Record({
    'last_cycles_probe_ts' : IDL.Opt(IDL.Nat64),
    'last_cycles_probe_result' : IDL.Opt(CyclesProbeResult),
    'first_seen_ts' : IDL.Opt(IDL.Nat64),
    'last_commitment_ts' : IDL.Opt(IDL.Nat64),
  });
  const CanisterOverview = IDL.Record({
    'meta' : CanisterMeta,
    'canister_id' : IDL.Principal,
    'cycles_points' : IDL.Nat32,
    'commitment_points' : IDL.Nat32,
    'tracking_reasons' : IDL.Vec(CanisterTrackingReason),
  });
  const GetCommitmentHistoryArgs = IDL.Record({
    'descending' : IDL.Opt(IDL.Bool),
    'canister_id' : IDL.Principal,
    'start_after_tx_id' : IDL.Opt(IDL.Nat64),
    'limit' : IDL.Opt(IDL.Nat32),
  });
  const CommitmentSample = IDL.Record({
    'timestamp_nanos' : IDL.Opt(IDL.Nat64),
    'tx_id' : IDL.Nat64,
    'counts_toward_faucet' : IDL.Bool,
    'amount_e8s' : IDL.Nat64,
  });
  const CommitmentHistoryPage = IDL.Record({
    'next_start_after_tx_id' : IDL.Opt(IDL.Nat64),
    'items' : IDL.Vec(CommitmentSample),
  });
  const GetCyclesHistoryArgs = IDL.Record({
    'descending' : IDL.Opt(IDL.Bool),
    'canister_id' : IDL.Principal,
    'limit' : IDL.Opt(IDL.Nat32),
    'start_after_ts' : IDL.Opt(IDL.Nat64),
  });
  const CyclesSample = IDL.Record({
    'timestamp_nanos' : IDL.Nat64,
    'source' : CyclesSampleSource,
    'cycles' : IDL.Nat,
  });
  const CyclesHistoryPage = IDL.Record({
    'next_start_after_ts' : IDL.Opt(IDL.Nat64),
    'items' : IDL.Vec(CyclesSample),
  });
  const GetNeuronCommitmentHistoryArgs = IDL.Record({
    'descending' : IDL.Opt(IDL.Bool),
    'start_after_tx_id' : IDL.Opt(IDL.Nat64),
    'limit' : IDL.Opt(IDL.Nat32),
    'neuron_id' : IDL.Nat64,
  });
  const PublicCounts = IDL.Record({
    'qualifying_commitment_count' : IDL.Nat64,
    'declared_neuron_count' : IDL.Opt(IDL.Nat64),
    'tracked_canister_count' : IDL.Nat64,
    'memo_registered_canister_count' : IDL.Nat64,
    'relay_target_canister_count' : IDL.Nat64,
    'sns_discovered_canister_count' : IDL.Nat64,
    'relay_instance_canister_count' : IDL.Nat64,
    'total_rewards_e8s' : IDL.Nat64,
    'total_output_e8s' : IDL.Nat64,
    'raw_icp_declared_canister_count' : IDL.Opt(IDL.Nat64),
  });
  const CommitmentIndexFault = IDL.Record({
    'offending_tx_id' : IDL.Nat64,
    'observed_at_ts' : IDL.Nat64,
    'message' : IDL.Text,
    'last_cursor_tx_id' : IDL.Opt(IDL.Nat64),
  });
  const IcpXdrRateSnapshot = IDL.Record({
    'decimals' : IDL.Nat32,
    'fetched_at_ts' : IDL.Nat64,
    'rate' : IDL.Nat64,
    'timestamp' : IDL.Nat64,
  });
  const PublicStatus = IDL.Record({
    'stable_memory_bytes' : IDL.Opt(IDL.Nat64),
    'last_icp_xdr_rate_error' : IDL.Opt(IDL.Text),
    'index_interval_seconds' : IDL.Nat64,
    'cmc_canister_id' : IDL.Opt(IDL.Principal),
    'last_completed_cycles_sweep_ts' : IDL.Opt(IDL.Nat64),
    'relay_setup_min_e8s' : IDL.Opt(IDL.Nat64),
    'relay_factory_enabled' : IDL.Opt(IDL.Bool),
    'staking_account' : Account,
    'heap_memory_bytes' : IDL.Opt(IDL.Nat64),
    'rewards_account' : IDL.Opt(Account),
    'output_source_account' : IDL.Opt(Account),
    'cycles_interval_seconds' : IDL.Nat64,
    'index_canister_id' : IDL.Opt(IDL.Principal),
    'faucet_canister_id' : IDL.Principal,
    'commitment_index_fault' : IDL.Opt(CommitmentIndexFault),
    'ledger_canister_id' : IDL.Principal,
    'output_account' : IDL.Opt(Account),
    'icp_xdr_rate' : IDL.Opt(IcpXdrRateSnapshot),
    'total_memory_bytes' : IDL.Opt(IDL.Nat64),
    'last_index_run_ts' : IDL.Opt(IDL.Nat64),
  });
  const RelayTargetSetArgs = IDL.Record({
    'target_canister_ids' : IDL.Vec(IDL.Principal),
  });
  const RelayCreationPhase = IDL.Variant({
    'RelayFunded' : IDL.Null,
    'Reserved' : IDL.Null,
    'CmcTransferPrepared' : IDL.Null,
    'CmcTransferAccepted' : IDL.Null,
    'RelayFundingPrepared' : IDL.Null,
    'HandoffAttempted' : IDL.Null,
    'ProbingTargets' : IDL.Null,
    'ChildCreated' : IDL.Null,
    'CreateDispatched' : IDL.Null,
    'CmcNotifySucceeded' : IDL.Null,
    'CodeInstalled' : IDL.Null,
  });
  const RelaySetupState = IDL.Variant({
    'ManualRecoveryRequired' : IDL.Record({
      'relay_canister_id' : IDL.Opt(IDL.Principal),
      'message' : IDL.Text,
      'phase' : RelayCreationPhase,
    }),
    'Active' : IDL.Record({ 'relay_canister_id' : IDL.Principal }),
    'NotFunded' : IDL.Null,
    'InProgress' : IDL.Record({
      'relay_canister_id' : IDL.Opt(IDL.Principal),
      'phase' : RelayCreationPhase,
    }),
  });
  const RelaySetupView = IDL.Record({
    'setup_key_identifier' : IDL.Text,
    'canonical_target_canister_ids' : IDL.Vec(IDL.Principal),
    'state' : RelaySetupState,
    'nominal_minimum_e8s' : IDL.Nat64,
    'singleton_nominal_minimum_e8s' : IDL.Nat64,
    'total_extra_target_charge_e8s' : IDL.Nat64,
    'setup_account' : IDL.Opt(Account),
    'extra_target_count' : IDL.Nat64,
    'factory_available' : IDL.Bool,
    'target_count' : IDL.Nat32,
    'setup_account_identifier' : IDL.Opt(IDL.Text),
    'extra_target_unit_charge_e8s' : IDL.Nat64,
  });
  const RelaySetupViewResult = IDL.Variant({
    'Ok' : RelaySetupView,
    'Err' : IDL.Text,
  });
  const ListCanistersArgs = IDL.Record({
    'tracking_reason_filter' : IDL.Opt(CanisterTrackingReason),
    'start_after' : IDL.Opt(IDL.Principal),
    'limit' : IDL.Opt(IDL.Nat32),
  });
  const CanisterListItem = IDL.Record({
    'canister_id' : IDL.Principal,
    'tracking_reasons' : IDL.Vec(CanisterTrackingReason),
  });
  const ListCanistersResponse = IDL.Record({
    'items' : IDL.Vec(CanisterListItem),
    'next_start_after' : IDL.Opt(IDL.Principal),
  });
  const ListMemoRegisteredCanisterSummariesArgs = IDL.Record({
    'page_size' : IDL.Opt(IDL.Nat32),
    'page' : IDL.Opt(IDL.Nat32),
  });
  const MemoRegisteredCanisterSummary = IDL.Record({
    'last_cycles_probe_ts' : IDL.Opt(IDL.Nat64),
    'latest_cycles' : IDL.Opt(IDL.Nat),
    'total_qualifying_committed_e8s' : IDL.Nat64,
    'qualifying_commitment_count' : IDL.Nat64,
    'canister_id' : IDL.Principal,
    'tracking_reasons' : IDL.Vec(CanisterTrackingReason),
    'last_commitment_ts' : IDL.Opt(IDL.Nat64),
  });
  const ListMemoRegisteredCanisterSummariesResponse = IDL.Record({
    'page_size' : IDL.Nat32,
    'total' : IDL.Nat64,
    'page' : IDL.Nat32,
    'items' : IDL.Vec(MemoRegisteredCanisterSummary),
  });
  const ListRecentCommitmentsArgs = IDL.Record({
    'limit' : IDL.Opt(IDL.Nat32),
    'qualifying_only' : IDL.Opt(IDL.Bool),
  });
  const RecentCommitmentOutcomeCategory = IDL.Variant({
    'UnderThresholdCommitment' : IDL.Null,
    'QualifyingCommitment' : IDL.Null,
    'InvalidTargetMemo' : IDL.Null,
  });
  const RecentCommitmentListItem = IDL.Record({
    'timestamp_nanos' : IDL.Opt(IDL.Nat64),
    'tx_id' : IDL.Nat64,
    'neuron_memo_text' : IDL.Opt(IDL.Text),
    'canister_id' : IDL.Opt(IDL.Principal),
    'counts_toward_faucet' : IDL.Bool,
    'memo_text' : IDL.Opt(IDL.Text),
    'amount_e8s' : IDL.Nat64,
    'raw_icp_memo_text' : IDL.Opt(IDL.Text),
    'outcome_category' : RecentCommitmentOutcomeCategory,
    'neuron_id' : IDL.Opt(IDL.Nat64),
  });
  const ListRecentCommitmentsResponse = IDL.Record({
    'items' : IDL.Vec(RecentCommitmentListItem),
  });
  const RelaySetupNotifyResult = IDL.Variant({
    'ManualRecoveryRequired' : IDL.Record({
      'relay_canister_id' : IDL.Opt(IDL.Principal),
      'message' : IDL.Text,
      'phase' : RelayCreationPhase,
    }),
    'BelowMinimum' : IDL.Record({
      'shortfall_e8s' : IDL.Nat64,
      'required_e8s' : IDL.Nat64,
      'balance_e8s' : IDL.Nat64,
    }),
    'FailedPreSpend' : IDL.Record({ 'message' : IDL.Text }),
    'Busy' : IDL.Null,
    'Active' : IDL.Record({ 'relay_canister_id' : IDL.Principal }),
    'BelowCurrentRequirement' : IDL.Record({
      'shortfall_e8s' : IDL.Nat64,
      'required_e8s' : IDL.Nat64,
      'balance_e8s' : IDL.Nat64,
    }),
    'InProgress' : IDL.Record({
      'relay_canister_id' : IDL.Opt(IDL.Principal),
      'phase' : RelayCreationPhase,
    }),
  });
  return IDL.Service({
    'find_canisters_by_memo_prefix' : IDL.Func(
        [FindCanistersByMemoPrefixArgs],
        [FindCanistersByMemoPrefixResponse],
        ['query'],
      ),
    'get_canister_module_hashes' : IDL.Func(
        [],
        [IDL.Vec(CanisterModuleHash)],
        ['query'],
      ),
    'get_canister_overview' : IDL.Func(
        [IDL.Principal],
        [IDL.Opt(CanisterOverview)],
        ['query'],
      ),
    'get_commitment_history' : IDL.Func(
        [GetCommitmentHistoryArgs],
        [CommitmentHistoryPage],
        ['query'],
      ),
    'get_cycles_history' : IDL.Func(
        [GetCyclesHistoryArgs],
        [CyclesHistoryPage],
        ['query'],
      ),
    'get_neuron_commitment_history' : IDL.Func(
        [GetNeuronCommitmentHistoryArgs],
        [CommitmentHistoryPage],
        ['query'],
      ),
    'get_public_counts' : IDL.Func([], [PublicCounts], ['query']),
    'get_public_status' : IDL.Func([], [PublicStatus], ['query']),
    'get_raw_icp_commitment_history' : IDL.Func(
        [GetCommitmentHistoryArgs],
        [CommitmentHistoryPage],
        ['query'],
      ),
    'get_relay_setup_view' : IDL.Func(
        [RelayTargetSetArgs],
        [RelaySetupViewResult],
        ['query'],
      ),
    'list_canisters' : IDL.Func(
        [ListCanistersArgs],
        [ListCanistersResponse],
        ['query'],
      ),
    'list_memo_registered_canister_summaries' : IDL.Func(
        [ListMemoRegisteredCanisterSummariesArgs],
        [ListMemoRegisteredCanisterSummariesResponse],
        ['query'],
      ),
    'list_recent_commitments' : IDL.Func(
        [ListRecentCommitmentsArgs],
        [ListRecentCommitmentsResponse],
        ['query'],
      ),
    'notify_relay_setup' : IDL.Func(
        [RelayTargetSetArgs],
        [RelaySetupNotifyResult],
        [],
      ),
  });
};
export const init = ({ IDL }) => {
  const Account = IDL.Record({
    'owner' : IDL.Principal,
    'subaccount' : IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const InitArgs = IDL.Record({
    'canonical_relay_canister_id' : IDL.Opt(IDL.Principal),
    'io_surplus_neuron_id' : IDL.Opt(IDL.Nat64),
    'min_tx_e8s' : IDL.Opt(IDL.Nat64),
    'cmc_canister_id' : IDL.Opt(IDL.Principal),
    'xrc_canister_id' : IDL.Opt(IDL.Principal),
    'relay_cycle_safety_margin_e8s' : IDL.Opt(IDL.Nat64),
    'relay_setup_min_e8s' : IDL.Opt(IDL.Nat64),
    'relay_factory_enabled' : IDL.Opt(IDL.Bool),
    'staking_account' : Account,
    'max_canisters_per_cycles_tick' : IDL.Opt(IDL.Nat32),
    'self_service_relay_interval_seconds' : IDL.Opt(IDL.Nat64),
    'enable_sns_tracking' : IDL.Opt(IDL.Bool),
    'relay_initial_cycles' : IDL.Opt(IDL.Nat),
    'relay_min_subaccount_one_seed_e8s' : IDL.Opt(IDL.Nat64),
    'max_index_pages_per_tick' : IDL.Opt(IDL.Nat32),
    'rewards_account' : IDL.Opt(Account),
    'max_commitment_entries_per_canister' : IDL.Opt(IDL.Nat32),
    'output_source_account' : IDL.Opt(Account),
    'cycles_interval_seconds' : IDL.Opt(IDL.Nat64),
    'max_cycles_entries_per_canister' : IDL.Opt(IDL.Nat32),
    'index_canister_id' : IDL.Opt(IDL.Principal),
    'faucet_canister_id' : IDL.Opt(IDL.Principal),
    'ledger_canister_id' : IDL.Opt(IDL.Principal),
    'output_account' : IDL.Opt(Account),
    'canonical_relay_targets' : IDL.Opt(IDL.Vec(IDL.Principal)),
    'scan_interval_seconds' : IDL.Opt(IDL.Nat64),
    'sns_wasm_canister_id' : IDL.Opt(IDL.Principal),
  });
  return [InitArgs];
};
