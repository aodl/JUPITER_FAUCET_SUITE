use anyhow::{anyhow, bail, Context, Result};
use candid::{decode_one, encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};
use sha2::{Digest, Sha224};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

fn require_ignored_flag() -> Result<()> {
    // These PocketIC suites are intentionally #[ignore] so a plain cargo test stays fast.
    // The supported repository entry points (for example `cargo run -p xtask -- test_all`)
    // invoke them explicitly with `--ignored`.
    Ok(())
}

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
}

fn build_wasm_cached(cache: &OnceLock<Vec<u8>>, package: &str, features: Option<&str>) -> Result<Vec<u8>> {
    if let Some(bytes) = cache.get() {
        return Ok(bytes.clone());
    }

    let mut cmd = Command::new("cargo");
    cmd.args([
        "build",
        "--target",
        "wasm32-unknown-unknown",
        "--release",
        "-p",
        package,
        "--locked",
    ])
    .current_dir(format!("{}/..", repo_root()));
    if let Some(f) = features {
        cmd.args(["--features", f]);
    }
    let status = cmd
        .status()
        .with_context(|| format!("failed to build {package}"))?;
    if !status.success() {
        bail!("cargo build failed for {package}");
    }
    let raw_name = package.replace('-', "_");
    let path = format!(
        "{}/../target/wasm32-unknown-unknown/release/{raw_name}.wasm",
        repo_root()
    );
    let bytes = std::fs::read(path).with_context(|| format!("failed to read wasm for {package}"))?;
    let _ = cache.set(bytes.clone());
    Ok(bytes)
}

static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static INDEX_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static FAUCET_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static LIFELINE_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn ledger_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&LEDGER_WASM, "mock-icrc-ledger", None)
}
fn index_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&INDEX_WASM, "mock-icp-index", None)
}
fn cmc_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&CMC_WASM, "mock-cmc", None)
}
fn faucet_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&FAUCET_WASM, "jupiter-faucet", Some("debug_api"))
}
fn lifeline_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&LIFELINE_WASM, "jupiter-lifeline", None)
}

fn tick_n(pic: &PocketIc, n: usize) {
    for _ in 0..n {
        pic.tick();
    }
}

