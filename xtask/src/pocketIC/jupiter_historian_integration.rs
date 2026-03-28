use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};
use sha2::{Digest, Sha224};

#[path = "real_blackhole.rs"]
mod real_blackhole;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

fn require_ignored_flag() -> Result<()> { Ok(()) }
fn repo_root() -> &'static str { env!("CARGO_MANIFEST_DIR") }

fn build_wasm_cached(cache: &OnceLock<Vec<u8>>, package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    if let Some(bytes) = cache.get() {
        return Ok(bytes.clone());
    }
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--target", "wasm32-unknown-unknown", "--release", "-p", package, "--locked"])
        .current_dir(format!("{}/..", repo_root()));
    if let Some(f) = features {
        cmd.args(["--features", f]);
    }
    let status = cmd.status().with_context(|| format!("failed to build {package}"))?;
    if !status.success() {
        bail!("cargo build failed for {package}");
    }
    let raw_name = package.replace('-', "_");
    let path = format!("{}/../target/wasm32-unknown-unknown/release/{raw_name}.wasm", repo_root());
    let bytes = std::fs::read(path).with_context(|| format!("failed to read wasm for {package}"))?;
    let _ = cache.set(bytes.clone());
    Ok(bytes)
}

static INDEX_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_WASM_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static SNS_ROOT_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static HISTORIAN_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn index_wasm() -> Result<Vec<u8>> { build_wasm_cached(&INDEX_WASM, "mock-icp-index", None) }
fn sns_wasm_wasm() -> Result<Vec<u8>> { build_wasm_cached(&SNS_WASM_WASM, "mock-sns-wasm", None) }
fn sns_root_wasm() -> Result<Vec<u8>> { build_wasm_cached(&SNS_ROOT_WASM, "mock-sns-root", None) }
fn historian_wasm() -> Result<Vec<u8>> { build_wasm_cached(&HISTORIAN_WASM, "jupiter-historian", Some("debug_api")) }

fn tick_n(pic: &PocketIc, n: usize) {
    for _ in 0..n { pic.tick(); }
}

fn set_controllers_exact(pic: &PocketIc, canister: Principal, controllers: Vec<Principal>) -> Result<()> {
    let sender = pic
        .get_controllers(canister)
        .first()
        .copied()
        .unwrap_or(Principal::anonymous());
    pic.set_controllers(canister, Some(sender), controllers)
        .map_err(|e| anyhow!("set_controllers reject: {e:?}"))?;
    Ok(())
}

