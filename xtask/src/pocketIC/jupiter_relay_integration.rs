#![allow(non_snake_case)]

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::{PocketIc, PocketIcBuilder};

#[path = "support/mod.rs"]
mod support;

use support::calls::{query_one, tick_n, update_bytes, update_noargs, update_one};

fn require_ignored_flag() -> Result<()> {
    support::assertions::require_ignored_flag()
}

fn build_wasm_cached(
    cache: &OnceLock<Vec<u8>>,
    package: &str,
    features: Option<&str>,
) -> Result<Vec<u8>> {
    let workspace_root = support::wasm::workspace_root_from_manifest(env!("CARGO_MANIFEST_DIR"))?;
    support::wasm::build_wasm_cached(&workspace_root, cache, package, features, None, false)
}

static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static BLACKHOLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn ledger_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&LEDGER_WASM, "mock-icrc-ledger", None)
}
fn cmc_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&CMC_WASM, "mock-cmc", None)
}
fn blackhole_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&BLACKHOLE_WASM, "mock-blackhole", None)
}
fn relay_wasm() -> Result<Vec<u8>> {
    build_wasm_cached(&RELAY_WASM, "jupiter-relay", Some("debug_api"))
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayInitArg {
    managed_canisters: Vec<Principal>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<u32>,
    raw_icp_mode: Option<RawIcpModeConfig>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayUpgradeArg {
    managed_canisters: Option<Vec<Principal>>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<Option<u32>>,
    raw_icp_mode: Option<Option<RawIcpModeConfig>>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RawIcpModeConfig {
    min_cycles_threshold: u128,
    recipients: Vec<RawIcpRecipient>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RawIcpRecipient {
    account: Account,
    memo: Option<Vec<u8>>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayMode {
    BaselineOnly,
    CyclesTopUp,
    RawIcp,
    Degraded,
    NoFunds,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelaySummary {
    mode: RelayMode,
    total_burn_cycles: u128,
    transfer_count: u32,
    ledger_transfer_count: u32,
    ledger_sent_e8s: u64,
    ledger_fees_e8s: u64,
    cmc_notify_success_count: u32,
    cmc_notify_failed_count: u32,
    cmc_notify_ambiguous_count: u32,
    planned_retained_e8s: u64,
    known_unspent_e8s: u64,
    ambiguous_e8s: u64,
    failed_transfers: u32,
    ambiguous_transfers: u32,
    partial_tick_count: u32,
    probe_failures: Vec<ProbeFailure>,
    canisters: Vec<CanisterBurnSample>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ProbeFailure {
    canister_id: Principal,
    error: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterBurnSample {
    canister_id: Principal,
    burn_cycles: u128,
    weight: u128,
    gross_share_e8s: u64,
    amount_e8s: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct NotifyRecord {
    canister_id: Principal,
    block_index: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNotifyBehavior {
    Ok,
    Processing,
    Other {
        error_code: u64,
        error_message: String,
    },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum DebugNextTransferError {
    TemporarilyUnavailable,
    Duplicate { duplicate_of: u64 },
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

#[derive(Clone, Debug, CandidType, Deserialize)]
struct DebugState {
    active_job_present: bool,
}

struct RelayEnv {
    pic: PocketIc,
    ledger: Principal,
    cmc: Principal,
    blackhole: Principal,
    relay: Principal,
}

impl RelayEnv {
    fn new(max_transfers_per_tick: Option<u32>) -> Result<Self> {
        Self::new_with_config(max_transfers_per_tick, |_, cmc, _, _| (vec![cmc], None))
    }

    fn new_with_config<F>(max_transfers_per_tick: Option<u32>, config: F) -> Result<Self>
    where
        F: FnOnce(
            Principal,
            Principal,
            Principal,
            Principal,
        ) -> (Vec<Principal>, Option<RawIcpModeConfig>),
    {
        let pic = PocketIcBuilder::new().with_application_subnet().build();
        let ledger = pic.create_canister();
        let cmc = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        for canister in [ledger, cmc, blackhole, relay] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        let (managed_canisters, raw_icp_mode) = config(ledger, cmc, blackhole, relay);
        let init = RelayInitArg {
            managed_canisters,
            ledger_canister_id: Some(ledger),
            cmc_canister_id: Some(cmc),
            blackhole_canister_id: Some(blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick,
            raw_icp_mode,
        };
        pic.install_canister(relay, relay_wasm()?, encode_one(init)?, None);
        Ok(Self {
            pic,
            ledger,
            cmc,
            blackhole,
            relay,
        })
    }

    fn set_managed_cycles(&self, cycles: u128) -> Result<()> {
        self.set_canister_cycles(self.cmc, cycles)
    }

    fn set_canister_cycles(&self, canister: Principal, cycles: u128) -> Result<()> {
        let _: () = update_bytes(
            &self.pic,
            self.blackhole,
            Principal::anonymous(),
            "debug_set_status",
            encode_args((canister, Some(Nat::from(cycles)), vec![self.blackhole]))?,
        )?;
        Ok(())
    }

    fn credit_relay(&self, amount_e8s: u64) -> Result<()> {
        let relay_account = Account {
            owner: self.relay,
            subaccount: None,
        };
        let _: () = update_bytes(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_credit",
            encode_args((relay_account, amount_e8s))?,
        )?;
        Ok(())
    }

    fn add_relay_cycles(&self, cycles: u128) {
        self.pic.add_cycles(self.relay, cycles);
    }

    fn tick_relay(&self) -> Result<RelaySummary> {
        let _: () = update_noargs(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_main_tick",
        )?;
        tick_n(&self.pic, 5);
        self.summary()
    }

    fn summary(&self) -> Result<RelaySummary> {
        let summary: Option<RelaySummary> = query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_last_summary",
            (),
        )?;
        summary.context("expected relay summary")
    }

    fn logs_text(&self) -> Result<String> {
        let records = self
            .pic
            .fetch_canister_logs(self.relay, Principal::anonymous())
            .map_err(|e| anyhow::anyhow!("fetch_canister_logs reject: {e:?}"))?;
        Ok(records
            .iter()
            .map(|record| String::from_utf8_lossy(&record.content).into_owned())
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn transfers(&self) -> Result<Vec<TransferRecord>> {
        query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_transfers",
            (),
        )
    }

    fn notifications(&self) -> Result<Vec<NotifyRecord>> {
        query_one(
            &self.pic,
            self.cmc,
            Principal::anonymous(),
            "debug_notifications",
            (),
        )
    }

    fn balance_of(&self, account: Account) -> Result<u64> {
        let balance: Nat = query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "icrc1_balance_of",
            account,
        )?;
        Ok(nat_to_u64(&balance))
    }

    fn set_trap_after_successful_transfer(&self, value: bool) -> Result<()> {
        update_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_trap_after_successful_transfer",
            value,
        )
    }

    fn set_cmc_script(&self, script: Vec<DebugNotifyBehavior>) -> Result<()> {
        update_one(
            &self.pic,
            self.cmc,
            Principal::anonymous(),
            "debug_set_script",
            script,
        )
    }

    fn set_ledger_error_script(&self, script: Vec<DebugNextTransferError>) -> Result<()> {
        update_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "debug_set_error_script",
            script,
        )
    }

    fn debug_state(&self) -> Result<DebugState> {
        query_one(
            &self.pic,
            self.relay,
            Principal::anonymous(),
            "debug_state",
            (),
        )
    }

    fn advance_time_and_tick(&self, secs: u64, ticks: usize) {
        self.pic.advance_time(Duration::from_secs(secs));
        tick_n(&self.pic, ticks);
    }

    fn upgrade_relay_without_config_changes(&self) -> Result<()> {
        self.pic
            .upgrade_canister(
                self.relay,
                relay_wasm()?,
                encode_one(Option::<RelayUpgradeArg>::None)?,
                Some(Principal::anonymous()),
            )
            .map_err(|e| anyhow::anyhow!("upgrade_canister reject: {e:?}"))?;
        Ok(())
    }
}

fn nat_to_u64(value: &Nat) -> u64 {
    value.0.to_string().parse().unwrap_or(u64::MAX)
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
fn baseline_then_weighted_cmc_topup_records_real_async_notify() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_managed_cycles(5_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::CyclesTopUp
        || topup.ledger_transfer_count == 0
        || topup.cmc_notify_success_count == 0
    {
        bail!("expected successful cycles top-up after baseline, got {topup:?}");
    }
    let notifications: Vec<NotifyRecord> = query_one(
        &env.pic,
        env.cmc,
        Principal::anonymous(),
        "debug_notifications",
        (),
    )?;
    if notifications
        .iter()
        .all(|notification| notification.canister_id != env.cmc)
    {
        bail!("expected CMC notification for managed canister, got {notifications:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("Cycles:")
        || !logs.contains("CONFIG ")
        || !logs.contains("RELAY_SUMMARY mode=CyclesTopUp")
        || !logs.contains("RELAY_CANISTER ")
        || !logs.contains("burn_cycles=")
        || !logs.contains("gross_share_e8s=")
        || !logs.contains("amount_e8s=")
    {
        bail!("expected public relay logs for cycles top-up, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn weighted_cmc_topup_prefers_higher_burn_managed_canister() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| (vec![ledger, cmc], None))?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_canister_cycles(env.ledger, 9_000_000_000_000)?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::CyclesTopUp || summary.cmc_notify_success_count < 2 {
        bail!("expected successful multi-canister cycles top-up, got {summary:?}");
    }

    let ledger_sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.ledger)
        .context("missing ledger managed-canister allocation")?;
    let cmc_sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing CMC managed-canister allocation")?;
    if cmc_sample.gross_share_e8s <= ledger_sample.gross_share_e8s {
        bail!("expected higher-burn CMC canister to receive larger gross share: ledger={ledger_sample:?} cmc={cmc_sample:?}");
    }
    let logs = env.logs_text()?;
    let cmc_log_fragment = format!(
        "RELAY_CANISTER canister_id={} previous_cycles=",
        env.cmc.to_text()
    );
    if !logs.contains(&cmc_log_fragment)
        || !logs.contains(&format!("burn_cycles={}", cmc_sample.burn_cycles))
        || !logs.contains(&format!("gross_share_e8s={}", cmc_sample.gross_share_e8s))
        || !logs.contains(&format!("amount_e8s={}", cmc_sample.amount_e8s))
    {
        bail!(
            "expected public logs to include CMC burn/allocation sample {cmc_sample:?}, got {logs}"
        );
    }

    let ledger_subaccount = principal_to_subaccount(env.ledger);
    let cmc_subaccount = principal_to_subaccount(env.cmc);
    let transfers = env.transfers()?;
    let ledger_transfer = transfers
        .iter()
        .find(|transfer| {
            transfer.to.owner == env.cmc && transfer.to.subaccount == Some(ledger_subaccount)
        })
        .context("missing transfer to ledger canister CMC deposit account")?;
    let cmc_transfer = transfers
        .iter()
        .find(|transfer| {
            transfer.to.owner == env.cmc && transfer.to.subaccount == Some(cmc_subaccount)
        })
        .context("missing transfer to CMC canister CMC deposit account")?;
    if nat_to_u64(&cmc_transfer.amount) <= nat_to_u64(&ledger_transfer.amount) {
        bail!("expected higher-burn CMC transfer amount to exceed lower-burn ledger amount, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_canister_with_increased_cycles_gets_zero_weight_when_others_burned() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| (vec![ledger, cmc], None))?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 8_000_000_000_000)?;
    env.set_managed_cycles(12_000_000_000_000)?;
    let summary = env.tick_relay()?;
    let burned = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.ledger)
        .context("missing burned canister sample")?;
    let gained = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing gained canister sample")?;
    if burned.amount_e8s == 0
        || gained.burn_cycles != 0
        || gained.weight != 0
        || gained.gross_share_e8s != 0
        || gained.amount_e8s != 0
    {
        bail!("expected gained canister to receive zero weight/share while another burned: {summary:?}");
    }
    let notifications = env.notifications()?;
    if notifications
        .iter()
        .any(|notification| notification.canister_id == env.cmc)
    {
        bail!("expected no notify for gained canister, got {notifications:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_splits_equally_when_no_canister_burned_cycles() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| (vec![ledger, cmc], None))?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(99_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::CyclesTopUp {
        bail!("expected cycles top-up mode, got {summary:?}");
    }
    let managed = summary
        .canisters
        .iter()
        .filter(|sample| sample.canister_id == env.ledger || sample.canister_id == env.cmc)
        .collect::<Vec<_>>();
    if managed.len() != 2 || managed.iter().any(|sample| sample.weight != 1) {
        bail!("expected equal unit weights for zero-burn managed canisters, got {summary:?}");
    }
    let diff = managed[0]
        .gross_share_e8s
        .abs_diff(managed[1].gross_share_e8s);
    if diff > 1 {
        bail!("expected equal gross shares subject only to flooring, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn raw_icp_mode_transfers_external_and_self_subaccount_without_cmc_notify() -> Result<()> {
    require_ignored_flag()?;
    let self_subaccount = [7_u8; 32];
    let external_memo = vec![0xA1, 0xB2];
    let self_sub_memo = vec![0xC3];
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(RawIcpModeConfig {
                min_cycles_threshold: 1_000_000_000_000,
                recipients: vec![
                    RawIcpRecipient {
                        account: Account {
                            owner: cmc,
                            subaccount: None,
                        },
                        memo: Some(external_memo.clone()),
                    },
                    RawIcpRecipient {
                        account: Account {
                            owner: relay,
                            subaccount: Some(self_subaccount),
                        },
                        memo: Some(self_sub_memo.clone()),
                    },
                    RawIcpRecipient {
                        account: Account {
                            owner: relay,
                            subaccount: None,
                        },
                        memo: None,
                    },
                ],
            }),
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before raw mode transfer, got {baseline:?}");
    }

    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::RawIcp
        || summary.ledger_transfer_count != 2
        || summary.cmc_notify_success_count != 0
    {
        bail!("expected raw ICP mode to make exactly two ledger transfers and no CMC notify, got {summary:?}");
    }
    let notifications = env.notifications()?;
    if !notifications.is_empty() {
        bail!("expected raw ICP mode to skip CMC notify entirely, got {notifications:?}");
    }

    let transfers = env.transfers()?;
    if !transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.cmc,
                subaccount: None,
            })
            && transfer.memo == Some(external_memo.clone())
    }) {
        bail!(
            "expected external raw ICP recipient transfer with configured memo, got {transfers:?}"
        );
    }
    if !transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.relay,
                subaccount: Some(self_subaccount),
            })
            && transfer.memo == Some(self_sub_memo.clone())
    }) {
        bail!("expected relay self-subaccount raw ICP transfer with configured memo, got {transfers:?}");
    }
    if transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.relay,
                subaccount: None,
            })
    }) {
        bail!("expected relay default raw ICP recipient to be retained without transfer, got {transfers:?}");
    }
    let retained = env.balance_of(Account {
        owner: env.relay,
        subaccount: None,
    })?;
    if retained != 33_000_000 {
        bail!("expected relay default account to retain one gross share, got {retained}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=RawIcp")
        || !logs.contains("RELAY_RAW_RECIPIENT ")
        || !logs.contains("retained_self=true")
        || !logs.contains("memo=a1b2")
        || !logs.contains("memo=c3")
    {
        bail!("expected public raw ICP recipient logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_does_not_switch_to_raw_icp_when_min_cycles_equals_threshold() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(RawIcpModeConfig {
                min_cycles_threshold: 5_000_000_000_000,
                recipients: vec![RawIcpRecipient {
                    account: Account {
                        owner: relay,
                        subaccount: None,
                    },
                    memo: None,
                }],
            }),
        )
    })?;
    env.set_managed_cycles(5_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::CyclesTopUp || summary.cmc_notify_success_count == 0 {
        bail!("expected exact threshold to stay in CMC mode, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_raw_mode_is_per_tick_not_permanent_latch() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(RawIcpModeConfig {
                min_cycles_threshold: 5_000_000_000_000,
                recipients: vec![RawIcpRecipient {
                    account: Account {
                        owner: relay,
                        subaccount: None,
                    },
                    memo: None,
                }],
            }),
        )
    })?;
    env.set_managed_cycles(6_000_000_000_000)?;
    env.add_relay_cycles(2_000_000_000_000);
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_managed_cycles(6_000_000_000_000)?;
    let raw = env.tick_relay()?;
    if raw.mode != RelayMode::RawIcp {
        bail!("expected raw mode while all cycles are above threshold, got {raw:?}");
    }

    env.credit_relay(100_000_000)?;
    env.set_managed_cycles(4_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::CyclesTopUp || topup.cmc_notify_success_count == 0 {
        bail!("expected later below-threshold tick to return to CMC mode, got {topup:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn fail_closed_blackhole_probe_failure_spends_nothing() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.credit_relay(100_000_000)?;

    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::Degraded
        || summary.probe_failures.is_empty()
        || summary.ledger_transfer_count != 0
    {
        bail!("expected degraded no-spend summary after missing blackhole status, got {summary:?}");
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    if !transfers.is_empty() {
        bail!("expected no ledger transfers when blackhole probe fails, got {transfers:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=Degraded") || !logs.contains("RELAY_PROBE_FAILURE ") {
        bail!("expected public degraded probe failure logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn cmc_processing_is_retried_without_double_spending_ledger() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_cmc_script(vec![
        DebugNotifyBehavior::Processing,
        DebugNotifyBehavior::Ok,
    ])?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.cmc_notify_success_count != 1
        || summary.cmc_notify_ambiguous_count != 0
    {
        bail!("expected one ledger transfer and successful retry after CMC Processing, got {summary:?}");
    }
    let transfers: Vec<TransferRecord> = query_one(
        &env.pic,
        env.ledger,
        Principal::anonymous(),
        "debug_transfers",
        (),
    )?;
    if transfers.len() != 1 {
        bail!("expected CMC Processing retry not to duplicate ledger transfers, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_marks_cmc_repeated_retryable_notify_as_ambiguous() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_cmc_script(vec![
        DebugNotifyBehavior::Processing,
        DebugNotifyBehavior::Other {
            error_code: 1,
            error_message: "still processing".to_string(),
        },
    ])?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.cmc_notify_ambiguous_count != 1
        || summary.ambiguous_e8s == 0
    {
        bail!("expected accepted ledger spend with ambiguous repeated CMC uncertainty, got {summary:?}");
    }
    if env.transfers()?.len() != 1 {
        bail!("expected no changed-identity retry after CMC ambiguity");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_treats_ledger_duplicate_as_accepted_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_ledger_error_script(vec![DebugNextTransferError::Duplicate { duplicate_of: 77 }])?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 1
        || summary.failed_transfers != 0
        || summary.cmc_notify_success_count != 1
    {
        bail!("expected duplicate ledger response to count as accepted transfer, got {summary:?}");
    }
    let notifications = env.notifications()?;
    if notifications.len() != 1 || notifications[0].block_index != 77 {
        bail!("expected CMC notify with duplicate block index, got {notifications:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_respects_max_transfers_per_tick_and_resumes_active_job() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(Some(1), |ledger, cmc, _, _| (vec![ledger, cmc], None))?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 8_000_000_000_000)?;
    env.set_managed_cycles(8_000_000_000_000)?;
    let _ = env.tick_relay()?;
    if env.transfers()?.len() != 1 || !env.debug_state()?.active_job_present {
        bail!("expected first allocation tick to start one transfer and keep active job");
    }

    let mut previous_transfer_count = env.transfers()?.len();
    let mut summary = env.summary()?;
    for _ in 0..5 {
        summary = env.tick_relay()?;
        let current_transfer_count = env.transfers()?.len();
        if current_transfer_count.saturating_sub(previous_transfer_count) > 1 {
            bail!("expected at most one new transfer per tick with limit=1, got previous={previous_transfer_count} current={current_transfer_count}");
        }
        previous_transfer_count = current_transfer_count;
        if !env.debug_state()?.active_job_present {
            break;
        }
    }
    if summary.mode != RelayMode::CyclesTopUp
        || summary.partial_tick_count == 0
        || summary.ledger_transfer_count < 2
        || env.debug_state()?.active_job_present
    {
        bail!("expected later tick to resume and complete transfer-limited job, got {summary:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=CyclesTopUp") || !logs.contains("partial_tick_count=") {
        bail!("expected transfer-limit public summary logs with partial_tick_count, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn upgrade_after_trap_following_ledger_transfer_recovers_without_double_spend() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_managed_cycles(5_000_000_000_000)?;
    env.set_trap_after_successful_transfer(true)?;
    let trapped = update_noargs::<()>(
        &env.pic,
        env.relay,
        Principal::anonymous(),
        "debug_main_tick",
    );
    if trapped.is_ok() {
        bail!("expected debug_main_tick to reject after injected relay transfer trap");
    }
    tick_n(&env.pic, 10);

    let transfers_mid = env.transfers()?;
    if transfers_mid.len() != 1 {
        bail!("expected exactly one ledger transfer to land before upgrade, got {transfers_mid:?}");
    }
    if !env.notifications()?.is_empty() {
        bail!("expected trap before accepted-transfer persistence to leave no CMC notifications before upgrade");
    }
    let st_mid = env.debug_state()?;
    if !st_mid.active_job_present {
        bail!("expected interrupted relay job to remain active before upgrade");
    }

    env.upgrade_relay_without_config_changes()?;
    let st_after_upgrade = env.debug_state()?;
    if !st_after_upgrade.active_job_present {
        bail!("expected interrupted relay job to remain active immediately after upgrade");
    }

    env.advance_time_and_tick(1, 20);

    let transfers_after = env.transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected post-upgrade recovery to reuse duplicate ledger transfer without double spend, got {transfers_after:?}");
    }
    let notifications = env.notifications()?;
    if notifications.len() != 1 || notifications[0].canister_id != env.cmc {
        bail!("expected one successful CMC notify after upgrade recovery, got {notifications:?}");
    }
    let summary = env.summary()?;
    if summary.mode != RelayMode::CyclesTopUp
        || summary.cmc_notify_success_count != 1
        || summary.ledger_transfer_count != 1
    {
        bail!("unexpected relay summary after upgrade recovery: {summary:?}");
    }
    let st_done = env.debug_state()?;
    if st_done.active_job_present {
        bail!("expected post-upgrade recovery to complete the interrupted relay job");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=CyclesTopUp")
        || !logs.contains("ledger_transfer_count=1")
        || !logs.contains("cmc_notify_success_count=1")
    {
        bail!("expected coherent public summary after upgrade recovery, got {logs}");
    }
    Ok(())
}
