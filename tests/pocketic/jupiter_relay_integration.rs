#![allow(non_snake_case)]

use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use candid::{encode_args, encode_one, CandidType, Deserialize, Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use pocket_ic::PocketIc;

#[path = "support/mod.rs"]
mod support;

use support::account_identifier::principal_to_subaccount;
use support::calls::{query_one, tick_n, update_bytes, update_noargs, update_one};

fn require_ignored_flag() -> Result<()> {
    support::assertions::require_ignored_flag()
}

fn principal(text: &str) -> Principal {
    Principal::from_text(text).unwrap()
}

static LEDGER_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static CMC_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static GOVERNANCE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static BLACKHOLE_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_WASM: OnceLock<Vec<u8>> = OnceLock::new();
static RELAY_PROD_WASM: OnceLock<Vec<u8>> = OnceLock::new();

fn ledger_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&LEDGER_WASM, "mock-icrc-ledger", None)
}
fn cmc_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&CMC_WASM, "mock-cmc", None)
}
fn governance_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&GOVERNANCE_WASM, "mock-nns-governance", None)
}
fn blackhole_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&BLACKHOLE_WASM, "mock-blackhole", None)
}
fn relay_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&RELAY_WASM, "jupiter-relay", Some("debug_api"))
}
fn relay_prod_wasm() -> Result<Vec<u8>> {
    support::wasm::build_wasm_cached_for_test(&RELAY_PROD_WASM, "jupiter-relay", None)
}