fn update_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(pic: &PocketIc, canister: Principal, sender: Principal, method: &str, arg: A) -> Result<R> {
    let reply = pic.update_call(canister, sender, method, encode_one(arg)?).map_err(|e| anyhow!("update_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

fn update_bytes<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    payload: Vec<u8>,
) -> Result<R> {
    let reply = pic
        .update_call(canister, sender, method, payload)
        .map_err(|e| anyhow!("update_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

fn update_noargs<R: for<'de> Deserialize<'de> + CandidType>(pic: &PocketIc, canister: Principal, sender: Principal, method: &str) -> Result<R> {
    update_one(pic, canister, sender, method, ())
}

fn query_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(pic: &PocketIc, canister: Principal, sender: Principal, method: &str, arg: A) -> Result<R> {
    let reply = pic.query_call(canister, sender, method, encode_one(arg)?).map_err(|e| anyhow!("query_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianInitArg {
    staking_account: Account,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    sns_wasm_canister_id: Option<Principal>,
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_contribution_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct HistorianUpgradeArg {
    enable_sns_tracking: Option<bool>,
    scan_interval_seconds: Option<u64>,
    cycles_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
    max_cycles_entries_per_canister: Option<u32>,
    max_contribution_entries_per_canister: Option<u32>,
    max_index_pages_per_tick: Option<u32>,
    max_canisters_per_cycles_tick: Option<u32>,
    blackhole_canister_id: Option<Principal>,
    sns_wasm_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    faucet_canister_id: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
enum CanisterSource {
    MemoContribution,
    SnsDiscovery,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListCanistersArgs {
    start_after: Option<Principal>,
    limit: Option<u32>,
    source_filter: Option<CanisterSource>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterListItem {
    canister_id: Principal,
    sources: Vec<CanisterSource>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListCanistersResponse {
    items: Vec<CanisterListItem>,
    next_start_after: Option<Principal>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetContributionHistoryArgs {
    canister_id: Principal,
    start_after_tx_id: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ContributionSample {
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ContributionHistoryPage {
    items: Vec<ContributionSample>,
    next_start_after_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum CyclesSampleSource {
    BlackholeStatus,
    SelfCanister,
    SnsRootSummary,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CyclesSample {
    timestamp_nanos: u64,
    cycles: u128,
    source: CyclesSampleSource,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CyclesHistoryPage {
    items: Vec<CyclesSample>,
    next_start_after_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetCyclesHistoryArgs {
    canister_id: Principal,
    start_after_ts: Option<u64>,
    limit: Option<u32>,
    descending: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    distinct_canister_count: u32,
    last_indexed_staking_tx_id: Option<u64>,
    last_sns_discovery_ts: u64,
    last_completed_cycles_sweep_ts: u64,
    active_cycles_sweep_present: bool,
    active_cycles_sweep_next_index: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterStatus {
    cycles: Option<Nat>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SnsCanisterSummary {
    canister_id: Option<Principal>,
    status: Option<SnsCanisterStatus>,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct GetSnsCanistersSummaryResponse {
    root: Option<SnsCanisterSummary>,
    governance: Option<SnsCanisterSummary>,
    ledger: Option<SnsCanisterSummary>,
    swap: Option<SnsCanisterSummary>,
    index: Option<SnsCanisterSummary>,
    dapps: Vec<SnsCanisterSummary>,
    archives: Vec<SnsCanisterSummary>,
}
#[derive(Clone, Debug, CandidType, Deserialize)]
struct PublicCounts {
    registered_canister_count: u64,
    qualifying_contribution_count: u64,
    icp_burned_e8s: u64,
    sns_discovered_canister_count: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct PublicStatus {
    staking_account: Account,
    ledger_canister_id: Principal,
    last_index_run_ts: Option<u64>,
    index_interval_seconds: u64,
    last_completed_cycles_sweep_ts: Option<u64>,
    cycles_interval_seconds: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum RegisteredCanisterSummarySort {
    CanisterIdAsc,
    LastContributionDesc,
    QualifyingContributionCountDesc,
    TotalQualifyingContributedDesc,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListRegisteredCanisterSummariesArgs {
    page: Option<u32>,
    page_size: Option<u32>,
    sort: Option<RegisteredCanisterSummarySort>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RegisteredCanisterSummary {
    canister_id: Principal,
    sources: Vec<CanisterSource>,
    qualifying_contribution_count: u64,
    total_qualifying_contributed_e8s: u64,
    last_contribution_ts: Option<u64>,
    latest_cycles: Option<u128>,
    last_cycles_probe_ts: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListRegisteredCanisterSummariesResponse {
    items: Vec<RegisteredCanisterSummary>,
    page: u32,
    page_size: u32,
    total: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, Default)]
struct ListRecentContributionsArgs {
    limit: Option<u32>,
    qualifying_only: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RecentContributionListItem {
    canister_id: Option<Principal>,
    memo_text: Option<String>,
    tx_id: u64,
    timestamp_nanos: Option<u64>,
    amount_e8s: u64,
    counts_toward_faucet: bool,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ListRecentContributionsResponse {
    items: Vec<RecentContributionListItem>,
}


fn account_identifier_text(account: &Account) -> String {
    let subaccount = account.subaccount.unwrap_or([0u8; 32]);
    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(account.owner.as_slice());
    hasher.update(subaccount);
    let hash = hasher.finalize();
    let checksum = crc32fast::hash(&hash).to_be_bytes();
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&checksum);
    bytes[4..].copy_from_slice(&hash);
    hex::encode(bytes)
}

struct Harness {
    pic: PocketIc,
    index: Principal,
    blackhole: Principal,
    cmc: Principal,
    sns_wasm: Principal,
    historian: Principal,
}

impl Harness {
    fn new(enable_sns_tracking: bool) -> Result<Self> {
        let pic = PocketIcBuilder::new().with_application_subnet().build();
        let index = pic.create_canister();
        let blackhole = pic.create_canister();
        let sns_wasm = pic.create_canister();
        let cmc = pic.create_canister();
        let historian = pic.create_canister();
        for canister in [index, blackhole, sns_wasm, cmc, historian] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(index, index_wasm()?, vec![], None);
        pic.install_canister(blackhole, real_blackhole::real_blackhole_wasm()?, vec![], None);
        set_controllers_exact(&pic, blackhole, vec![blackhole])?;
        pic.install_canister(sns_wasm, sns_wasm_wasm()?, vec![], None);

        let staking_account = Account { owner: Principal::anonymous(), subaccount: Some([9u8; 32]) };
        let init = HistorianInitArg {
            staking_account,
            ledger_canister_id: Some(index),
            index_canister_id: Some(index),
            cmc_canister_id: Some(cmc),
            faucet_canister_id: Some(blackhole),
            blackhole_canister_id: Some(blackhole),
            sns_wasm_canister_id: Some(sns_wasm),
            enable_sns_tracking: Some(enable_sns_tracking),
            scan_interval_seconds: Some(60),
            cycles_interval_seconds: Some(1),
            min_tx_e8s: Some(10),
            max_cycles_entries_per_canister: Some(100),
            max_contribution_entries_per_canister: Some(100),
            max_index_pages_per_tick: Some(10),
            max_canisters_per_cycles_tick: Some(10),
        };
        pic.install_canister(historian, historian_wasm()?, encode_one(init)?, None);
        Ok(Self { pic, index, blackhole, cmc, sns_wasm, historian })
    }

    fn staking_identifier(&self) -> Result<String> {
        let account = Account { owner: Principal::anonymous(), subaccount: Some([9u8; 32]) };
        Ok(account_identifier_text(&account))
    }

    fn tick(&self) {
        self.pic.advance_time(Duration::from_secs(2));
        tick_n(&self.pic, 5);
    }
}

fn principal_to_subaccount(principal: Principal) -> [u8; 32] {
    let bytes = principal.as_slice();
    let mut out = [0u8; 32];
    out[0] = bytes.len() as u8;
    let len = bytes.len().min(31);
    out[1..1 + len].copy_from_slice(&bytes[..len]);
    out
}

#[test]
#[ignore]
fn historian_indexes_contributions_and_blackhole_cycles() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.blackhole;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(&h.pic, h.index, Principal::anonymous(), "debug_append_transfer", encode_args((staking_id, 42u64, Some(target.to_text().into_bytes())))?)?;

    h.tick();
    let _: () = update_noargs(&h.pic, h.historian, Principal::anonymous(), "debug_driver_tick")?;

    let st: DebugState = query_one(&h.pic, h.historian, Principal::anonymous(), "debug_state", ())?;
    assert_eq!(st.distinct_canister_count, 1);
    assert_eq!(st.last_indexed_staking_tx_id, Some(1));

    let canisters: ListCanistersResponse = query_one(&h.pic, h.historian, Principal::anonymous(), "list_canisters", ListCanistersArgs { start_after: None, limit: Some(10), source_filter: None })?;
    assert_eq!(canisters.items.len(), 1);
    assert_eq!(canisters.items[0].canister_id, target);
    assert_eq!(canisters.items[0].sources, vec![CanisterSource::MemoContribution]);

    let contribs: ContributionHistoryPage = query_one(&h.pic, h.historian, Principal::anonymous(), "get_contribution_history", GetContributionHistoryArgs { canister_id: target, start_after_tx_id: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(contribs.items.len(), 1);
    assert_eq!(contribs.items[0].tx_id, 1);
    assert!(contribs.items[0].counts_toward_faucet);

    let cycles: CyclesHistoryPage = query_one(&h.pic, h.historian, Principal::anonymous(), "get_cycles_history", GetCyclesHistoryArgs { canister_id: target, start_after_ts: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(cycles.items.len(), 1);
    assert!(cycles.items[0].cycles > 0);
    assert!(matches!(cycles.items[0].source, CyclesSampleSource::BlackholeStatus));
    Ok(())
}

#[test]
#[ignore]
fn historian_discovers_sns_canisters_and_records_summary_cycles() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(true)?;
    let sns_root = h.pic.create_canister();
    h.pic.add_cycles(sns_root, 5_000_000_000_000);
    h.pic.install_canister(sns_root, sns_root_wasm()?, vec![], None);

    let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
    let dapp = Principal::from_text("2vxsx-fae")?;
    let archive = Principal::from_text("rdmx6-jaaaa-aaaaa-aaadq-cai")?;

    let summary = GetSnsCanistersSummaryResponse {
        root: Some(SnsCanisterSummary { canister_id: Some(sns_root), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(1000u64)) }) }),
        governance: Some(SnsCanisterSummary { canister_id: Some(governance), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(2000u64)) }) }),
        ledger: None,
        swap: None,
        index: None,
        dapps: vec![SnsCanisterSummary { canister_id: Some(dapp), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(3000u64)) }) }],
        archives: vec![SnsCanisterSummary { canister_id: Some(archive), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(4000u64)) }) }],
    };
    let _: () = update_one(&h.pic, sns_root, Principal::anonymous(), "debug_set_summary", summary)?;
    let _: () = update_one(&h.pic, h.sns_wasm, Principal::anonymous(), "debug_set_roots", vec![sns_root])?;

    h.tick();
    let _: () = update_noargs(&h.pic, h.historian, Principal::anonymous(), "debug_driver_tick")?;

    let canisters: ListCanistersResponse = query_one(&h.pic, h.historian, Principal::anonymous(), "list_canisters", ListCanistersArgs { start_after: None, limit: Some(10), source_filter: Some(CanisterSource::SnsDiscovery) })?;
    let ids: Vec<_> = canisters.items.iter().map(|i| i.canister_id).collect();
    assert!(ids.contains(&sns_root));
    assert!(ids.contains(&governance));
    assert!(ids.contains(&dapp));
    assert!(ids.contains(&archive));

    let dapp_cycles: CyclesHistoryPage = query_one(&h.pic, h.historian, Principal::anonymous(), "get_cycles_history", GetCyclesHistoryArgs { canister_id: dapp, start_after_ts: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(dapp_cycles.items.len(), 1);
    assert_eq!(dapp_cycles.items[0].cycles, 3000u128);
    assert!(matches!(dapp_cycles.items[0].source, CyclesSampleSource::SnsRootSummary));
    Ok(())
}

#[test]
#[ignore]
fn historian_upgrade_preserves_histories() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.blackhole;
    let staking_id = h.staking_identifier()?;
    let _: u64 = update_bytes(&h.pic, h.index, Principal::anonymous(), "debug_append_transfer", encode_args((staking_id, 42u64, Some(target.to_text().into_bytes())))?)?;
    h.tick();
    let _: () = update_noargs(&h.pic, h.historian, Principal::anonymous(), "debug_driver_tick")?;

    let upgrade_sender = h.pic.get_controllers(h.historian).first().copied().unwrap_or(h.historian);
    h.pic
        .upgrade_canister(
            h.historian,
            historian_wasm()?,
            encode_one(Option::<HistorianUpgradeArg>::None)?,
            Some(upgrade_sender),
        )
        .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;

    let contribs: ContributionHistoryPage = query_one(&h.pic, h.historian, Principal::anonymous(), "get_contribution_history", GetContributionHistoryArgs { canister_id: target, start_after_tx_id: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(contribs.items.len(), 1);
    let cycles: CyclesHistoryPage = query_one(&h.pic, h.historian, Principal::anonymous(), "get_cycles_history", GetCyclesHistoryArgs { canister_id: target, start_after_ts: None, limit: Some(10), descending: Some(false) })?;
    assert_eq!(cycles.items.len(), 1);
    assert!(cycles.items[0].cycles > 0);
    Ok(())
}


#[test]
#[ignore]
fn historian_public_queries_surface_expected_counts_and_recent_items() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(false)?;
    let target = h.blackhole;
    let staking_id = h.staking_identifier()?;

    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id.clone(), 42u64, Some(target.to_text().into_bytes())))?,
    )?;
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((staking_id, 5u64, Some(target.to_text().into_bytes())))?,
    )?;
    let burn_account = account_identifier_text(&Account { owner: h.cmc, subaccount: Some(principal_to_subaccount(target)) });
    let _: u64 = update_bytes(
        &h.pic,
        h.index,
        Principal::anonymous(),
        "debug_append_transfer",
        encode_args((burn_account, 42u64, Option::<Vec<u8>>::None))?,
    )?;

    h.tick();
    let _: () = update_noargs(&h.pic, h.historian, Principal::anonymous(), "debug_driver_tick")?;

    let counts: PublicCounts = query_one(&h.pic, h.historian, Principal::anonymous(), "get_public_counts", ())?;
    assert_eq!(counts.registered_canister_count, 1);
    assert_eq!(counts.qualifying_contribution_count, 1);
    assert_eq!(counts.icp_burned_e8s, 42);
    assert_eq!(counts.sns_discovered_canister_count, 0);

    let status: PublicStatus = query_one(&h.pic, h.historian, Principal::anonymous(), "get_public_status", ())?;
    assert_eq!(status.staking_account.owner, Principal::anonymous());
    assert_eq!(status.staking_account.subaccount, Some([9u8; 32]));
    assert_eq!(status.ledger_canister_id, h.index);
    assert_eq!(status.index_interval_seconds, 60);
    assert_eq!(status.cycles_interval_seconds, 1);
    assert!(status.last_index_run_ts.is_some());

    let registered: ListRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_registered_canister_summaries",
        ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
            sort: Some(RegisteredCanisterSummarySort::TotalQualifyingContributedDesc),
        },
    )?;
    assert_eq!(registered.total, 1);
    assert_eq!(registered.items.len(), 1);
    assert_eq!(registered.items[0].canister_id, target);
    assert_eq!(registered.items[0].sources, vec![CanisterSource::MemoContribution]);
    assert_eq!(registered.items[0].qualifying_contribution_count, 1);
    assert_eq!(registered.items[0].total_qualifying_contributed_e8s, 42);
    assert!(registered.items[0].last_contribution_ts.is_some());
    assert!(registered.items[0].latest_cycles.unwrap_or_default() > 0);
    assert!(registered.items[0].last_cycles_probe_ts.is_some());

    let recent_all: ListRecentContributionsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_contributions",
        ListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert_eq!(recent_all.items.len(), 2);
    assert_eq!(recent_all.items[0].tx_id, 2);
    assert_eq!(recent_all.items[0].amount_e8s, 5);
    assert!(!recent_all.items[0].counts_toward_faucet);
    assert_eq!(recent_all.items[1].tx_id, 1);
    assert_eq!(recent_all.items[1].amount_e8s, 42);
    assert!(recent_all.items[1].counts_toward_faucet);

    let recent_qualifying: ListRecentContributionsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_contributions",
        ListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(true),
        },
    )?;
    assert_eq!(recent_qualifying.items.len(), 1);
    assert_eq!(recent_qualifying.items[0].tx_id, 1);
    assert_eq!(recent_qualifying.items[0].canister_id, Some(target));
    Ok(())
}

#[test]
#[ignore]
fn historian_public_counts_exclude_sns_only_canisters_from_registered_totals() -> Result<()> {
    require_ignored_flag()?;
    let h = Harness::new(true)?;
    let sns_root = h.pic.create_canister();
    h.pic.add_cycles(sns_root, 5_000_000_000_000);
    h.pic.install_canister(sns_root, sns_root_wasm()?, vec![], None);

    let governance = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
    let dapp = Principal::from_text("2vxsx-fae")?;

    let summary = GetSnsCanistersSummaryResponse {
        root: Some(SnsCanisterSummary { canister_id: Some(sns_root), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(1000u64)) }) }),
        governance: Some(SnsCanisterSummary { canister_id: Some(governance), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(2000u64)) }) }),
        ledger: None,
        swap: None,
        index: None,
        dapps: vec![SnsCanisterSummary { canister_id: Some(dapp), status: Some(SnsCanisterStatus { cycles: Some(Nat::from(3000u64)) }) }],
        archives: vec![],
    };
    let _: () = update_one(&h.pic, sns_root, Principal::anonymous(), "debug_set_summary", summary)?;
    let _: () = update_one(&h.pic, h.sns_wasm, Principal::anonymous(), "debug_set_roots", vec![sns_root])?;

    h.tick();
    let _: () = update_noargs(&h.pic, h.historian, Principal::anonymous(), "debug_driver_tick")?;

    let counts: PublicCounts = query_one(&h.pic, h.historian, Principal::anonymous(), "get_public_counts", ())?;
    assert_eq!(counts.registered_canister_count, 0);
    assert_eq!(counts.qualifying_contribution_count, 0);
    assert_eq!(counts.icp_burned_e8s, 0);
    assert!(counts.sns_discovered_canister_count >= 3);

    let registered: ListRegisteredCanisterSummariesResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_registered_canister_summaries",
        ListRegisteredCanisterSummariesArgs {
            page: Some(0),
            page_size: Some(10),
            sort: Some(RegisteredCanisterSummarySort::CanisterIdAsc),
        },
    )?;
    assert_eq!(registered.total, 0);
    assert!(registered.items.is_empty());

    let recent: ListRecentContributionsResponse = query_one(
        &h.pic,
        h.historian,
        Principal::anonymous(),
        "list_recent_contributions",
        ListRecentContributionsArgs {
            limit: Some(10),
            qualifying_only: Some(false),
        },
    )?;
    assert!(recent.items.is_empty());
    Ok(())
}