fn update_bytes<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    bytes: Vec<u8>,
) -> Result<R> {
    let reply = pic
        .update_call(canister, sender, method, bytes)
        .map_err(|e| anyhow!("update_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

fn update_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    update_bytes(pic, canister, sender, method, encode_one(arg)?)
}

fn update_noargs<R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
) -> Result<R> {
    update_one(pic, canister, sender, method, ())
}

fn query_one<A: CandidType, R: for<'de> Deserialize<'de> + CandidType>(
    pic: &PocketIc,
    canister: Principal,
    sender: Principal,
    method: &str,
    arg: A,
) -> Result<R> {
    let bytes = encode_one(arg)?;
    let reply = pic
        .query_call(canister, sender, method, bytes)
        .map_err(|e| anyhow!("query_call {method} rejected: {e:?}"))?;
    decode_one(&reply).map_err(Into::into)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct FaucetInitArg {
    staking_account: Account,
    payout_subaccount: Option<Vec<u8>>,
    ledger_canister_id: Option<Principal>,
    index_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    rescue_controller: Principal,
    blackhole_controller: Option<Principal>,
    blackhole_armed: Option<bool>,
    expected_first_staking_tx_id: Option<u64>,
    main_interval_seconds: Option<u64>,
    rescue_interval_seconds: Option<u64>,
    min_tx_e8s: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct FaucetUpgradeArg {
    blackhole_controller: Option<Principal>,
    blackhole_armed: Option<bool>,
    clear_forced_rescue: Option<bool>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugAccounts {
    payout: Account,
    staking: Account,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum ForcedRescueReason {
    BootstrapNoSuccess,
    IndexAnchorMissing,
    IndexLatestInvariantBroken,
    CmcZeroSuccessRuns,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    active_payout_job_present: bool,
    last_summary_present: bool,
    last_successful_transfer_ts: Option<u64>,
    last_rescue_check_ts: u64,
    rescue_triggered: bool,
    blackhole_controller: Option<Principal>,
    blackhole_armed_since_ts: Option<u64>,
    forced_rescue_reason: Option<ForcedRescueReason>,
    consecutive_index_anchor_failures: u8,
    consecutive_index_latest_invariant_failures: u8,
    consecutive_cmc_zero_success_runs: u8,
    last_observed_staking_balance_e8s: Option<u64>,
    last_observed_latest_tx_id: Option<u64>,
    expected_first_staking_tx_id: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugFootprint {
    state_candid_bytes: u64,
    active_payout_job_candid_bytes: u64,
    last_summary_candid_bytes: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct FaucetSummary {
    pot_start_e8s: u64,
    pot_remaining_e8s: u64,
    denom_staking_balance_e8s: u64,
    topped_up_count: u64,
    topped_up_sum_e8s: u64,
    topped_up_min_e8s: Option<u64>,
    topped_up_max_e8s: Option<u64>,
    failed_topups: u64,
    ignored_under_threshold: u64,
    ignored_bad_memo: u64,
    remainder_to_self_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugGetCall {
    account_identifier: String,
    start: Option<u64>,
    max_results: u64,
    returned_count: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct TransferRecord {
    from: Account,
    to: Account,
    amount: Nat,
    fee: Nat,
    memo: Option<Vec<u8>>,
    created_at_time: Option<u64>,
    result: String,
}

fn test_blackhole_controller() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai").unwrap()
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

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNextTransferError {

    TemporarilyUnavailable,
    TooOld,
    CreatedInFuture { ledger_time: u64 },
    BadFee { expected_fee_e8s: u64 },
    Duplicate { duplicate_of: u64 },
}
#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNotifyBehavior {
    Ok,
    Processing,
    Refunded { reason: String, block_index: Option<u64> },
    TransactionTooOld(u64),
    InvalidTransaction(String),
    Other { error_code: u64, error_message: String },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugIndexGetBehavior {
    Ok,
    Err(String),
}

struct FaucetEnv {
    pic: PocketIc,
    ledger: Principal,
    index: Principal,
    cmc: Principal,
    lifeline: Principal,
    faucet: Principal,
    blackhole_controller: Principal,
    staking_account: Account,
    accounts: DebugAccounts,
    staking_id: String,
}

impl FaucetEnv {
    fn new() -> Result<Self> {
        Self::new_with_init_overrides(|_| {})
    }

    fn new_with_init_overrides<F>(edit_init: F) -> Result<Self>
    where
        F: FnOnce(&mut FaucetInitArg),
    {
        let pic = PocketIcBuilder::new().with_application_subnet().build();
        let ledger = pic.create_canister();
        let index = pic.create_canister();
        let cmc = pic.create_canister();
        let lifeline = pic.create_canister();
        let faucet = pic.create_canister();

        for c in [ledger, index, cmc, lifeline, faucet] {
            pic.add_cycles(c, 5_000_000_000_000);
        }

        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(index, index_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(lifeline, lifeline_wasm()?, vec![], None);

        let staking_account = Account {
            owner: Principal::anonymous(),
            subaccount: Some([9u8; 32]),
        };
        let blackhole_controller = test_blackhole_controller();

        let mut init = FaucetInitArg {
            staking_account: staking_account.clone(),
            payout_subaccount: None,
            ledger_canister_id: Some(ledger),
            index_canister_id: Some(index),
            cmc_canister_id: Some(cmc),
            rescue_controller: lifeline,
            blackhole_controller: Some(blackhole_controller),
            blackhole_armed: Some(false),
            expected_first_staking_tx_id: None,
            main_interval_seconds: Some(60),
            rescue_interval_seconds: Some(60),
            min_tx_e8s: Some(100_000_000),
        };
        edit_init(&mut init);
        pic.install_canister(faucet, faucet_wasm()?, encode_one(init)?, None);

        let accounts: DebugAccounts = query_one(&pic, faucet, Principal::anonymous(), "debug_accounts", ())?;
        let staking_id = account_identifier_text(&staking_account);

        Ok(Self {
            pic,
            ledger,
            index,
            cmc,
            lifeline,
            faucet,
            blackhole_controller,
            staking_account,
            accounts,
            staking_id,
        })
    }

    fn credit_payout(&self, amount_e8s: u64) -> Result<()> {
        update_bytes::<()>(&self.pic, self.ledger, Principal::anonymous(), "debug_credit", encode_args((self.accounts.payout.clone(), amount_e8s))?)
    }

    fn credit_staking(&self, amount_e8s: u64) -> Result<()> {
        update_bytes::<()>(&self.pic, self.ledger, Principal::anonymous(), "debug_credit", encode_args((self.staking_account.clone(), amount_e8s))?)
    }

    fn append_transfer(&self, amount_e8s: u64, memo: Option<Vec<u8>>) -> Result<u64> {
        update_bytes(&self.pic, self.index, Principal::anonymous(), "debug_append_transfer", encode_args((self.staking_id.clone(), amount_e8s, memo))?)
    }

    fn append_repeated_transfer(&self, count: u64, amount_e8s: u64, memo: Option<Vec<u8>>) -> Result<u64> {
        update_bytes(&self.pic, self.index, Principal::anonymous(), "debug_append_repeated_transfer", encode_args((self.staking_id.clone(), count, amount_e8s, memo))?)
    }

    fn set_cmc_fail(&self, fail: bool) -> Result<()> {
        update_one(&self.pic, self.cmc, Principal::anonymous(), "debug_set_fail", fail)
    }

    fn set_cmc_script(&self, script: Vec<DebugNotifyBehavior>) -> Result<()> {
        update_one(&self.pic, self.cmc, Principal::anonymous(), "debug_set_script", script)
    }

    fn set_ledger_next_error(&self, err: Option<DebugNextTransferError>) -> Result<()> {
        update_one(&self.pic, self.ledger, Principal::anonymous(), "debug_set_next_error", err)
    }

    fn set_ledger_error_script(&self, errs: Vec<DebugNextTransferError>) -> Result<()> {
        update_one(&self.pic, self.ledger, Principal::anonymous(), "debug_set_error_script", errs)
    }

    fn set_index_get_script(&self, script: Vec<DebugIndexGetBehavior>) -> Result<()> {
        update_one(&self.pic, self.index, Principal::anonymous(), "debug_set_get_script", script)
    }

    fn set_last_successful_transfer_ts(&self, ts: Option<u64>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_last_successful_transfer_ts", ts)
    }

    fn set_blackhole_armed(&self, v: Option<bool>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_blackhole_armed", v)
    }

    fn set_blackhole_armed_since_ts(&self, ts: Option<u64>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_blackhole_armed_since_ts", ts)
    }

    fn set_expected_first_staking_tx_id(&self, v: Option<u64>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_expected_first_staking_tx_id", v)
    }

    fn set_main_lock_expires_at_ts(&self, ts: Option<u64>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_main_lock_expires_at_ts", ts)
    }

    fn set_trap_after_successful_transfers(&self, n: Option<u32>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_trap_after_successful_transfers", n)
    }

    fn set_real_trap_after_successful_transfers(&self, n: Option<u32>) -> Result<()> {
        update_one(&self.pic, self.faucet, Principal::anonymous(), "debug_set_real_trap_after_successful_transfers", n)
    }

    fn rescue_tick(&self) -> Result<()> {
        update_noargs::<()>(&self.pic, self.faucet, Principal::anonymous(), "debug_rescue_tick")?;
        tick_n(&self.pic, 10);
        Ok(())
    }

    fn controllers(&self) -> Vec<Principal> {
        self.pic.get_controllers(self.faucet)
    }

    fn set_blackholed_controllers(&self) -> Result<()> {
        let current = self.pic.get_controllers(self.faucet);
        let sender = current.first().copied().unwrap_or(self.faucet);
        self.pic
            .set_controllers(self.faucet, Some(sender), vec![self.blackhole_controller, self.faucet])
            .map_err(|e| anyhow!("set_controllers reject: {e:?}"))?;
        Ok(())
    }

    fn advance_time_and_tick(&self, secs: u64, ticks: usize) {
        self.pic.advance_time(Duration::from_secs(secs));
        tick_n(&self.pic, ticks);
    }

    fn main_tick(&self) -> Result<()> {
        update_noargs::<()>(&self.pic, self.faucet, Principal::anonymous(), "debug_main_tick")?;
        tick_n(&self.pic, 10);
        Ok(())
    }

    fn state(&self) -> Result<DebugState> {
        query_one(&self.pic, self.faucet, Principal::anonymous(), "debug_state", ())
    }

    fn footprint(&self) -> Result<DebugFootprint> {
        query_one(&self.pic, self.faucet, Principal::anonymous(), "debug_footprint", ())
    }

    fn summary(&self) -> Result<FaucetSummary> {
        let summary: Option<FaucetSummary> = query_one(&self.pic, self.faucet, Principal::anonymous(), "debug_last_summary", ())?;
        summary.ok_or_else(|| anyhow!("expected faucet summary"))
    }

    fn notifications(&self) -> Result<Vec<NotifyRecord>> {
        query_one(&self.pic, self.cmc, Principal::anonymous(), "debug_notifications", ())
    }

    fn ledger_transfers(&self) -> Result<Vec<TransferRecord>> {
        query_one(&self.pic, self.ledger, Principal::anonymous(), "debug_transfers", ())
    }

    fn index_get_calls(&self) -> Result<Vec<DebugGetCall>> {
        query_one(&self.pic, self.index, Principal::anonymous(), "debug_get_calls", ())
    }

    fn upgrade(&self) -> Result<()> {
        self.upgrade_with_args(FaucetUpgradeArg { blackhole_controller: None, blackhole_armed: None, clear_forced_rescue: None })
    }

    fn upgrade_with_args(&self, args: FaucetUpgradeArg) -> Result<()> {
        let sender = self.pic.get_controllers(self.faucet).first().copied().unwrap_or(self.faucet);
        self.pic
            .upgrade_canister(self.faucet, faucet_wasm()?, encode_one(Some(args))?, Some(sender))
            .map_err(|e| anyhow!("upgrade_canister reject: {e:?}"))?;
        tick_n(&self.pic, 5);
        Ok(())
    }
}

#[test]
#[ignore]
fn faucet_retries_persisted_notification_after_cmc_failure() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.set_cmc_script(vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok])?;
    env.main_tick()?;

    let st = env.state()?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected inline notify retry to complete within one tick without persisted retry state");
    }
    if env.notifications()?.len() != 1 {
        bail!("expected exactly one CMC notification after inline retry success");
    }
    if env.ledger_transfers()?.len() != 1 {
        bail!("expected inline notify retry to avoid any duplicate ledger transfer");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_retries_notify_without_duplicate_ledger_transfer_across_repeated_ticks() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.set_cmc_script(vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok])?;
    env.main_tick()?;
    env.main_tick()?;
    env.main_tick()?;

    let transfers = env.ledger_transfers()?;
    if transfers.len() != 1 {
        bail!("expected inline notify retry to avoid duplicate ledger transfers across repeated ticks, got {} transfers", transfers.len());
    }
    if env.notifications()?.len() != 1 {
        bail!("expected exactly one notification after inline recovery");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_upgrade_after_inline_retry_recovery_remains_quiescent() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(90_000_000)?;
    env.credit_staking(90_000_000)?;
    env.append_transfer(90_000_000, Some(target.to_text().into_bytes()))?;
    env.set_cmc_script(vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok])?;

    env.main_tick()?;

    let st_before = env.state()?;
    if st_before.active_payout_job_present || !st_before.last_summary_present {
        bail!("expected inline retry path to complete before upgrade");
    }
    let transfers_before = env.ledger_transfers()?;
    if transfers_before.len() != 1 {
        bail!("expected a single beneficiary ledger transfer before upgrade, got {}", transfers_before.len());
    }
    if env.notifications()?.len() != 1 {
        bail!("expected one completed notification before upgrade");
    }

    env.upgrade()?;

    let st_after = env.state()?;
    if st_after.active_payout_job_present || !st_after.last_summary_present {
        bail!("expected completed inline retry recovery to remain quiescent across upgrade");
    }

    let transfers_after = env.ledger_transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected upgrade not to duplicate beneficiary transfer, got {}", transfers_after.len());
    }
    if env.notifications()?.len() != 1 {
        bail!("expected exactly one notification to remain after upgrade");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_replays_full_history_on_each_new_job_and_keeps_same_beneficiary_separate() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let memo = Some(target.to_text().into_bytes());

    env.credit_staking(300_000_000)?;
    env.append_repeated_transfer(3, 100_000_000, memo.clone())?;

    env.credit_payout(90_000_000)?;
    env.main_tick()?;

    let first_summary = env.summary()?;
    if first_summary.topped_up_count != 3 {
        bail!("expected first payout job to top up three separate historical contributions, got {}", first_summary.topped_up_count);
    }
    let first_calls = env.index_get_calls()?;
    let first_starts: Vec<Option<u64>> = first_calls.iter().map(|c| c.start).collect();
    if first_starts != vec![None] {
        bail!("expected first payout job to begin scanning from start, got starts {first_starts:?}");
    }

    env.credit_payout(60_000_000)?;
    env.main_tick()?;

    let second_summary = env.summary()?;
    if second_summary.topped_up_count != 3 {
        bail!("expected second payout job to replay the same three contributions, got {}", second_summary.topped_up_count);
    }
    let notifications = env.notifications()?;
    let beneficiary_notes = notifications.iter().filter(|n| n.canister_id == target).count();
    if beneficiary_notes != 6 {
        bail!("expected replay semantics to notify the same beneficiary three times per run, got {beneficiary_notes}");
    }
    let all_calls = env.index_get_calls()?;
    let starts: Vec<Option<u64>> = all_calls.iter().map(|c| c.start).collect();
    if starts != vec![None, None] {
        bail!("expected each new payout job to rescan from start, got starts {starts:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_scans_across_many_pages_and_skips_bad_or_small_entries_without_poisoning_run() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let good_target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let memo = Some(good_target.to_text().into_bytes());

    env.credit_staking(1_500_000_000)?;
    env.append_repeated_transfer(499, 1_000_000, memo.clone())?;
    env.append_transfer(200_000_000, Some(b"bad-memo".to_vec()))?;
    env.append_repeated_transfer(500, 1_000_000, None)?;
    env.append_transfer(1_500_000, memo.clone())?;
    env.append_transfer(300_000_000, memo.clone())?;

    env.credit_payout(120_000_000)?;
    env.main_tick()?;

    let summary = env.summary()?;
    if summary.topped_up_count != 1 {
        bail!("expected exactly one qualifying contribution across many pages, got {}", summary.topped_up_count);
    }
    if summary.ignored_under_threshold != 1000 {
        bail!("expected 1000 under-threshold contributions to be counted, got {}", summary.ignored_under_threshold);
    }
    if summary.ignored_bad_memo != 1 {
        bail!("expected one bad memo to be skipped, got {}", summary.ignored_bad_memo);
    }
    let calls = env.index_get_calls()?;
    if calls.len() < 3 {
        bail!("expected many-page scan to require multiple index calls, got {}", calls.len());
    }
    if calls.first().and_then(|c| c.start).is_some() {
        bail!("expected the first page scan to start from the beginning");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_upgrade_with_partial_progress_resumes_cursor_and_preserves_completed_work() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let first = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let second = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;

    env.credit_staking(800_000_000)?;
    env.append_transfer(200_000_000, Some(first.to_text().into_bytes()))?;
    env.append_repeated_transfer(499, 1_000_000, Some(first.to_text().into_bytes()))?;
    env.append_transfer(101_000_000, Some(second.to_text().into_bytes()))?;
    env.credit_payout(120_000_000)?;
    env.set_index_get_script(vec![
        DebugIndexGetBehavior::Ok,
        DebugIndexGetBehavior::Err("mid-scan failure".to_string()),
        DebugIndexGetBehavior::Ok,
    ])?;

    env.main_tick()?;

    let st = env.state()?;
    if !st.active_payout_job_present || st.last_summary_present {
        bail!("expected partial progress with an active job cursor and no final summary after mid-scan index failure");
    }
    let notifications_before = env.notifications()?;
    if notifications_before.iter().filter(|n| n.canister_id == first).count() != 1 {
        bail!("expected first beneficiary to be fully processed before upgrade");
    }
    if notifications_before.iter().filter(|n| n.canister_id == second).count() != 0 {
        bail!("expected second beneficiary not to be processed before upgrade");
    }

    env.upgrade()?;
    env.main_tick()?;

    let notifications_after = env.notifications()?;
    if notifications_after.iter().filter(|n| n.canister_id == first).count() != 1 {
        bail!("expected first beneficiary not to be replayed within the same active job after upgrade");
    }
    if notifications_after.iter().filter(|n| n.canister_id == second).count() != 1 {
        bail!("expected second beneficiary to complete exactly once after upgrade resume");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 2 {
        bail!("expected two completed beneficiary top-ups after resume, got {}", summary.topped_up_count);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_upgrade_with_partial_progress_resumes_automatically_without_waiting_for_weekly_interval() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.main_interval_seconds = Some(7 * 24 * 60 * 60);
    })?;
    let first = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let second = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;

    env.credit_staking(800_000_000)?;
    env.append_transfer(200_000_000, Some(first.to_text().into_bytes()))?;
    env.append_repeated_transfer(499, 1_000_000, Some(first.to_text().into_bytes()))?;
    env.append_transfer(101_000_000, Some(second.to_text().into_bytes()))?;
    env.credit_payout(120_000_000)?;
    env.set_index_get_script(vec![
        DebugIndexGetBehavior::Ok,
        DebugIndexGetBehavior::Err("mid-scan failure".to_string()),
        DebugIndexGetBehavior::Ok,
    ])?;

    env.main_tick()?;

    let st_before = env.state()?;
    if !st_before.active_payout_job_present || st_before.last_summary_present {
        bail!("expected partial progress with an active job cursor and no final summary before upgrade");
    }
    env.upgrade()?;

    let st_after_upgrade = env.state()?;
    if !st_after_upgrade.active_payout_job_present || st_after_upgrade.last_summary_present {
        bail!("expected active payout job to remain persisted immediately after upgrade before the one-shot resume tick fires");
    }

    env.advance_time_and_tick(1, 20);

    let notifications_after = env.notifications()?;
    if notifications_after.iter().filter(|n| n.canister_id == first).count() != 1 {
        bail!("expected first beneficiary not to be replayed by the automatic post-upgrade resume tick");
    }
    if notifications_after.iter().filter(|n| n.canister_id == second).count() != 1 {
        bail!("expected second beneficiary to complete exactly once via the automatic post-upgrade resume tick");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 2 {
        bail!("expected automatic post-upgrade resume to finish the active job without waiting for the weekly interval, got {} completed top-ups", summary.topped_up_count);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_upgrade_during_transfer_notify_boundary_recovers_without_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.main_interval_seconds = Some(7 * 24 * 60 * 60);
        init.rescue_interval_seconds = Some(7 * 24 * 60 * 60);
    })?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.set_trap_after_successful_transfers(Some(1))?;
    env.main_tick()?;

    let st_mid = env.state()?;
    if !st_mid.active_payout_job_present || st_mid.last_summary_present {
        bail!("expected injected interruption to leave an active payout job without final summary before upgrade");
    }
    let transfers_mid = env.ledger_transfers()?;
    if transfers_mid.len() != 1 {
        bail!("expected exactly one beneficiary ledger transfer to land before upgrade, got {}", transfers_mid.len());
    }
    if !env.notifications()?.is_empty() {
        bail!("expected interruption before notify_top_up to leave no CMC notifications before upgrade");
    }

    env.upgrade()?;

    let st_after_upgrade = env.state()?;
    if !st_after_upgrade.active_payout_job_present || st_after_upgrade.last_summary_present {
        bail!("expected interrupted payout job to remain persisted immediately after upgrade before auto-resume fires");
    }

    env.advance_time_and_tick(1, 20);

    let transfers_after = env.ledger_transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected post-upgrade recovery to reuse the original ledger transfer without duplication, got {} transfers", transfers_after.len());
    }
    let notes = env.notifications()?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected exactly one beneficiary notification after post-upgrade recovery, got {notes:?}");
    }

    let summary = env.summary()?;
    if summary.topped_up_count != 1 || summary.topped_up_sum_e8s != 99_990_000 || summary.failed_topups != 0 {
        bail!("unexpected summary after post-upgrade recovery: topped_up_count={} topped_up_sum_e8s={} failed_topups={}", summary.topped_up_count, summary.topped_up_sum_e8s, summary.failed_topups);
    }

    let st_done = env.state()?;
    if st_done.active_payout_job_present || !st_done.last_summary_present {
        bail!("expected post-upgrade recovery to finalize the interrupted payout job");
    }


    Ok(())
}

#[test]
#[ignore]
fn faucet_real_trap_during_transfer_notify_boundary_recovers_without_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.main_interval_seconds = Some(7 * 24 * 60 * 60);
        init.rescue_interval_seconds = Some(7 * 24 * 60 * 60);
    })?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.set_real_trap_after_successful_transfers(Some(1))?;
    let trapped = update_noargs::<()>(&env.pic, env.faucet, Principal::anonymous(), "debug_main_tick");
    if trapped.is_ok() {
        bail!("expected debug_main_tick to reject after injected real trap");
    }
    tick_n(&env.pic, 10);

    let st_mid = env.state()?;
    if !st_mid.active_payout_job_present || st_mid.last_summary_present {
        bail!("expected real trap to leave an active payout job without final summary before upgrade");
    }
    let transfers_mid = env.ledger_transfers()?;
    if transfers_mid.len() != 1 {
        bail!("expected exactly one beneficiary ledger transfer to land before upgrade, got {}", transfers_mid.len());
    }
    if !env.notifications()?.is_empty() {
        bail!("expected real trap before notify_top_up to leave no CMC notifications before upgrade");
    }

    env.upgrade()?;

    let st_after_upgrade = env.state()?;
    if !st_after_upgrade.active_payout_job_present || st_after_upgrade.last_summary_present {
        bail!("expected trapped payout job to remain persisted immediately after upgrade before auto-resume fires");
    }

    env.advance_time_and_tick(1, 20);

    let transfers_after = env.ledger_transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected post-upgrade recovery after real trap to reuse the original ledger transfer without duplication, got {} transfers", transfers_after.len());
    }
    let notes = env.notifications()?;
    if notes.len() != 1 || notes[0].canister_id != target {
        bail!("expected exactly one beneficiary notification after post-upgrade recovery from real trap, got {notes:?}");
    }

    let summary = env.summary()?;
    if summary.topped_up_count != 1 || summary.topped_up_sum_e8s != 99_990_000 || summary.failed_topups != 0 {
        bail!("unexpected summary after post-upgrade real-trap recovery: topped_up_count={} topped_up_sum_e8s={} failed_topups={}", summary.topped_up_count, summary.topped_up_sum_e8s, summary.failed_topups);
    }

    let st_done = env.state()?;
    if st_done.active_payout_job_present || !st_done.last_summary_present {
        bail!("expected post-upgrade real-trap recovery to finalize the interrupted payout job");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_large_history_repeated_runs_keep_state_footprint_bounded() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let memo = Some(target.to_text().into_bytes());

    let baseline = env.footprint()?;
    env.credit_staking(2_000_000_000)?;
    env.append_repeated_transfer(2_000, 1_000_000, memo.clone())?;
    env.append_repeated_transfer(10, 100_000_000, memo)?;

    for _ in 0..3 {
        env.credit_payout(200_000_000)?;
        env.main_tick()?;
        let summary = env.summary()?;
        if summary.topped_up_count != 10 {
            bail!("expected each replay over the large history to top up the same 10 qualifying entries, got {}", summary.topped_up_count);
        }
    }

    let final_footprint = env.footprint()?;
    if final_footprint.active_payout_job_candid_bytes != 0 {
        bail!("expected no in-flight state after large repeated runs");
    }
    if final_footprint.state_candid_bytes > baseline.state_candid_bytes.saturating_add(512) {
        bail!(
            "expected completed large-history runs to return near baseline footprint; baseline={} final={}",
            baseline.state_candid_bytes,
            final_footprint.state_candid_bytes
        );
    }

    Ok(())
}


#[test]
#[ignore]
fn faucet_timer_cadence_waits_for_elapsed_time_before_running_automatically() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.advance_time_and_tick(30, 10);
    let st_before = env.state()?;
    if st_before.last_summary_present || st_before.active_payout_job_present {
        bail!("expected faucet timer not to fire before the 60-second interval elapsed");
    }
    if !env.ledger_transfers()?.is_empty() {
        bail!("expected no ledger transfers before timer interval elapsed");
    }

    env.advance_time_and_tick(31, 20);
    let summary = env.summary()?;
    if summary.topped_up_count != 1 {
        bail!("expected automatic timer run to complete one beneficiary top-up, got {}", summary.topped_up_count);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_repeated_ticks_after_completion_do_not_duplicate_topups() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.main_tick()?;

    let notes_after_first = env.notifications()?;
    let transfers_after_first = env.ledger_transfers()?;
    if notes_after_first.len() != 1 || transfers_after_first.len() != 1 {
        bail!("expected one completed top-up after first run, got notes={} transfers={}", notes_after_first.len(), transfers_after_first.len());
    }

    env.main_tick()?;
    env.main_tick()?;
    env.main_tick()?;

    let notes_after_repeats = env.notifications()?;
    let transfers_after_repeats = env.ledger_transfers()?;
    if notes_after_repeats.len() != 1 || transfers_after_repeats.len() != 1 {
        bail!("expected repeated ticks with no new payout balance not to duplicate work, got notes={} transfers={}", notes_after_repeats.len(), transfers_after_repeats.len());
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_debug_footprint_returns_to_baseline_after_retry() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    let baseline = env.footprint()?;
    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    env.set_cmc_script(vec![DebugNotifyBehavior::Processing, DebugNotifyBehavior::Ok])?;
    env.main_tick()?;

    let after_run = env.footprint()?;
    if after_run.active_payout_job_candid_bytes != 0 {
        bail!("expected inline retry policy not to leave any persisted in-flight footprint");
    }
    if after_run.state_candid_bytes > baseline.state_candid_bytes.saturating_add(1024) {
        bail!(
            "expected inline retry path to return near baseline footprint; baseline={} after_run={}",
            baseline.state_candid_bytes,
            after_run.state_candid_bytes
        );
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_ledger_temporary_failure_before_transfer_recovers_inline() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.set_ledger_next_error(Some(DebugNextTransferError::TemporarilyUnavailable))?;

    env.main_tick()?;
    let st = env.state()?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected temporary ledger failure before transfer to be retried inline and finish within one tick");
    }
    if env.ledger_transfers()?.len() != 1 {
        bail!("expected exactly one beneficiary transfer after inline recovery");
    }
    if env.notifications()?.len() != 1 {
        bail!("expected one beneficiary notification after inline recovery");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_duplicate_ledger_result_uses_duplicate_block_index_without_new_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.set_ledger_next_error(Some(DebugNextTransferError::Duplicate { duplicate_of: 55 }))?;

    env.main_tick()?;

    let transfers = env.ledger_transfers()?;
    if !transfers.is_empty() {
        bail!("expected injected duplicate path not to create a fresh mock-ledger transfer");
    }
    let notes = env.notifications()?;
    if notes.len() != 1 || notes[0].block_index != 55 {
        bail!("expected duplicate block index 55 to be used for notify_top_up, got {notes:?}");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 1 {
        bail!("expected duplicate ledger result to still count as a completed beneficiary top-up, got {}", summary.topped_up_count);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_temporary_ledger_failure_then_duplicate_counts_as_success_without_extra_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.set_ledger_error_script(vec![
        DebugNextTransferError::TemporarilyUnavailable,
        DebugNextTransferError::Duplicate { duplicate_of: 56 },
    ])?;

    env.main_tick()?;
    let st = env.state()?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected duplicate-on-inline-retry path to finish within one tick");
    }
    let transfers = env.ledger_transfers()?;
    if !transfers.is_empty() {
        bail!("expected duplicate-on-inline-retry path not to create any fresh mock-ledger transfers, got {} transfers", transfers.len());
    }
    let notes = env.notifications()?;
    let beneficiary_count = notes.iter().filter(|n| n.canister_id == target).count();
    let self_count = notes.iter().filter(|n| n.canister_id == env.faucet).count();
    if beneficiary_count != 1 || self_count != 0 || notes.iter().find(|n| n.canister_id == target).map(|n| n.block_index) != Some(56) {
        bail!("expected one beneficiary notification with duplicate block index 56 and no self notification in the full-pot case, got {notes:?}");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 1 || summary.failed_topups != 0 || summary.remainder_to_self_e8s != 0 {
        bail!(
            "expected duplicate-on-inline-retry path to count as success with no remainder in the full-pot case, got topped_up_count={} failed_topups={} remainder_to_self_e8s={}",
            summary.topped_up_count,
            summary.failed_topups,
            summary.remainder_to_self_e8s
        );
    }

    Ok(())
}


#[test]
#[ignore]
fn faucet_terminal_cmc_errors_still_retry_safely_without_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    for script in [
        vec![
            DebugNotifyBehavior::Refunded {
                reason: "refunded".to_string(),
                block_index: Some(7),
            },
            DebugNotifyBehavior::Ok,
        ],
        vec![DebugNotifyBehavior::TransactionTooOld(99), DebugNotifyBehavior::Ok],
        vec![
            DebugNotifyBehavior::InvalidTransaction("bad block".to_string()),
            DebugNotifyBehavior::Ok,
        ],
    ] {
        update_noargs::<()>(&env.pic, env.ledger, Principal::anonymous(), "debug_reset")?;
        update_noargs::<()>(&env.pic, env.index, Principal::anonymous(), "debug_reset")?;
        update_noargs::<()>(&env.pic, env.cmc, Principal::anonymous(), "debug_reset")?;
        update_noargs::<()>(&env.pic, env.faucet, Principal::anonymous(), "debug_reset_runtime_state")?;

        env.credit_payout(100_000_000)?;
        env.credit_staking(100_000_000)?;
        env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
        env.set_cmc_script(script)?;

        env.main_tick()?;
        let st = env.state()?;
        if st.active_payout_job_present || !st.last_summary_present {
            bail!("expected terminal typed CMC error to be retried inline and complete within one tick");
        }
        if env.ledger_transfers()?.len() != 1 || env.notifications()?.len() != 1 {
            bail!("expected terminal typed CMC inline retry path to avoid duplicate transfer and finish with one notification");
        }
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_retry_exhaustion_skips_contribution_and_finishes_with_remainder_accounting() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let faucet_id = env.faucet;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.set_ledger_error_script(vec![
        DebugNextTransferError::TemporarilyUnavailable,
        DebugNextTransferError::TemporarilyUnavailable,
    ])?;

    env.main_tick()?;
    let st = env.state()?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected exhausted inline retry path to complete the job within one tick");
    }
    let summary = env.summary()?;
    if summary.failed_topups != 1 || summary.topped_up_count != 0 {
        bail!(
            "expected exhausted inline retry path to record one failed top-up and no beneficiary success, got failed_topups={} topped_up_count={}",
            summary.failed_topups,
            summary.topped_up_count
        );
    }
    if summary.remainder_to_self_e8s != 99_990_000 || summary.pot_remaining_e8s != 0 {
        bail!(
            "expected exhausted inline retry path to finish via full remainder-to-self accounting, got remainder_to_self_e8s={} pot_remaining_e8s={}",
            summary.remainder_to_self_e8s,
            summary.pot_remaining_e8s
        );
    }
    let transfers = env.ledger_transfers()?;
    if transfers.len() != 1 {
        bail!("expected only the fallback remainder transfer after inline retry exhaustion, got {} transfers", transfers.len());
    }
    let notes = env.notifications()?;
    if notes.len() != 1 || notes[0].canister_id != faucet_id {
        bail!("expected inline retry exhaustion to end with exactly one self notification, got {notes:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_retry_exhaustion_on_one_contribution_does_not_block_later_success_in_same_job() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let failed_target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let success_target = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;
    let faucet_id = env.faucet;

    env.credit_payout(100_000_000)?;
    env.credit_staking(200_000_000)?;
    env.append_transfer(100_000_000, Some(failed_target.to_text().into_bytes()))?;
    env.append_transfer(100_000_000, Some(success_target.to_text().into_bytes()))?;
    env.set_ledger_error_script(vec![
        DebugNextTransferError::TemporarilyUnavailable,
        DebugNextTransferError::TemporarilyUnavailable,
    ])?;

    env.main_tick()?;
    let st = env.state()?;
    if st.active_payout_job_present || !st.last_summary_present {
        bail!("expected inline retry exhaustion on the first contribution not to leave work behind");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 1 || summary.failed_topups != 1 {
        bail!(
            "expected one later success and one exhausted inline retry failure, got topped_up_count={} failed_topups={}",
            summary.topped_up_count,
            summary.failed_topups
        );
    }
    if summary.remainder_to_self_e8s != 49_990_000 {
        bail!("expected remaining half of the pot to be returned to self, got {}", summary.remainder_to_self_e8s);
    }
    let notes = env.notifications()?;
    let success_count = notes.iter().filter(|n| n.canister_id == success_target).count();
    let failed_count = notes.iter().filter(|n| n.canister_id == failed_target).count();
    let self_count = notes.iter().filter(|n| n.canister_id == faucet_id).count();
    if success_count != 1 || failed_count != 0 || self_count != 1 {
        bail!(
            "expected only the later contribution and the self remainder to notify successfully, got success_count={} failed_count={} self_count={}",
            success_count,
            failed_count,
            self_count
        );
    }
    if env.ledger_transfers()?.len() != 2 {
        bail!("expected one beneficiary transfer plus one self remainder transfer after inline retry exhaustion");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_index_failure_mid_scan_resumes_without_duplicating_completed_work() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let first_target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let second_target = Principal::from_text("r7inp-6aaaa-aaaaa-aaabq-cai")?;

    env.credit_payout(120_000_000)?;
    env.credit_staking(800_000_000)?;
    env.append_transfer(200_000_000, Some(first_target.to_text().into_bytes()))?;
    env.append_repeated_transfer(499, 1_000_000, Some(first_target.to_text().into_bytes()))?;
    env.append_transfer(101_000_000, Some(second_target.to_text().into_bytes()))?;
    env.set_index_get_script(vec![
        DebugIndexGetBehavior::Ok,
        DebugIndexGetBehavior::Err("mid-scan failure".to_string()),
        DebugIndexGetBehavior::Ok,
    ])?;

    env.main_tick()?;
    let st1 = env.state()?;
    if !st1.active_payout_job_present || st1.last_summary_present {
        bail!("expected mid-scan index failure to preserve partial job state without a final summary");
    }
    let notes_after_first = env.notifications()?;
    let first_count_after_first = notes_after_first.iter().filter(|n| n.canister_id == first_target).count();
    let second_count_after_first = notes_after_first.iter().filter(|n| n.canister_id == second_target).count();
    if first_count_after_first != 1 || second_count_after_first != 0 {
        bail!(
            "expected first page work to complete before index failure, got first_count={} second_count={}",
            first_count_after_first,
            second_count_after_first
        );
    }

    env.main_tick()?;
    let st2 = env.state()?;
    if st2.active_payout_job_present || !st2.last_summary_present {
        bail!("expected second tick to resume and complete the job after index recovery");
    }
    let summary = env.summary()?;
    if summary.topped_up_count != 2 || summary.ignored_under_threshold != 499 {
        bail!(
            "expected resumed scan to preserve first-page work and finish second page, got topped_up_count={} ignored_under_threshold={}",
            summary.topped_up_count,
            summary.ignored_under_threshold
        );
    }
    let notes = env.notifications()?;
    let first_count = notes.iter().filter(|n| n.canister_id == first_target).count();
    let second_count = notes.iter().filter(|n| n.canister_id == second_target).count();
    if first_count != 1 || second_count != 1 {
        bail!(
            "expected resumed scan not to duplicate first-page work and to complete the later contribution, got first_count={} second_count={}",
            first_count,
            second_count
        );
    }
    let calls = env.index_get_calls()?;
    if calls.len() < 3 || calls[0].start.is_some() || calls[1].start.is_none() || calls[2].start != calls[1].start {
        bail!("expected resume after failure to retry the same later-page cursor, got calls {calls:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_rescue_controller_roundtrip_uses_real_controller_updates() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    let c0 = env.controllers();
    if !(c0.contains(&env.faucet) && c0.contains(&env.blackhole_controller) && c0.len() == 2) {
        bail!("expected blackhole+self controller baseline, got {c0:?}");
    }

    env.set_blackhole_armed(Some(true))?;
    env.set_last_successful_transfer_ts(Some(0))?;
    env.rescue_tick()?;

    let mut broken = env.controllers();
    broken.sort_by_key(|p| p.to_text());
    let mut expected_broken = vec![env.blackhole_controller, env.lifeline, env.faucet];
    expected_broken.sort_by_key(|p| p.to_text());
    if broken != expected_broken {
        bail!("expected broken rescue path to widen controllers to [blackhole,self,rescue], got {broken:?}");
    }

    let now_secs = (env.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    env.set_last_successful_transfer_ts(Some(now_secs.saturating_add(1)))?;
    env.rescue_tick()?;

    let recovered = env.controllers();
    if !(recovered.contains(&env.faucet) && recovered.contains(&env.blackhole_controller) && recovered.len() == 2) {
        bail!("expected healthy rescue path to converge back to blackhole+self, got {recovered:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_large_history_many_replays_do_not_monotonically_drift_state_size() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let memo = Some(target.to_text().into_bytes());

    let baseline = env.footprint()?;
    env.credit_staking(2_000_000_000)?;
    env.append_repeated_transfer(2_000, 1_000_000, memo.clone())?;
    env.append_repeated_transfer(5, 100_000_000, memo)?;

    for run in 0..20u64 {
        env.credit_payout(120_000_000)?;
        env.main_tick()?;
        let summary = env.summary()?;
        if summary.topped_up_count != 5 {
            bail!("expected replay {run} to top up the same 5 qualifying entries, got {}", summary.topped_up_count);
        }
        let footprint = env.footprint()?;
        if footprint.active_payout_job_candid_bytes != 0 {
            bail!("expected replay {run} to finish without residual in-flight state");
        }
        if footprint.state_candid_bytes > baseline.state_candid_bytes.saturating_add(512) {
            bail!(
                "expected replay {run} footprint to remain near baseline; baseline={} current={}",
                baseline.state_candid_bytes,
                footprint.state_candid_bytes
            );
        }
    }

    Ok(())
}


#[test]
#[ignore]
fn faucet_unarmed_rescue_broken_conditions_do_not_change_controllers() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(false))?;
    env.set_last_successful_transfer_ts(Some(0))?;
    let before = env.controllers();

    env.rescue_tick()?;

    let after = env.controllers();
    if after != before || !(after.contains(&env.faucet) && after.contains(&env.blackhole_controller) && after.len() == 2) {
        bail!("expected unarmed rescue tick under broken conditions to leave controllers unchanged, before={before:?} after={after:?}");
    }
    let st = env.state()?;
    if st.rescue_triggered || st.forced_rescue_reason.is_some() {
        bail!("expected unarmed broken-path rescue tick not to latch rescue state, got {:?}", st);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_unarmed_rescue_forced_reason_does_not_change_controllers() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(false))?;
    env.set_expected_first_staking_tx_id(Some(1))?;
    let before = env.controllers();

    env.main_tick()?;
    env.main_tick()?;

    let st_after_latch = env.state()?;
    if st_after_latch.forced_rescue_reason != Some(ForcedRescueReason::IndexAnchorMissing) {
        bail!("expected missing anchor twice to latch forced rescue reason even while unarmed, got {:?}", st_after_latch);
    }
    let after_latch = env.controllers();
    if after_latch != before || !(after_latch.contains(&env.faucet) && after_latch.contains(&env.blackhole_controller) && after_latch.len() == 2) {
        bail!("expected forced rescue reason while unarmed not to change controllers, before={before:?} after_latch={after_latch:?}");
    }

    env.rescue_tick()?;

    let after_tick = env.controllers();
    if after_tick != before || !(after_tick.contains(&env.faucet) && after_tick.contains(&env.blackhole_controller) && after_tick.len() == 2) {
        bail!("expected explicit rescue tick while unarmed and forced to leave controllers unchanged, before={before:?} after_tick={after_tick:?}");
    }
    let st_after_tick = env.state()?;
    if st_after_tick.rescue_triggered {
        bail!("expected unarmed forced rescue not to mark rescue_triggered, got {:?}", st_after_tick);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_bootstrap_rescue_fires_before_first_successful_topup() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(true))?;
    env.set_blackhole_armed_since_ts(Some(0))?;
    env.set_last_successful_transfer_ts(None)?;
    env.advance_time_and_tick(15 * 24 * 60 * 60, 5);
    env.rescue_tick()?;

    let st = env.state()?;
    if !st.rescue_triggered || st.forced_rescue_reason != Some(ForcedRescueReason::BootstrapNoSuccess) {
        bail!("expected bootstrap forced rescue to latch, got {:?}", st);
    }
    let mut controllers = env.controllers();
    controllers.sort_by_key(|p| p.to_text());
    let mut expected = vec![env.blackhole_controller, env.lifeline, env.faucet];
    expected.sort_by_key(|p| p.to_text());
    if controllers != expected {
        bail!("expected bootstrap rescue to widen controllers, got {controllers:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_init_args_preserve_expected_first_staking_tx_id() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.expected_first_staking_tx_id = Some(42);
    })?;

    let st = env.state()?;
    if st.expected_first_staking_tx_id != Some(42) {
        bail!("expected install arg expected_first_staking_tx_id to round-trip into state, got {:?}", st);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_correct_first_tx_anchor_stays_healthy() -> Result<()> {
    require_ignored_flag()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let expected_first_tx_id = 1u64;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.blackhole_armed = Some(true);
        init.expected_first_staking_tx_id = Some(expected_first_tx_id);
    })?;

    env.set_blackholed_controllers()?;
    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    let observed_first_tx_id = env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    if observed_first_tx_id != expected_first_tx_id {
        bail!("expected first mocked staking tx id {expected_first_tx_id}, got {observed_first_tx_id}");
    }

    env.main_tick()?;
    let st = env.state()?;
    if st.forced_rescue_reason.is_some() || st.consecutive_index_anchor_failures != 0 {
        bail!("expected matching first tx anchor to stay healthy, got {:?}", st);
    }
    if st.last_successful_transfer_ts.is_none() {
        bail!("expected matching first tx anchor scenario to complete a successful top-up, got {:?}", st);
    }
    let mut controllers = env.controllers();
    controllers.sort_by_key(|p| p.to_text());
    let mut expected = vec![env.blackhole_controller, env.faucet];
    expected.sort_by_key(|p| p.to_text());
    if controllers != expected {
        bail!("expected matching first tx anchor to keep healthy blackhole+self controllers, got {controllers:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_wrong_first_tx_anchor_latches_rescue_after_real_first_transfer() -> Result<()> {
    require_ignored_flag()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.blackhole_armed = Some(true);
        init.expected_first_staking_tx_id = Some(2);
    })?;

    env.set_blackholed_controllers()?;
    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    let observed_first_tx_id = env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    if observed_first_tx_id != 1 {
        bail!("expected first mocked staking tx id 1, got {observed_first_tx_id}");
    }

    env.main_tick()?;
    let st1 = env.state()?;
    if st1.consecutive_index_anchor_failures != 1 || st1.forced_rescue_reason.is_some() {
        bail!("expected first wrong-anchor observation to count once without latching rescue, got {:?}", st1);
    }

    env.main_tick()?;
    let st2 = env.state()?;
    if st2.forced_rescue_reason != Some(ForcedRescueReason::IndexAnchorMissing) {
        bail!("expected wrong first tx anchor to latch forced rescue after the second observation, got {:?}", st2);
    }
    let mut controllers = env.controllers();
    controllers.sort_by_key(|p| p.to_text());
    let mut expected = vec![env.blackhole_controller, env.lifeline, env.faucet];
    expected.sort_by_key(|p| p.to_text());
    if controllers != expected {
        bail!("expected wrong first tx anchor to widen controllers, got {controllers:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_anchor_failure_resets_if_observed_oldest_tx_heals_before_latch() -> Result<()> {
    require_ignored_flag()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;
    let env = FaucetEnv::new_with_init_overrides(|init| {
        init.blackhole_armed = Some(true);
        init.expected_first_staking_tx_id = Some(1);
    })?;

    env.set_blackholed_controllers()?;
    env.main_tick()?;
    let st1 = env.state()?;
    if st1.consecutive_index_anchor_failures != 1 || st1.forced_rescue_reason.is_some() {
        bail!("expected first missing-anchor observation before any transfer to count once, got {:?}", st1);
    }

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    let first_tx_id = env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    if first_tx_id != 1 {
        bail!("expected first mocked staking tx id 1, got {first_tx_id}");
    }

    env.main_tick()?;
    let st2 = env.state()?;
    if st2.consecutive_index_anchor_failures != 0 || st2.forced_rescue_reason.is_some() {
        bail!("expected anchor failure counter to reset once the observed oldest tx matches before latching rescue, got {:?}", st2);
    }
    if st2.last_successful_transfer_ts.is_none() {
        bail!("expected healed anchor scenario to complete a successful top-up, got {:?}", st2);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_missing_anchor_twice_latches_forced_rescue() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(true))?;
    env.set_expected_first_staking_tx_id(Some(1))?;

    env.main_tick()?;
    let st1 = env.state()?;
    if st1.consecutive_index_anchor_failures != 1 || st1.forced_rescue_reason.is_some() {
        bail!("expected first missing-anchor observation to count once, got {:?}", st1);
    }

    env.main_tick()?;
    let st2 = env.state()?;
    if st2.forced_rescue_reason != Some(ForcedRescueReason::IndexAnchorMissing) {
        bail!("expected missing anchor twice to latch forced rescue, got {:?}", st2);
    }
    let mut controllers = env.controllers();
    controllers.sort_by_key(|p| p.to_text());
    let mut expected = vec![env.blackhole_controller, env.lifeline, env.faucet];
    expected.sort_by_key(|p| p.to_text());
    if controllers != expected {
        bail!("expected anchor-loss forced rescue to widen controllers, got {controllers:?}");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_balance_change_without_new_latest_tx_twice_latches_forced_rescue() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(true))?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;
    env.main_tick()?;

    env.credit_staking(50_000_000)?;
    env.main_tick()?;
    let st1 = env.state()?;
    if st1.consecutive_index_latest_invariant_failures != 1 || st1.forced_rescue_reason.is_some() {
        bail!("expected first latest-invariant failure to count once, got {:?}", st1);
    }

    env.main_tick()?;
    let st2 = env.state()?;
    if st2.forced_rescue_reason != Some(ForcedRescueReason::IndexLatestInvariantBroken) {
        bail!("expected second latest-invariant failure to latch forced rescue, got {:?}", st2);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_two_zero_success_cmc_runs_latch_forced_rescue() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackholed_controllers()?;
    env.set_blackhole_armed(Some(true))?;
    env.set_cmc_fail(true)?;

    for _ in 0..2 {
        env.credit_payout(100_000_000)?;
        env.credit_staking(100_000_000)?;
        env.append_transfer(100_000_000, Some(Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?.to_text().into_bytes()))?;
        env.main_tick()?;
        env.advance_time_and_tick(61, 20);
        env.main_tick()?;
    }

    let st = env.state()?;
    if st.forced_rescue_reason != Some(ForcedRescueReason::CmcZeroSuccessRuns) || st.consecutive_cmc_zero_success_runs < 2 {
        bail!("expected two zero-success CMC runs to latch forced rescue, got {:?}", st);
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_reclaims_stale_main_lease_after_time_fast_forward() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;
    let target = Principal::from_text("22255-zqaaa-aaaas-qf6uq-cai")?;

    env.credit_payout(100_000_000)?;
    env.credit_staking(100_000_000)?;
    env.append_transfer(100_000_000, Some(target.to_text().into_bytes()))?;

    let now_secs = (env.pic.get_time().as_nanos_since_unix_epoch() / 1_000_000_000) as u64;
    env.set_main_lock_expires_at_ts(Some(now_secs + 30))?;
    env.main_tick()?;

    let st_before = env.state()?;
    if st_before.last_summary_present || st_before.active_payout_job_present {
        bail!("expected active lease to suppress the first main tick, got {:?}", st_before);
    }
    if !env.ledger_transfers()?.is_empty() || !env.notifications()?.is_empty() {
        bail!("expected no payout-side activity while the main lease is still active");
    }

    env.advance_time_and_tick(31, 5);
    env.main_tick()?;

    let summary = env
        .summary()
        .context("expected stale-lease retry to produce a summary")?;
    if summary.topped_up_count != 1 || summary.failed_topups != 0 {
        bail!(
            "expected stale-lease retry to complete one beneficiary top-up, got topped_up_count={} failed_topups={}",
            summary.topped_up_count,
            summary.failed_topups,
        );
    }
    if env.notifications()?.len() != 1 || env.ledger_transfers()?.len() != 1 {
        bail!("expected exactly one ledger transfer and one notify after stale-lease recovery");
    }

    Ok(())
}

#[test]
#[ignore]
fn faucet_forced_rescue_survives_upgrade_and_can_be_cleared() -> Result<()> {
    require_ignored_flag()?;
    let env = FaucetEnv::new()?;

    env.set_blackhole_armed(Some(true))?;
    env.set_expected_first_staking_tx_id(Some(1))?;
    env.main_tick()?;
    env.main_tick()?;
    let st1 = env.state()?;
    if st1.forced_rescue_reason != Some(ForcedRescueReason::IndexAnchorMissing) {
        bail!("expected forced rescue before upgrade, got {:?}", st1);
    }

    env.upgrade()?;
    let st2 = env.state()?;
    if st2.forced_rescue_reason != Some(ForcedRescueReason::IndexAnchorMissing) {
        bail!("expected forced rescue to survive upgrade, got {:?}", st2);
    }

    env.upgrade_with_args(FaucetUpgradeArg { blackhole_controller: None, blackhole_armed: None, clear_forced_rescue: Some(true) })?;
    let st3 = env.state()?;
    if st3.forced_rescue_reason.is_some()
        || st3.consecutive_index_anchor_failures != 0
        || st3.consecutive_index_latest_invariant_failures != 0
        || st3.consecutive_cmc_zero_success_runs != 0
    {
        bail!("expected forced rescue clear path to reset counters, got {:?}", st3);
    }

    Ok(())
}