fn wasm_contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayInitArg {
    managed_canisters: Vec<Principal>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<u32>,
    surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    surplus_neuron_recipients: Vec<SurplusNeuronRecipient>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct RelayUpgradeArg {
    managed_canisters: Option<Vec<Principal>>,
    ledger_canister_id: Option<Principal>,
    cmc_canister_id: Option<Principal>,
    governance_canister_id: Option<Principal>,
    blackhole_canister_id: Option<Principal>,
    main_interval_seconds: Option<u64>,
    max_transfers_per_tick: Option<Option<u32>>,
    surplus_canister_recipients: Option<Vec<SurplusCanisterRecipient>>,
    surplus_neuron_recipients: Option<Vec<SurplusNeuronRecipient>>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SurplusCanisterRecipient {
    canister_id: Principal,
    memo: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SurplusNeuronRecipient {
    neuron_id: u64,
    memo: Vec<u8>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum RelayMode {
    BaselineOnly,
    TopUpThenSurplus,
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
    conversion_estimate_used: Option<ConversionEstimate>,
    surplus_transfers: Vec<SurplusTransferSample>,
    skipped_surplus_reason: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ConversionEstimate {
    cycles_per_e8: u128,
    timestamp_nanos: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ProbeFailure {
    canister_id: Principal,
    error: String,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct CanisterBurnSample {
    canister_id: Principal,
    previous_cycles: Option<u128>,
    current_cycles: u128,
    relay_minted_cycles: u128,
    burn_cycles: u128,
    carried_deficit_cycles: u128,
    target_topup_cycles: u128,
    gross_share_e8s: u64,
    amount_e8s: u64,
    actual_minted_cycles: u128,
    remaining_deficit_cycles: u128,
    skipped_reason: Option<String>,
}

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
enum SurplusTarget {
    Canister(Principal),
    Neuron(u64),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct SurplusTransferSample {
    target: SurplusTarget,
    account: Account,
    gross_share_e8s: u64,
    amount_e8s: u64,
    memo_len: Option<u32>,
    skipped_reason: Option<String>,
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
    governance: Principal,
    blackhole: Principal,
    relay: Principal,
}

impl RelayEnv {
    fn new(max_transfers_per_tick: Option<u32>) -> Result<Self> {
        Self::new_with_config(max_transfers_per_tick, |_, cmc, _, _| {
            (vec![cmc], None, Vec::new())
        })
    }

    fn new_with_config<F>(max_transfers_per_tick: Option<u32>, config: F) -> Result<Self>
    where
        F: FnOnce(
            Principal,
            Principal,
            Principal,
            Principal,
        ) -> (
            Vec<Principal>,
            Option<Vec<SurplusCanisterRecipient>>,
            Vec<SurplusNeuronRecipient>,
        ),
    {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let ledger = pic.create_canister();
        let cmc = pic.create_canister();
        let governance = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        for canister in [ledger, cmc, governance, blackhole, relay] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(governance, governance_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        let (managed_canisters, surplus_canister_recipients, surplus_neuron_recipients) =
            config(ledger, cmc, blackhole, relay);
        let init = RelayInitArg {
            managed_canisters,
            ledger_canister_id: Some(ledger),
            cmc_canister_id: Some(cmc),
            governance_canister_id: Some(governance),
            blackhole_canister_id: Some(blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick,
            surplus_canister_recipients,
            surplus_neuron_recipients,
        };
        pic.install_canister(relay, relay_wasm()?, encode_one(init)?, None);
        Ok(Self {
            pic,
            ledger,
            cmc,
            governance,
            blackhole,
            relay,
        })
    }

    fn new_with_production_blackholes_managed() -> Result<Self> {
        let pic = support::pocketic::builder()
            .with_application_subnet()
            .build();
        let ledger = pic.create_canister();
        let cmc = pic.create_canister();
        let governance = pic.create_canister();
        let blackhole = pic.create_canister();
        let relay = pic.create_canister();
        let fiduciary_blackhole = principal("77deu-baaaa-aaaar-qb6za-cai");
        let thirteen_node_blackhole = principal("e3mmv-5qaaa-aaaah-aadma-cai");
        pic.create_canister_with_id(None, None, fiduciary_blackhole)
            .map_err(anyhow::Error::msg)?;
        pic.create_canister_with_id(None, None, thirteen_node_blackhole)
            .map_err(anyhow::Error::msg)?;

        for canister in [
            ledger,
            cmc,
            governance,
            blackhole,
            relay,
            fiduciary_blackhole,
            thirteen_node_blackhole,
        ] {
            pic.add_cycles(canister, 5_000_000_000_000);
        }
        pic.install_canister(ledger, ledger_wasm()?, vec![], None);
        pic.install_canister(cmc, cmc_wasm()?, vec![], None);
        pic.install_canister(governance, governance_wasm()?, vec![], None);
        pic.install_canister(blackhole, blackhole_wasm()?, vec![], None);
        pic.install_canister(fiduciary_blackhole, blackhole_wasm()?, vec![], None);
        pic.install_canister(thirteen_node_blackhole, blackhole_wasm()?, vec![], None);

        let init = RelayInitArg {
            managed_canisters: vec![cmc, fiduciary_blackhole, thirteen_node_blackhole],
            ledger_canister_id: Some(ledger),
            cmc_canister_id: Some(cmc),
            governance_canister_id: Some(governance),
            blackhole_canister_id: Some(blackhole),
            main_interval_seconds: Some(31_536_000),
            max_transfers_per_tick: None,
            surplus_canister_recipients: None,
            surplus_neuron_recipients: Vec::new(),
        };
        pic.install_canister(relay, relay_wasm()?, encode_one(init)?, None);

        for (probe, target, cycles) in [
            (blackhole, cmc, 10_000_000_000_000_u128),
            (
                fiduciary_blackhole,
                fiduciary_blackhole,
                20_000_000_000_000_u128,
            ),
            (
                thirteen_node_blackhole,
                thirteen_node_blackhole,
                30_000_000_000_000_u128,
            ),
        ] {
            let _: () = update_bytes(
                &pic,
                probe,
                Principal::anonymous(),
                "debug_set_status",
                encode_args((target, Some(Nat::from(cycles)), vec![probe]))?,
            )?;
        }

        Ok(Self {
            pic,
            ledger,
            cmc,
            governance,
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

    fn credit_relay_subaccount_one(&self, amount_e8s: u64) -> Result<()> {
        let relay_account = Account {
            owner: self.relay,
            subaccount: Some(relay_subaccount_one()),
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

    fn relay_balance(&self) -> Result<u64> {
        let balance: Nat = query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "icrc1_balance_of",
            Account {
                owner: self.relay,
                subaccount: None,
            },
        )?;
        Ok(nat_to_u64(&balance))
    }

    fn relay_subaccount_one_balance(&self) -> Result<u64> {
        let balance: Nat = query_one(
            &self.pic,
            self.ledger,
            Principal::anonymous(),
            "icrc1_balance_of",
            Account {
                owner: self.relay,
                subaccount: Some(relay_subaccount_one()),
            },
        )?;
        Ok(nat_to_u64(&balance))
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

    fn claim_or_refresh_calls(&self) -> Result<u64> {
        query_one(
            &self.pic,
            self.governance,
            Principal::anonymous(),
            "debug_get_claim_or_refresh_calls",
            (),
        )
    }

    fn set_claim_or_refresh_fails(&self, value: bool) -> Result<()> {
        update_one(
            &self.pic,
            self.governance,
            Principal::anonymous(),
            "debug_set_claim_or_refresh_fails",
            value,
        )
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

fn neuron_subaccount(neuron_id: u64) -> [u8; 32] {
    let mut account = [0u8; 32];
    account[24..].copy_from_slice(&neuron_id.to_be_bytes());
    account
}

fn relay_subaccount_one() -> [u8; 32] {
    let mut subaccount = [0u8; 32];
    subaccount[31] = 1;
    subaccount
}

#[test]
#[ignore]
fn relay_production_wasm_does_not_export_status_or_admin_endpoints() -> Result<()> {
    require_ignored_flag()?;
    let wasm = relay_prod_wasm()?;
    let removed_debug_marker = b"relay_"
        .iter()
        .chain(b"status")
        .copied()
        .collect::<Vec<_>>();
    let removed_debug_query_marker = b"canister_query "
        .iter()
        .chain(removed_debug_marker.iter())
        .copied()
        .collect::<Vec<_>>();
    for needle in [
        removed_debug_query_marker.as_slice(),
        b"canister_update admin_schedule_main_tick_now".as_slice(),
        removed_debug_marker.as_slice(),
        b"admin_schedule_main_tick_now".as_slice(),
    ] {
        if wasm_contains(&wasm, needle) {
            bail!(
                "production relay Wasm unexpectedly contains exported endpoint marker `{}`",
                String::from_utf8_lossy(needle)
            );
        }
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_debug_wasm_does_not_export_status_or_admin_endpoints() -> Result<()> {
    require_ignored_flag()?;
    let wasm = relay_wasm()?;
    let removed_debug_marker = b"relay_"
        .iter()
        .chain(b"status")
        .copied()
        .collect::<Vec<_>>();
    let removed_debug_query_marker = b"canister_query "
        .iter()
        .chain(removed_debug_marker.iter())
        .copied()
        .collect::<Vec<_>>();
    for needle in [
        removed_debug_query_marker.as_slice(),
        removed_debug_marker.as_slice(),
        b"canister_update admin_schedule_main_tick_now".as_slice(),
        b"admin_schedule_main_tick_now".as_slice(),
    ] {
        if wasm_contains(&wasm, needle) {
            bail!(
                "debug relay Wasm unexpectedly contains status/admin endpoint marker `{}`",
                String::from_utf8_lossy(needle)
            );
        }
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_forwards_without_default_account_funds() -> Result<()> {
    require_ignored_flag()?;
    let jupiter_faucet_neuron = 11_614_578_985_374_291_210_u64;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to avoid ledger work, got {summary:?}");
    }
    if !env.notifications()?.is_empty() {
        bail!("expected no CMC notification for subaccount-1 forwarding");
    }

    let transfers = env.transfers()?;
    if transfers.len() != 1 {
        bail!("expected exactly one subaccount-1 transfer, got {transfers:?}");
    }
    let transfer = &transfers[0];
    if transfer.from
        != (Account {
            owner: env.relay,
            subaccount: Some(relay_subaccount_one()),
        })
    {
        bail!("expected transfer source to be Relay subaccount 1, got {transfer:?}");
    }
    if transfer.to
        != (Account {
            owner: env.governance,
            subaccount: Some(neuron_subaccount(jupiter_faucet_neuron)),
        })
    {
        bail!("expected transfer destination to be Jupiter Faucet neuron staking account, got {transfer:?}");
    }
    if nat_to_u64(&transfer.amount) != 100_000_000 || nat_to_u64(&transfer.fee) != 10_000 {
        bail!("expected transfer to send balance minus fee, got {transfer:?}");
    }
    let expected_memo = format!("{}.Relay", env.relay.to_text().replace('-', "")).into_bytes();
    if transfer.memo.as_deref() != Some(expected_memo.as_slice()) {
        bail!("expected compact Relay Faucet memo, got {transfer:?}");
    }
    if env.claim_or_refresh_calls()? != 1 {
        bail!("expected one Jupiter Faucet neuron claim_or_refresh call");
    }
    if env.relay_balance()? != 0 || env.relay_subaccount_one_balance()? != 0 {
        bail!(
            "expected default and subaccount-1 balances to be zero after transfer, default={} sub1={}",
            env.relay_balance()?,
            env.relay_subaccount_one_balance()?
        );
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("amount_e8s=100000000")
        || !logs.contains("memo_len=")
        || logs.contains(&String::from_utf8(expected_memo).unwrap())
    {
        bail!("expected faucet commitment log without raw memo bytes, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_waits_until_one_icp_net() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_009_999)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || !env.transfers()?.is_empty() {
        bail!("expected no transfer below 1 ICP net threshold, got {summary:?}");
    }
    if env.relay_subaccount_one_balance()? != 100_009_999 {
        bail!(
            "expected subaccount-1 balance to remain accumulated, got {}",
            env.relay_subaccount_one_balance()?
        );
    }
    let logs = env.logs_text()?;
    if logs.contains("skipped_reason=subaccount_1_below_1_icp_net") {
        bail!("expected below-threshold subaccount-1 scan to stay out of repeated public logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_no_funds_is_quiet_without_skip_log() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || !env.transfers()?.is_empty() {
        bail!("expected no transfer with empty subaccount-1, got {summary:?}");
    }

    let logs = env.logs_text()?;
    if logs.contains("skipped_reason=subaccount_1_no_funds") {
        bail!("expected no-funds scan to stay out of repeated public logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_treats_ledger_duplicate_as_accepted() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;
    env.set_ledger_error_script(vec![DebugNextTransferError::Duplicate { duplicate_of: 77 }])?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to stay idle, got {summary:?}");
    }
    if env.claim_or_refresh_calls()? != 1 {
        bail!("expected duplicate response to be accepted and followed by claim_or_refresh");
    }
    if !env.transfers()?.is_empty() {
        bail!("mock duplicate response should not create a second ledger transfer record");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("amount_e8s=100000000")
        || !logs.contains("skipped_reason=null")
    {
        bail!("expected accepted duplicate faucet commitment log, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn subaccount_one_commitment_refresh_failure_does_not_duplicate_transfer() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay_subaccount_one(100_010_000)?;
    env.set_claim_or_refresh_fails(true)?;

    let summary = env.tick_relay()?;
    if summary.ledger_transfer_count != 0 || summary.cmc_notify_success_count != 0 {
        bail!("expected default-account job to stay idle, got {summary:?}");
    }
    let transfers = env.transfers()?;
    if transfers.len() != 1 {
        bail!("expected exactly one ledger transfer despite refresh failure, got {transfers:?}");
    }

    let _ = env.tick_relay()?;
    let transfers_after = env.transfers()?;
    if transfers_after.len() != 1 {
        bail!("expected no duplicate transfer on later tick after refresh failure, got {transfers_after:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_FAUCET_COMMITMENT ")
        || !logs.contains("skipped_reason=null")
        || !logs.contains("relay ERR message=faucet%20commitment%20neuron%20refresh%20failed")
    {
        bail!("expected accepted transfer and logged follow-up refresh failure, got {logs}");
    }
    let summary_pos = logs
        .find("RELAY_SUMMARY ")
        .context("expected allocation-job summary log")?;
    let refresh_failure_pos = logs
        .find("relay ERR message=faucet%20commitment%20neuron%20refresh%20failed")
        .context("expected scheduled refresh failure log")?;
    if refresh_failure_pos < summary_pos {
        bail!(
            "expected faucet commitment refresh failure to be logged after allocation-job summary, got {logs}"
        );
    }
    Ok(())
}

#[test]
#[ignore]
fn baseline_then_headroom_cmc_topup_records_real_async_notify() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_managed_cycles(5_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::TopUpThenSurplus
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
        || !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("RELAY_CANISTER ")
        || !logs.contains("burn_cycles=")
        || !logs.contains("planned_topup_e8s=")
        || !logs.contains("actual_topup_e8s=")
    {
        bail!("expected public relay logs for cycles top-up, got {logs}");
    }
    if logs.contains("relay INFO ") {
        bail!("relay INFO logs should not be emitted, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn no_raw_recipients_routes_all_spendable_icp_as_cycles() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.cmc_notify_success_count == 0
        || !summary.surplus_transfers.is_empty()
        || summary.skipped_surplus_reason.as_deref() != Some("no_raw_icp_recipients")
    {
        bail!("expected all-cycles allocation with no raw surplus phase, got {summary:?}");
    }
    let sample = summary
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing managed CMC burn sample")?;
    if sample.amount_e8s == 0 || sample.gross_share_e8s <= 99_000_000 {
        bail!(
            "expected nearly all spendable ICP gross to go to the burned canister, got {summary:?}"
        );
    }
    if summary
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .any(|sample| sample.amount_e8s == 0)
    {
        bail!("expected every positive-burn canister to receive a top-up, got {summary:?}");
    }
    if summary.planned_retained_e8s >= 10_000 {
        bail!("expected only fee-unspendable dust to remain, got {summary:?}");
    }
    let transfers = env.transfers()?;
    if !transfers.iter().any(|transfer| {
        transfer.to.owner == env.cmc
            && transfer.to.subaccount == Some(principal_to_subaccount(env.cmc))
    }) {
        bail!("expected CMC top-up transfer for burned managed canister, got {transfers:?}");
    }
    if transfers
        .iter()
        .any(|transfer| transfer.to.owner != env.cmc)
    {
        bail!("expected no raw ICP surplus transfer without recipients, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn no_raw_recipients_waits_until_every_positive_burner_is_fee_efficient() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.cmc, 10_000_000_000_000)?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.credit_relay(30_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_canister_cycles(env.cmc, 1_000_000_000_000)?;
    env.set_canister_cycles(env.ledger, 9_900_000_000_000)?;
    let retained = env.tick_relay()?;
    if retained.mode != RelayMode::TopUpThenSurplus
        || retained.ledger_transfer_count != 0
        || retained.cmc_notify_success_count != 0
        || retained.planned_retained_e8s != 30_000
        || retained.skipped_surplus_reason.as_deref()
            != Some("all_cycles_batch_below_fee_efficient_threshold")
    {
        bail!("expected all-cycles batch gate to retain insufficient balance, got {retained:?}");
    }
    if retained
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .any(|sample| {
            sample.amount_e8s != 0
                || sample.skipped_reason.as_deref()
                    != Some("all_cycles_batch_below_fee_efficient_threshold")
        })
    {
        bail!("expected no partial fast-burner top-up below threshold, got {retained:?}");
    }
    if !env.transfers()?.is_empty() || env.relay_balance()? != 30_000 {
        bail!("expected retained ICP to remain in relay default ledger account");
    }

    env.credit_relay(10_000_000_000)?;
    env.set_canister_cycles(env.cmc, 100_000_000_000)?;
    env.set_canister_cycles(env.ledger, 9_800_000_000_000)?;
    let funded = env.tick_relay()?;
    let positive = funded
        .canisters
        .iter()
        .filter(|sample| sample.burn_cycles > 0)
        .collect::<Vec<_>>();
    if funded.mode != RelayMode::TopUpThenSurplus
        || funded.cmc_notify_success_count != positive.len() as u32
        || !funded.surplus_transfers.is_empty()
        || funded.skipped_surplus_reason.as_deref() != Some("no_raw_icp_recipients")
    {
        bail!("expected fee-efficient all-cycles batch without raw ICP surplus, got {funded:?}");
    }
    if positive.len() < 2
        || positive
            .iter()
            .any(|sample| sample.amount_e8s == 0 || sample.gross_share_e8s < 20_000)
    {
        bail!(
            "expected all positive-burn canisters to receive fee-efficient top-ups, got {funded:?}"
        );
    }

    let transfers = env.transfers()?;
    let cmc_topup_accounts = [
        Account {
            owner: env.cmc,
            subaccount: Some(principal_to_subaccount(env.cmc)),
        },
        Account {
            owner: env.cmc,
            subaccount: Some(principal_to_subaccount(env.ledger)),
        },
    ];
    if !cmc_topup_accounts
        .iter()
        .all(|account| transfers.iter().any(|transfer| transfer.to == *account))
        || transfers
            .iter()
            .any(|transfer| transfer.to.owner != env.cmc)
    {
        bail!("expected CMC top-up transfers for positive burners and no raw surplus, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn headroom_cmc_topup_prefers_higher_burn_managed_canister() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(10_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly || baseline.ledger_transfer_count != 0 {
        bail!("expected baseline-only first tick without transfer, got {baseline:?}");
    }

    env.set_canister_cycles(env.ledger, 9_000_000_000_000)?;
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.cmc_notify_success_count < 2 {
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
        || !logs.contains(&format!("planned_topup_e8s={}", cmc_sample.amount_e8s))
        || !logs.contains(&format!("actual_topup_e8s={}", cmc_sample.amount_e8s))
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
fn relay_canister_with_increased_cycles_gets_no_topup_when_others_burned() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
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
    if gained.burn_cycles != 0 || gained.target_topup_cycles != 0 || gained.amount_e8s != 0 {
        bail!("expected gained canister to receive no top-up while another burned: {summary:?}");
    }
    if burned.burn_cycles == 0 {
        bail!("expected burned canister to report positive burn: {summary:?}");
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
    let env = RelayEnv::new_with_config(None, |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(99_000_000)?;
    let _ = env.tick_relay()?;

    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus {
        bail!("expected cycles top-up mode, got {summary:?}");
    }
    if summary
        .canisters
        .iter()
        .any(|sample| sample.burn_cycles != 0 || sample.target_topup_cycles != 0)
    {
        bail!("expected zero burn to plan no top-up, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn surplus_canister_transfer_uses_configured_memo_without_cmc_notify() -> Result<()> {
    require_ignored_flag()?;
    let external_memo = vec![0xA1, 0xB2];
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: cmc,
                memo: external_memo.clone(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before surplus transfer, got {baseline:?}");
    }

    env.set_managed_cycles(2_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.cmc_notify_success_count == 0 {
        bail!("expected bootstrap top-up to establish conversion estimate, got {topup:?}");
    }

    env.credit_relay(99_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(4_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.ledger_transfer_count == 0 {
        bail!("expected surplus transfer after top-up phase, got {summary:?}");
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
            "expected surplus canister recipient transfer with configured memo, got {transfers:?}"
        );
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("RELAY_SURPLUS_TRANSFER ")
        || !logs.contains("memo_len=2")
    {
        bail!("expected public surplus recipient logs, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn recovery_deficit_carries_underfunded_topup_and_blocks_surplus_until_recovered() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: cmc,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(10_000_000_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick, got {baseline:?}");
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(0)?;
    env.credit_relay(60_010_000)?;
    let underfunded = env.tick_relay()?;
    let underfunded_sample = underfunded
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing underfunded CMC burn sample")?;
    if underfunded.mode != RelayMode::TopUpThenSurplus
        || underfunded_sample.target_topup_cycles == 0
        || underfunded_sample.actual_minted_cycles >= underfunded_sample.target_topup_cycles
        || underfunded_sample.remaining_deficit_cycles == 0
        || !underfunded.surplus_transfers.is_empty()
    {
        bail!(
            "expected underfunded CMC top-up with retained recovery deficit, got {underfunded:?}"
        );
    }
    let carried_deficit = underfunded_sample.remaining_deficit_cycles;
    let logs_after_underfunded = env.logs_text()?;
    if !logs_after_underfunded.contains(&format!("remaining_deficit_cycles={carried_deficit}")) {
        bail!(
            "expected RELAY_CANISTER log to expose remaining deficit, got {logs_after_underfunded}"
        );
    }

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_250_000_000_000)?;
    env.credit_relay(300_000_000)?;
    let recovered = env.tick_relay()?;
    let recovered_sample = recovered
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing recovered CMC burn sample")?;
    if recovered_sample.carried_deficit_cycles != carried_deficit
        || recovered_sample.target_topup_cycles != carried_deficit
        || recovered_sample.remaining_deficit_cycles != 0
        || recovered_sample.actual_minted_cycles < recovered_sample.target_topup_cycles
    {
        bail!(
            "expected next tick to carry and clear previous deficit without headroom, got {recovered:?}"
        );
    }
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(9_000_000_000_000)?;
    env.credit_relay(200_000_000)?;
    let mut surplus = env.tick_relay()?;
    for _ in 0..2 {
        if !surplus.surplus_transfers.is_empty() || !env.debug_state()?.active_job_present {
            break;
        }
        surplus = env.tick_relay()?;
    }
    let surplus_sample = surplus
        .canisters
        .iter()
        .find(|sample| sample.canister_id == env.cmc)
        .context("missing post-recovery CMC burn sample")?;
    if surplus_sample.carried_deficit_cycles != 0 || surplus_sample.remaining_deficit_cycles != 0 {
        bail!("expected recovery deficit to stay cleared on next clean tick, got {surplus:?}");
    }
    if surplus.surplus_transfers.iter().all(|transfer| {
        transfer.target != SurplusTarget::Canister(env.cmc) || transfer.amount_e8s == 0
    }) {
        bail!("expected raw surplus to route after recovery deficit cleared, got {surplus:?}");
    }
    let transfers = env.transfers()?;
    if transfers.iter().all(|transfer| {
        transfer.to
            != (Account {
                owner: env.cmc,
                subaccount: None,
            })
    }) {
        bail!("expected surplus transfer to CMC account after deficit recovery, got {transfers:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn surplus_neuron_transfers_are_suppressed_below_one_icp_each() -> Result<()> {
    require_ignored_flag()?;
    let io_neuron = 10_292_412_127_977_304_661_u64;
    let jupiter_faucet_neuron = 11_614_578_985_374_291_210_u64;
    let io_memo = b"10292412127977304661".to_vec();
    let env = RelayEnv::new_with_config(None, |_, cmc, _, _relay| {
        (
            vec![cmc],
            None,
            vec![
                SurplusNeuronRecipient {
                    neuron_id: io_neuron,
                    memo: Vec::new(),
                },
                SurplusNeuronRecipient {
                    neuron_id: jupiter_faucet_neuron,
                    memo: io_memo.clone(),
                },
            ],
        )
    })?;
    env.set_managed_cycles(4_000_000_000_000)?;
    env.credit_relay(99_000_000)?;

    let baseline = env.tick_relay()?;
    if baseline.mode != RelayMode::BaselineOnly {
        bail!("expected baseline-only first tick before surplus transfer, got {baseline:?}");
    }

    env.set_managed_cycles(2_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.cmc_notify_success_count == 0 {
        bail!("expected bootstrap top-up to establish conversion estimate, got {topup:?}");
    }

    env.credit_relay(99_000_000)?;
    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(4_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.ledger_transfer_count != summary.cmc_notify_success_count
    {
        bail!("expected only CMC top-up transfers below raw ICP threshold, got {summary:?}");
    }
    if summary.skipped_surplus_reason.as_deref() != Some("raw_icp_share_below_1_icp") {
        bail!("expected raw ICP threshold skip reason, got {summary:?}");
    }
    if !summary.surplus_transfers.iter().all(|transfer| {
        transfer.amount_e8s == 0
            && transfer.skipped_reason.as_deref() == Some("raw_icp_share_below_1_icp")
    }) {
        bail!("expected all surplus neuron transfers suppressed below threshold, got {summary:?}");
    }

    let transfers = env.transfers()?;
    if transfers.iter().any(|transfer| {
        transfer.to
            == (Account {
                owner: env.governance,
                subaccount: Some(neuron_subaccount(io_neuron)),
            })
            || transfer.to
                == (Account {
                    owner: env.governance,
                    subaccount: Some(neuron_subaccount(jupiter_faucet_neuron)),
                })
    }) {
        bail!("expected no raw ICP neuron surplus transfers below threshold, got {transfers:?}");
    }
    if !summary
        .surplus_transfers
        .iter()
        .any(|transfer| transfer.memo_len == Some(io_memo.len() as u32))
    {
        bail!("expected suppressed Jupiter Faucet memo metadata to be preserved, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_retains_funds_when_cycles_are_unchanged_and_conversion_is_missing() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: relay,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(5_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
    let _ = env.tick_relay()?;

    env.add_relay_cycles(1_000_000_000_000);
    env.set_managed_cycles(5_000_000_000_000)?;
    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::TopUpThenSurplus || summary.ledger_transfer_count != 0 {
        bail!("expected unchanged cycles and missing conversion to retain funds, got {summary:?}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_recomputes_topups_each_tick_after_prior_no_topup_tick() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_config(None, |_, cmc, _, relay| {
        (
            vec![cmc],
            Some(vec![SurplusCanisterRecipient {
                canister_id: relay,
                memo: Vec::new(),
            }]),
            Vec::new(),
        )
    })?;
    env.set_managed_cycles(6_000_000_000_000)?;
    env.add_relay_cycles(2_000_000_000_000);
    env.credit_relay(100_000_000)?;
    let _ = env.tick_relay()?;

    env.set_managed_cycles(6_000_000_000_000)?;
    let raw = env.tick_relay()?;
    if raw.mode != RelayMode::TopUpThenSurplus {
        bail!("expected unchanged cycles to avoid top-up, got {raw:?}");
    }

    env.credit_relay(100_000_000)?;
    env.set_managed_cycles(4_000_000_000_000)?;
    let topup = env.tick_relay()?;
    if topup.mode != RelayMode::TopUpThenSurplus || topup.cmc_notify_success_count == 0 {
        bail!("expected later burn tick to perform CMC top-up, got {topup:?}");
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
fn relay_tick_succeeds_when_both_production_blackholes_are_managed() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new_with_production_blackholes_managed()?;

    let summary = env.tick_relay()?;
    if summary.mode != RelayMode::BaselineOnly || !summary.probe_failures.is_empty() {
        bail!("expected complete baseline tick with both managed blackholes, got {summary:?}");
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
    env.add_relay_cycles(1_000_000_000_000);
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
    env.add_relay_cycles(1_000_000_000_000);
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
    env.add_relay_cycles(1_000_000_000_000);
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
    let env = RelayEnv::new_with_config(Some(1), |ledger, cmc, _, _| {
        (vec![ledger, cmc], None, Vec::new())
    })?;
    env.set_canister_cycles(env.ledger, 10_000_000_000_000)?;
    env.set_managed_cycles(10_000_000_000_000)?;
    env.credit_relay(5_000_000_000)?;
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
    if summary.mode != RelayMode::TopUpThenSurplus
        || summary.partial_tick_count == 0
        || summary.ledger_transfer_count < 2
        || env.debug_state()?.active_job_present
    {
        bail!("expected later tick to resume and complete transfer-limited job, got {summary:?}");
    }
    let logs = env.logs_text()?;
    if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("partial_tick_count=")
    {
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

    env.add_relay_cycles(1_000_000_000_000);
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
    if summary.mode != RelayMode::TopUpThenSurplus
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
    if !logs.contains("RELAY_SUMMARY mode=TopUpThenSurplus")
        || !logs.contains("ledger_transfer_count=1")
        || !logs.contains("cmc_notify_success_count=1")
    {
        bail!("expected coherent public summary after upgrade recovery, got {logs}");
    }
    Ok(())
}

#[test]
#[ignore]
fn relay_post_upgrade_logs_lifecycle_and_keeps_startup_liveness_stateless() -> Result<()> {
    require_ignored_flag()?;
    let env = RelayEnv::new(None)?;
    env.upgrade_relay_without_config_changes()?;
    env.pic.advance_time(Duration::from_secs(2));
    tick_n(&env.pic, 5);

    let logs = env.logs_text()?;
    if !logs.contains("relay LIFECYCLE event=post_upgrade_complete timers_installed=true") {
        bail!("expected post-upgrade lifecycle log, got {logs}");
    }
    if logs.contains("timer fired") {
        bail!("expected no timer-fired public log spam, got {logs}");
    }
    Ok(())
}
