export const idlFactory = ({ IDL }) => {
  const CanisterSource = IDL.Variant({
    MemoContribution: IDL.Null,
    SnsDiscovery: IDL.Null,
  });
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const PublicCounts = IDL.Record({
    registered_canister_count: IDL.Nat64,
    qualifying_contribution_count: IDL.Nat64,
    icp_burned_e8s: IDL.Nat64,
    sns_discovered_canister_count: IDL.Nat64,
  });
  const PublicStatus = IDL.Record({
    staking_account: Account,
    ledger_canister_id: IDL.Principal,
    last_index_run_ts: IDL.Opt(IDL.Nat64),
    index_interval_seconds: IDL.Nat64,
    last_completed_cycles_sweep_ts: IDL.Opt(IDL.Nat64),
    cycles_interval_seconds: IDL.Nat64,
  });
  const RegisteredCanisterSummarySort = IDL.Variant({
    CanisterIdAsc: IDL.Null,
    LastContributionDesc: IDL.Null,
    QualifyingContributionCountDesc: IDL.Null,
    TotalQualifyingContributedDesc: IDL.Null,
  });
  const ListRegisteredCanisterSummariesArgs = IDL.Record({
    page: IDL.Opt(IDL.Nat32),
    page_size: IDL.Opt(IDL.Nat32),
    sort: IDL.Opt(RegisteredCanisterSummarySort),
  });
  const RegisteredCanisterSummary = IDL.Record({
    canister_id: IDL.Principal,
    sources: IDL.Vec(CanisterSource),
    qualifying_contribution_count: IDL.Nat64,
    total_qualifying_contributed_e8s: IDL.Nat64,
    last_contribution_ts: IDL.Opt(IDL.Nat64),
    latest_cycles: IDL.Opt(IDL.Nat),
    last_cycles_probe_ts: IDL.Opt(IDL.Nat64),
  });
  const ListRegisteredCanisterSummariesResponse = IDL.Record({
    items: IDL.Vec(RegisteredCanisterSummary),
    page: IDL.Nat32,
    page_size: IDL.Nat32,
    total: IDL.Nat64,
  });
  const ListRecentContributionsArgs = IDL.Record({
    limit: IDL.Opt(IDL.Nat32),
    qualifying_only: IDL.Opt(IDL.Bool),
  });
  const RecentContributionListItem = IDL.Record({
    canister_id: IDL.Opt(IDL.Principal),
    memo_text: IDL.Opt(IDL.Text),
    tx_id: IDL.Nat64,
    timestamp_nanos: IDL.Opt(IDL.Nat64),
    amount_e8s: IDL.Nat64,
    counts_toward_faucet: IDL.Bool,
  });
  const ListRecentContributionsResponse = IDL.Record({
    items: IDL.Vec(RecentContributionListItem),
  });
  const ListRecentBurnsArgs = IDL.Record({
    limit: IDL.Opt(IDL.Nat32),
  });
  const RecentBurnListItem = IDL.Record({
    canister_id: IDL.Principal,
    tx_id: IDL.Nat64,
    timestamp_nanos: IDL.Opt(IDL.Nat64),
    amount_e8s: IDL.Nat64,
  });
  const ListRecentBurnsResponse = IDL.Record({
    items: IDL.Vec(RecentBurnListItem),
  });
  return IDL.Service({
    get_public_counts: IDL.Func([], [PublicCounts], ['query']),
    get_public_status: IDL.Func([], [PublicStatus], ['query']),
    list_registered_canister_summaries: IDL.Func(
      [ListRegisteredCanisterSummariesArgs],
      [ListRegisteredCanisterSummariesResponse],
      ['query'],
    ),
    list_recent_contributions: IDL.Func(
      [ListRecentContributionsArgs],
      [ListRecentContributionsResponse],
      ['query'],
    ),
    list_recent_burns: IDL.Func([ListRecentBurnsArgs], [ListRecentBurnsResponse], ['query']),
  });
};
export const init = ({ IDL }) => {
  const Account = IDL.Record({
    owner: IDL.Principal,
    subaccount: IDL.Opt(IDL.Vec(IDL.Nat8)),
  });
  const InitArgs = IDL.Record({
    staking_account: Account,
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
    max_contribution_entries_per_canister: IDL.Opt(IDL.Nat32),
    max_index_pages_per_tick: IDL.Opt(IDL.Nat32),
    max_canisters_per_cycles_tick: IDL.Opt(IDL.Nat32),
  });
  return [InitArgs];
};
