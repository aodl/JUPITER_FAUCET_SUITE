export const idlFactory = ({ IDL }) => {
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const CanisterTrackingReason = IDL.Variant({
    MemoCommitment: IDL.Null,
    SnsDiscovery: IDL.Null,
    RelayTarget: IDL.Null,
    RelayInstance: IDL.Null,
  });
  const CommitmentSample = IDL.Record({
    tx_id: IDL.Nat64,
    timestamp_nanos: IDL.Opt(IDL.Nat64),
    amount_e8s: IDL.Nat64,
    counts_toward_faucet: IDL.Bool,
  });
  const CyclesSampleSource = IDL.Variant({
    BlackholeStatus: IDL.Null,
    SelfCanister: IDL.Null,
    SnsRootSummary: IDL.Null,
  });
  const CyclesSample = IDL.Record({
    timestamp_nanos: IDL.Nat64,
    cycles: IDL.Nat,
    source: CyclesSampleSource,
  });
  const CyclesProbeResult = IDL.Variant({
    Ok: CyclesSampleSource,
    NotAvailable: IDL.Null,
    Error: IDL.Text,
  });
  const CanisterMeta = IDL.Record({
    first_seen_ts: IDL.Opt(IDL.Nat64),
    last_commitment_ts: IDL.Opt(IDL.Nat64),
    last_cycles_probe_ts: IDL.Opt(IDL.Nat64),
    last_cycles_probe_result: IDL.Opt(CyclesProbeResult),
  });
  const CommitmentIndexFault = IDL.Record({
    observed_at_ts: IDL.Nat64,
    last_cursor_tx_id: IDL.Opt(IDL.Nat64),
    offending_tx_id: IDL.Nat64,
    message: IDL.Text,
  });
  const ListCanistersArgs = IDL.Record({
    start_after: IDL.Opt(IDL.Principal),
    limit: IDL.Opt(IDL.Nat32),
    tracking_reason_filter: IDL.Opt(CanisterTrackingReason),
  });
  const CanisterListItem = IDL.Record({
    canister_id: IDL.Principal,
    tracking_reasons: IDL.Vec(CanisterTrackingReason),
  });
  const ListCanistersResponse = IDL.Record({
    items: IDL.Vec(CanisterListItem),
    next_start_after: IDL.Opt(IDL.Principal),
  });
  const GetCyclesHistoryArgs = IDL.Record({
    canister_id: IDL.Principal,
    start_after_ts: IDL.Opt(IDL.Nat64),
    limit: IDL.Opt(IDL.Nat32),
    descending: IDL.Opt(IDL.Bool),
  });
  const CyclesHistoryPage = IDL.Record({
    items: IDL.Vec(CyclesSample),
    next_start_after_ts: IDL.Opt(IDL.Nat64),
  });
  const GetCommitmentHistoryArgs = IDL.Record({
    canister_id: IDL.Principal,
    start_after_tx_id: IDL.Opt(IDL.Nat64),
    limit: IDL.Opt(IDL.Nat32),
    descending: IDL.Opt(IDL.Bool),
  });
  const CommitmentHistoryPage = IDL.Record({
    items: IDL.Vec(CommitmentSample),
    next_start_after_tx_id: IDL.Opt(IDL.Nat64),
  });
  const CanisterOverview = IDL.Record({
    canister_id: IDL.Principal,
    tracking_reasons: IDL.Vec(CanisterTrackingReason),
    meta: CanisterMeta,
    cycles_points: IDL.Nat32,
    commitment_points: IDL.Nat32,
  });
  const PublicCounts = IDL.Record({
    tracked_canister_count: IDL.Nat64,
    memo_registered_canister_count: IDL.Nat64,
    raw_icp_declared_canister_count: IDL.Opt(IDL.Nat64),
    declared_neuron_count: IDL.Opt(IDL.Nat64),
    qualifying_commitment_count: IDL.Nat64,
    sns_discovered_canister_count: IDL.Nat64,
    relay_target_canister_count: IDL.Nat64,
    relay_instance_canister_count: IDL.Nat64,
    total_output_e8s: IDL.Nat64,
    total_rewards_e8s: IDL.Nat64,
  });
  const IcpXdrRateSnapshot = IDL.Record({
    rate: IDL.Nat64,
    decimals: IDL.Nat32,
    timestamp: IDL.Nat64,
    fetched_at_ts: IDL.Nat64,
  });
  const PublicStatus = IDL.Record({
    staking_account: Account,
    ledger_canister_id: IDL.Principal,
    faucet_canister_id: IDL.Principal,
    cmc_canister_id: IDL.Opt(IDL.Principal),
    output_source_account: IDL.Opt(Account),
    output_account: IDL.Opt(Account),
    rewards_account: IDL.Opt(Account),
    index_canister_id: IDL.Opt(IDL.Principal),
    last_index_run_ts: IDL.Opt(IDL.Nat64),
    index_interval_seconds: IDL.Nat64,
    last_completed_cycles_sweep_ts: IDL.Opt(IDL.Nat64),
    cycles_interval_seconds: IDL.Nat64,
    heap_memory_bytes: IDL.Opt(IDL.Nat64),
    stable_memory_bytes: IDL.Opt(IDL.Nat64),
    total_memory_bytes: IDL.Opt(IDL.Nat64),
    commitment_index_fault: IDL.Opt(CommitmentIndexFault),
    icp_xdr_rate: IDL.Opt(IcpXdrRateSnapshot),
    last_icp_xdr_rate_error: IDL.Opt(IDL.Text),
    relay_factory_enabled: IDL.Opt(IDL.Bool),
    relay_setup_min_e8s: IDL.Opt(IDL.Nat64),
    relay_setup_dust_e8s: IDL.Opt(IDL.Nat64),
  });
  const RelayRegistryKind = IDL.Variant({
    Canonical: IDL.Null,
    SelfService: IDL.Null,
  });
  const RelayRegistration = IDL.Record({
    target_canister_id: IDL.Principal,
    relay_canister_id: IDL.Principal,
    kind: RelayRegistryKind,
    created_at_ts: IDL.Opt(IDL.Nat64),
  });
  const RelaySetupPublicStatus = IDL.Variant({
    NotFunded: IDL.Null,
    BelowMinimum: IDL.Null,
    PaymentNotAllowed: IDL.Null,
    IndexNotReady: IDL.Null,
    Pending: IDL.Null,
    CreatingRelay: IDL.Null,
    Active: IDL.Null,
    SweepingToExistingRelay: IDL.Null,
    Refunding: IDL.Null,
    Refunded: IDL.Null,
    FailedRetryable: IDL.Null,
    ManualRecoveryRequired: IDL.Null,
  });
  const GetRelaySetupViewArgs = IDL.Record({
    target_canister_id: IDL.Principal,
  });
  const RelaySetupView = IDL.Record({
    target_canister_id: IDL.Principal,
    setup_account: Account,
    setup_account_identifier: IDL.Text,
    minimum_e8s: IDL.Nat64,
    current_required_e8s: IDL.Opt(IDL.Nat64),
    nominal_minimum_e8s: IDL.Nat64,
    payment_allowed: IDL.Bool,
    payment_blocked_reason: IDL.Opt(IDL.Text),
    existing_relay: IDL.Opt(RelayRegistration),
    status: RelaySetupPublicStatus,
    factory_available: IDL.Bool,
    warning_text: IDL.Opt(IDL.Text),
  });
  const GetRelaySetupRecoveryViewArgs = IDL.Record({
    target_canister_id: IDL.Principal,
  });
  const RelaySetupTransferKind = IDL.Variant({
    CmcConversion: IDL.Null,
    RelayFunding: IDL.Null,
    ExistingRelaySweep: IDL.Null,
    Refund: IDL.Null,
  });
  const RedactedTransferRecord = IDL.Record({
    kind: RelaySetupTransferKind,
    from_account_identifier: IDL.Text,
    to_account_identifier: IDL.Text,
    amount_e8s: IDL.Nat64,
    fee_e8s: IDL.Nat64,
    created_at_time_nanos: IDL.Nat64,
    block_index: IDL.Opt(IDL.Nat64),
    completed: IDL.Bool,
  });
  const RelayCreateAttemptView = IDL.Record({
    target_canister_id: IDL.Principal,
    created_at_ts: IDL.Nat64,
    initial_cycles: IDL.Nat,
    create_attach_cycles: IDL.Nat,
  });
  const RelaySetupRecoveryView = IDL.Record({
    target_canister_id: IDL.Principal,
    status: RelaySetupPublicStatus,
    last_error: IDL.Opt(IDL.Text),
    relay_canister_id: IDL.Opt(IDL.Principal),
    setup_account_identifier: IDL.Text,
    setup_amount_seen_e8s: IDL.Nat64,
    setup_amount_processed_e8s: IDL.Nat64,
    cycle_conversion_e8s: IDL.Opt(IDL.Nat64),
    cycles_minted: IDL.Opt(IDL.Nat),
    configured_relay_create_attach_cycles: IDL.Nat,
    relay_onchain_module_hash_hex: IDL.Opt(IDL.Text),
    cycle_transfer: IDL.Opt(RedactedTransferRecord),
    relay_funding_transfer: IDL.Opt(RedactedTransferRecord),
    existing_relay_sweep_transfer: IDL.Opt(RedactedTransferRecord),
    refund_transfer_count: IDL.Nat32,
    relay_create_attempt: IDL.Opt(RelayCreateAttemptView),
    created_at_ts: IDL.Nat64,
    updated_at_ts: IDL.Nat64,
  });
  const ListRelayRegistrationsArgs = IDL.Record({
    start_after: IDL.Opt(IDL.Principal),
    limit: IDL.Opt(IDL.Nat32),
  });
  const ListRelayRegistrationsResponse = IDL.Record({
    items: IDL.Vec(RelayRegistration),
    next_start_after: IDL.Opt(IDL.Principal),
  });
  const RelaySetupNotifyResult = IDL.Variant({
    BelowMinimum: IDL.Record({ minimum_e8s: IDL.Nat64, current_balance_e8s: IDL.Nat64 }),
    InsufficientForCurrentRate: IDL.Record({ required_e8s: IDL.Nat64, current_balance_e8s: IDL.Nat64 }),
    TargetNotObservable: IDL.Record({ message: IDL.Text }),
    Pending: IDL.Record({ status: RelaySetupPublicStatus }),
    Active: IDL.Record({ relay: RelayRegistration }),
    SweptToExistingRelay: IDL.Record({ relay: RelayRegistration, amount_e8s: IDL.Nat64, block_index: IDL.Nat64 }),
    SweepBelowDust: IDL.Record({ relay: RelayRegistration, current_balance_e8s: IDL.Nat64 }),
    Refunded: IDL.Record({ blocks: IDL.Vec(IDL.Nat64) }),
    RefundPending: IDL.Record({ reason: IDL.Text }),
    Failed: IDL.Record({ status: RelaySetupPublicStatus, message: IDL.Text }),
  });
  const ListRegisteredCanisterSummariesArgs = IDL.Record({
    page: IDL.Opt(IDL.Nat32),
    page_size: IDL.Opt(IDL.Nat32),
  });
  const RegisteredCanisterSummary = IDL.Record({
    canister_id: IDL.Principal,
    tracking_reasons: IDL.Vec(CanisterTrackingReason),
    qualifying_commitment_count: IDL.Nat64,
    total_qualifying_committed_e8s: IDL.Nat64,
    last_commitment_ts: IDL.Opt(IDL.Nat64),
    latest_cycles: IDL.Opt(IDL.Nat),
    last_cycles_probe_ts: IDL.Opt(IDL.Nat64),
  });
  const ListRegisteredCanisterSummariesResponse = IDL.Record({
    items: IDL.Vec(RegisteredCanisterSummary),
    page: IDL.Nat32,
    page_size: IDL.Nat32,
    total: IDL.Nat64,
  });
  const FindCanistersByMemoPrefixArgs = IDL.Record({
    prefix: IDL.Text,
    limit: IDL.Opt(IDL.Nat32),
    tracking_reason_filter: IDL.Opt(CanisterTrackingReason),
  });
  const CanisterPrefixMatch = IDL.Record({
    canister_id: IDL.Principal,
    tracking_reasons: IDL.Vec(CanisterTrackingReason),
    matched_prefix: IDL.Text,
    qualifying_commitment_count: IDL.Nat64,
    total_qualifying_committed_e8s: IDL.Nat64,
    last_commitment_ts: IDL.Opt(IDL.Nat64),
    latest_cycles: IDL.Opt(IDL.Nat),
    last_cycles_probe_ts: IDL.Opt(IDL.Nat64),
  });
  const FindCanistersByMemoPrefixResponse = IDL.Record({
    items: IDL.Vec(CanisterPrefixMatch),
    truncated: IDL.Bool,
  });
  const ListRecentCommitmentsArgs = IDL.Record({
    limit: IDL.Opt(IDL.Nat32),
    qualifying_only: IDL.Opt(IDL.Bool),
  });
  const RecentCommitmentOutcomeCategory = IDL.Variant({
    QualifyingCommitment: IDL.Null,
    UnderThresholdCommitment: IDL.Null,
    InvalidTargetMemo: IDL.Null,
  });
  const RecentCommitmentListItem = IDL.Record({
    canister_id: IDL.Opt(IDL.Principal),
    neuron_id: IDL.Opt(IDL.Nat64),
    raw_icp_memo_text: IDL.Opt(IDL.Text),
    neuron_memo_text: IDL.Opt(IDL.Text),
    memo_text: IDL.Opt(IDL.Text),
    tx_id: IDL.Nat64,
    timestamp_nanos: IDL.Opt(IDL.Nat64),
    amount_e8s: IDL.Nat64,
    counts_toward_faucet: IDL.Bool,
    outcome_category: RecentCommitmentOutcomeCategory,
  });
  const ListRecentCommitmentsResponse = IDL.Record({
    items: IDL.Vec(RecentCommitmentListItem),
  });
  const CanisterModuleHash = IDL.Record({
    canister_id: IDL.Principal,
    module_hash_hex: IDL.Opt(IDL.Text),
    controllers: IDL.Opt(IDL.Vec(IDL.Principal)),
    heap_memory_bytes: IDL.Opt(IDL.Nat64),
    stable_memory_bytes: IDL.Opt(IDL.Nat64),
    total_memory_bytes: IDL.Opt(IDL.Nat64),
  });
  return IDL.Service({
    get_canister_module_hashes: IDL.Func([], [IDL.Vec(CanisterModuleHash)], ['query']),
    list_canisters: IDL.Func([ListCanistersArgs], [ListCanistersResponse], ['query']),
    get_cycles_history: IDL.Func([GetCyclesHistoryArgs], [CyclesHistoryPage], ['query']),
    get_commitment_history: IDL.Func([GetCommitmentHistoryArgs], [CommitmentHistoryPage], ['query']),
    get_canister_overview: IDL.Func([IDL.Principal], [IDL.Opt(CanisterOverview)], ['query']),
    get_public_counts: IDL.Func([], [PublicCounts], ['query']),
    get_public_status: IDL.Func([], [PublicStatus], ['query']),
    get_relay_setup_view: IDL.Func([GetRelaySetupViewArgs], [RelaySetupView], ['query']),
    get_relay_setup_recovery_view: IDL.Func(
      [GetRelaySetupRecoveryViewArgs],
      [RelaySetupRecoveryView],
      ['query'],
    ),
    list_relay_registrations: IDL.Func([ListRelayRegistrationsArgs], [ListRelayRegistrationsResponse], ['query']),
    notify_relay_setup: IDL.Func([IDL.Principal], [RelaySetupNotifyResult], []),
    find_canisters_by_memo_prefix: IDL.Func(
      [FindCanistersByMemoPrefixArgs],
      [FindCanistersByMemoPrefixResponse],
      ['query'],
    ),
    list_registered_canister_summaries: IDL.Func(
      [ListRegisteredCanisterSummariesArgs],
      [ListRegisteredCanisterSummariesResponse],
      ['query'],
    ),
    list_recent_commitments: IDL.Func(
      [ListRecentCommitmentsArgs],
      [ListRecentCommitmentsResponse],
      ['query'],
    ),
  });
};
export const init = ({ IDL }) => {
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const InitArgs = IDL.Record({
    staking_account: Account,
    output_source_account: IDL.Opt(Account),
    output_account: IDL.Opt(Account),
    rewards_account: IDL.Opt(Account),
    ledger_canister_id: IDL.Opt(IDL.Principal),
    index_canister_id: IDL.Opt(IDL.Principal),
    cmc_canister_id: IDL.Opt(IDL.Principal),
    faucet_canister_id: IDL.Opt(IDL.Principal),
    sns_wasm_canister_id: IDL.Opt(IDL.Principal),
    xrc_canister_id: IDL.Opt(IDL.Principal),
    enable_sns_tracking: IDL.Opt(IDL.Bool),
    scan_interval_seconds: IDL.Opt(IDL.Nat64),
    cycles_interval_seconds: IDL.Opt(IDL.Nat64),
    min_tx_e8s: IDL.Opt(IDL.Nat64),
    max_cycles_entries_per_canister: IDL.Opt(IDL.Nat32),
    max_commitment_entries_per_canister: IDL.Opt(IDL.Nat32),
    max_index_pages_per_tick: IDL.Opt(IDL.Nat32),
    max_canisters_per_cycles_tick: IDL.Opt(IDL.Nat32),
    relay_factory_enabled: IDL.Opt(IDL.Bool),
    relay_setup_min_e8s: IDL.Opt(IDL.Nat64),
    relay_setup_dust_e8s: IDL.Opt(IDL.Nat64),
    relay_setup_refund_cooldown_seconds: IDL.Opt(IDL.Nat64),
    relay_initial_cycles: IDL.Opt(IDL.Nat),
    relay_cycle_safety_margin_e8s: IDL.Opt(IDL.Nat64),
    relay_min_subaccount_one_seed_e8s: IDL.Opt(IDL.Nat64),
    self_service_relay_interval_seconds: IDL.Opt(IDL.Nat64),
    self_service_relay_max_transfers_per_tick: IDL.Opt(IDL.Nat32),
    io_surplus_neuron_id: IDL.Opt(IDL.Nat64),
    canonical_relay_canister_id: IDL.Opt(IDL.Principal),
    canonical_relay_targets: IDL.Opt(IDL.Vec(IDL.Principal)),
  });
  return [InitArgs];
};
