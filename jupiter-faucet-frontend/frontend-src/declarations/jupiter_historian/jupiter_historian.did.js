export const idlFactory = ({ IDL }) => {
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const CanisterSource = IDL.Variant({
    MemoCommitment: IDL.Null,
    SnsDiscovery: IDL.Null,
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
    source_filter: IDL.Opt(CanisterSource),
  });
  const CanisterListItem = IDL.Record({
    canister_id: IDL.Principal,
    sources: IDL.Vec(CanisterSource),
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
    sources: IDL.Vec(CanisterSource),
    meta: CanisterMeta,
    cycles_points: IDL.Nat32,
    commitment_points: IDL.Nat32,
  });
  const PublicCounts = IDL.Record({
    registered_canister_count: IDL.Nat64,
    qualifying_commitment_count: IDL.Nat64,
    sns_discovered_canister_count: IDL.Nat64,
    total_output_e8s: IDL.Nat64,
    total_rewards_e8s: IDL.Nat64,
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
  });
  const ListRegisteredCanisterSummariesArgs = IDL.Record({
    page: IDL.Opt(IDL.Nat32),
    page_size: IDL.Opt(IDL.Nat32),
  });
  const RegisteredCanisterSummary = IDL.Record({
    canister_id: IDL.Principal,
    sources: IDL.Vec(CanisterSource),
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
  });
  return IDL.Service({
    get_canister_module_hashes: IDL.Func([], [IDL.Vec(CanisterModuleHash)], []),
    list_canisters: IDL.Func([ListCanistersArgs], [ListCanistersResponse], ['query']),
    get_cycles_history: IDL.Func([GetCyclesHistoryArgs], [CyclesHistoryPage], ['query']),
    get_commitment_history: IDL.Func([GetCommitmentHistoryArgs], [CommitmentHistoryPage], ['query']),
    get_canister_overview: IDL.Func([IDL.Principal], [IDL.Opt(CanisterOverview)], ['query']),
    get_public_counts: IDL.Func([], [PublicCounts], ['query']),
    get_public_status: IDL.Func([], [PublicStatus], ['query']),
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
    blackhole_canister_id: IDL.Opt(IDL.Principal),
    sns_wasm_canister_id: IDL.Opt(IDL.Principal),
    enable_sns_tracking: IDL.Opt(IDL.Bool),
    scan_interval_seconds: IDL.Opt(IDL.Nat64),
    cycles_interval_seconds: IDL.Opt(IDL.Nat64),
    min_tx_e8s: IDL.Opt(IDL.Nat64),
    max_cycles_entries_per_canister: IDL.Opt(IDL.Nat32),
    max_commitment_entries_per_canister: IDL.Opt(IDL.Nat32),
    max_index_pages_per_tick: IDL.Opt(IDL.Nat32),
    max_canisters_per_cycles_tick: IDL.Opt(IDL.Nat32),
  });
  return [InitArgs];
};
