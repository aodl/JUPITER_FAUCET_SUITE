use super::*;
use candid::Nat;
use futures::executor::block_on;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

struct World {
    balance: u64,
    fee: u64,
    rate: u64,
    child: Principal,
    cycles: u128,
    module: Option<Vec<u8>>,
    controllers: Vec<Principal>,
    transfers: Vec<(TransferArg, u64)>,
    transfer_requests: Vec<TransferArg>,
    notifications: Vec<(Principal, u64)>,
    minted: BTreeMap<u64, u128>,
    final_funding: u64,
    creates: usize,
    installs: usize,
    lose_ledger_reply: bool,
    lose_duplicate_replies: usize,
    lose_cmc_reply: bool,
    processing: bool,
    unavailable: bool,
    fail_install: bool,
    funding_install_rejections: usize,
    reject_ledger_once: bool,
    raise_fee_after_install: bool,
    lose_install_reply: bool,
    duplicate_override: Option<u64>,
    next_cmc_error: Option<jupiter_ic_clients::cmc::NotifyTopUpError>,
    status_barrier: Option<futures::channel::oneshot::Receiver<()>>,
}
#[derive(Clone)]
struct Clients(Arc<Mutex<World>>);

#[async_trait::async_trait]
impl LedgerClient for Clients {
    async fn fee_e8s(&self) -> Result<u64, ClientError> {
        Ok(self.0.lock().unwrap().fee)
    }
    async fn balance_of_e8s(&self, _: Account) -> Result<u64, ClientError> {
        Ok(self.0.lock().unwrap().balance)
    }
    async fn icrc1_transfer(
        &self,
        arg: TransferArg,
    ) -> Result<Result<BlockIndex, TransferError>, ClientError> {
        let mut w = self.0.lock().unwrap();
        w.transfer_requests.push(arg.clone());
        if std::mem::take(&mut w.reject_ledger_once) {
            return Ok(Err(TransferError::TemporarilyUnavailable));
        }
        if let Some(block) = w.duplicate_override.take() {
            return Ok(Err(TransferError::Duplicate {
                duplicate_of: Nat::from(block),
            }));
        }
        if let Some(block) = w
            .transfers
            .iter()
            .find(|(old, _)| old == &arg)
            .map(|(_, block)| *block)
        {
            if w.lose_duplicate_replies > 0 {
                w.lose_duplicate_replies -= 1;
                return Err(ClientError::Call("lost duplicate reply".into()));
            }
            return Ok(Err(TransferError::Duplicate {
                duplicate_of: Nat::from(block),
            }));
        }
        if arg.fee != Some(Nat::from(w.fee)) {
            return Ok(Err(TransferError::BadFee {
                expected_fee: Nat::from(w.fee),
            }));
        }
        let amount = u64::try_from(arg.amount.0.clone()).unwrap();
        if w.balance < amount + w.fee {
            return Ok(Err(TransferError::InsufficientFunds {
                balance: Nat::from(w.balance),
            }));
        }
        w.balance -= amount + w.fee;
        if arg.to == relay_subaccount_one(w.child) {
            w.final_funding += amount;
        }
        let block = w.transfers.len() as u64 + 12;
        w.transfers.push((arg, block));
        if std::mem::take(&mut w.lose_ledger_reply) {
            return Err(ClientError::Call("lost accepted reply".into()));
        }
        Ok(Ok(Nat::from(block)))
    }
}
#[async_trait::async_trait]
impl CmcClient for Clients {
    async fn get_icp_xdr_conversion_rate(&self) -> Result<IcpXdrConversionRate, ClientError> {
        Ok(IcpXdrConversionRate {
            timestamp_seconds: 0,
            xdr_permyriad_per_icp: self.0.lock().unwrap().rate,
        })
    }
    async fn notify_top_up(
        &self,
        target: Principal,
        block: u64,
    ) -> Result<u128, jupiter_ic_clients::cmc::NotifyTopUpError> {
        use jupiter_ic_clients::cmc::{NotifyRetryableError, NotifyTopUpError};
        let mut w = self.0.lock().unwrap();
        w.notifications.push((target, block));
        if let Some(error) = w.next_cmc_error.take() {
            return Err(error);
        }
        if std::mem::take(&mut w.processing) {
            return Err(NotifyTopUpError::Retryable(
                NotifyRetryableError::Processing,
            ));
        }
        if let Some(minted) = w.minted.get(&block) {
            return Ok(*minted);
        }
        let arg = &w
            .transfers
            .iter()
            .find(|(_, id)| *id == block)
            .expect("accepted recovery block")
            .0;
        assert_eq!(
            arg.to,
            cmc_deposit_account(tests::config().cmc_canister_id.unwrap(), target)
        );
        let minted = u128::try_from(arg.amount.0.clone()).unwrap() * u128::from(w.rate);
        if target == w.child {
            w.cycles += minted;
        }
        w.minted.insert(block, minted);
        if std::mem::take(&mut w.lose_cmc_reply) {
            return Err(NotifyTopUpError::Transport("lost mint reply".into()));
        }
        Ok(minted)
    }
}
#[async_trait::async_trait]
impl ManagementClient for Clients {
    async fn create_canister(
        &self,
        args: &jupiter_ic_clients::management::CreateCanisterArgs,
        cycles: u128,
    ) -> Result<jupiter_ic_clients::management::CreateCanisterResult, String> {
        let mut w = self.0.lock().unwrap();
        w.creates += 1;
        w.cycles = cycles - 500_000_000_000;
        assert_eq!(
            args.settings.as_ref().unwrap().controllers,
            Some(vec![self_canister_id()])
        );
        Ok(jupiter_ic_clients::management::CreateCanisterResult {
            canister_id: w.child,
        })
    }
    async fn install_code(
        &self,
        args: &jupiter_ic_clients::management::InstallCodeArgs,
    ) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        w.installs += 1;
        assert_eq!(args.canister_id, w.child);
        assert!(matches!(
            args.mode,
            jupiter_ic_clients::management::InstallMode::Install
        ));
        assert!(w.module.is_none(), "never install twice");
        assert!(
            w.cycles >= recovery::child_budget(&tests::config()).unwrap(),
            "representative upfront budget floor"
        );
        if w.funding_install_rejections > 0 {
            w.funding_install_rejections -= 1;
            w.cycles = recovery::child_budget(&tests::config()).unwrap() - 100_000_000_000;
            return Err("CallRejected raw_reject_code: 2, is out of cycles".into());
        }
        if w.fail_install {
            return Err("CallRejected raw_reject_code: 2, unrelated platform failure".into());
        }
        w.module = approved_relay_onchain_module_hash().map(|h| h.to_vec());
        w.cycles -= 100_000_000_000;
        if w.raise_fee_after_install {
            w.fee = 20_000;
        }
        if w.lose_install_reply {
            return Err("lost successful install reply".into());
        }
        Ok(())
    }
    async fn canister_status(&self, id: Principal) -> Result<CanisterStatusResult, String> {
        let wait = self.0.lock().unwrap().status_barrier.take();
        if let Some(wait) = wait {
            let _ = wait.await;
        }
        let w = self.0.lock().unwrap();
        assert_eq!(id, w.child);
        if w.unavailable {
            return Err("temporary status outage".into());
        }
        Ok(CanisterStatusResult {
            status: CanisterStatusKind::Running,
            cycles: Nat::from(w.cycles),
            module_hash: w.module.clone(),
            settings: jupiter_ic_clients::management::DefiniteCanisterSettings {
                controllers: w.controllers.clone(),
                log_visibility: LogVisibility::Public,
                status_visibility: StatusVisibility::Public,
            },
        })
    }
    async fn update_settings(
        &self,
        args: &jupiter_ic_clients::management::UpdateSettingsArgs,
    ) -> Result<(), String> {
        let mut w = self.0.lock().unwrap();
        assert_eq!(args.canister_id, w.child);
        w.controllers = args.settings.controllers.clone().unwrap();
        Ok(())
    }
}

fn fixture(balance: u64) -> (CanonicalRelaySetup, RelaySetupKey, Clients) {
    clear_setup_entries_for_debug();
    admission::reset_for_test();
    let config = tests::config();
    state::set_state(State::new(config.clone(), 0));
    let setup = CanonicalRelaySetup::canonicalize(
        vec![Principal::from_slice(&[1])],
        vec![RelaySurplusRecipient::Principal {
            principal: Principal::from_slice(&[200]),
            memo: vec![],
        }],
    )
    .unwrap();
    let key = setup.key();
    let child = Principal::from_slice(&[80]);
    let mut old_config = config;
    old_config.relay_initial_cycles = 750_000_000_000;
    enable_legacy_continuation(&old_config, &approved_relay_onchain_module_hash().unwrap());
    // The exact predecessor record has no continuation field.
    #[derive(CandidType)]
    struct OldProgress {
        phase: RelayCreationPhase,
        cmc_transfer: Option<RelayTransferRecord>,
        cycles_minted: Option<u128>,
        create_dispatched_at_ts: Option<u64>,
        relay_canister_id: Option<Principal>,
        relay_funding_transfer: Option<RelayTransferRecord>,
        last_error: Option<String>,
    }
    #[derive(CandidType)]
    enum OldEntry {
        ManualRecoveryRequired(OldProgress),
    }
    let old = OldEntry::ManualRecoveryRequired(OldProgress {
        phase: RelayCreationPhase::ChildCreated,
        cmc_transfer: Some(RelayTransferRecord {
            amount_e8s: 45_000_000,
            fee_e8s: 10_000,
            created_at_time_nanos: 1,
            block_index: Some(11),
        }),
        cycles_minted: Some(1_000_000_000_000),
        create_dispatched_at_ts: Some(1),
        relay_canister_id: Some(child),
        relay_funding_transfer: None,
        last_error: Some("Retained diagnostics are not eligibility authority".into()),
    });
    let entry = RelaySetupEntry::from_bytes(Cow::Owned(candid::encode_one(old).unwrap()));
    assert!(
        matches!(&entry, RelaySetupEntry::ManualRecoveryRequired(p) if p.continuation.is_none())
    );
    insert_entry(key, entry);
    (
        setup,
        key,
        Clients(Arc::new(Mutex::new(World {
            balance,
            fee: 10_000,
            rate: 1_000_000,
            child,
            cycles: 500_000_000_000,
            module: None,
            controllers: vec![self_canister_id()],
            transfers: vec![],
            transfer_requests: vec![],
            notifications: vec![],
            minted: BTreeMap::new(),
            final_funding: 0,
            creates: 1,
            installs: 0,
            lose_ledger_reply: false,
            lose_duplicate_replies: 0,
            lose_cmc_reply: false,
            processing: false,
            unavailable: false,
            fail_install: false,
            funding_install_rejections: 0,
            reject_ledger_once: false,
            raise_fee_after_install: false,
            lose_install_reply: false,
            duplicate_override: None,
            next_cmc_error: None,
            status_barrier: None,
        }))),
    )
}
fn quote(setup: &CanonicalRelaySetup, key: RelaySetupKey, clients: &Clients) -> RelayRecoveryView {
    block_on(recovery::quote_for_test(
        key,
        setup,
        &tests::config(),
        clients,
        clients,
        clients,
    ))
}
fn resume(
    setup: &CanonicalRelaySetup,
    key: RelaySetupKey,
    clients: &Clients,
) -> RelaySetupNotifyResult {
    block_on(recovery::continue_setup(
        key,
        setup,
        &tests::config(),
        self_canister_id(),
        clients,
        clients,
        clients,
    ))
}

#[test]
fn user_funds_only_residual_and_finishes_the_same_legacy_child() {
    let (setup, key, clients) = fixture(10_000_000);
    let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
        panic!("funding quote")
    };
    assert_eq!(q.required_e8s, 111_540_000);
    assert_eq!(q.additional_e8s, 101_540_000);
    assert_eq!(q.setup_account, setup_account_for(self_canister_id(), key));
    assert!(state::relay_creating_keys().is_empty());
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::BelowCurrentRequirement { .. }
    ));
    assert!(clients.0.lock().unwrap().transfers.is_empty());
    clients.0.lock().unwrap().balance += q.additional_e8s;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.creates, 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
    assert!(w.controllers.is_empty());
    let Some(RelaySetupEntry::Active {
        completed: Some(completed),
        ..
    }) = get_entry(key)
    else {
        panic!("activation must retain the completed payment journal")
    };
    assert_eq!(completed.cmc_transfer.unwrap().block_index, Some(11));
    assert_eq!(completed.cycles_minted, Some(1_000_000_000_000));
    assert_eq!(completed.create_dispatched_at_ts, Some(1));
    let conversions = completed.continuation.unwrap().conversions;
    assert_eq!(conversions.len(), 1);
    assert_eq!(conversions[0].transfer.block_index, Some(12));
    assert!(conversions[0].cycles_minted.is_some());
    let seed = completed.relay_funding_transfer.unwrap();
    assert_eq!(seed.block_index, Some(13));
    assert_eq!(seed.amount_e8s, 105_020_000);
}

#[test]
fn lost_ledger_reply_reuses_identity_and_survives_upgrade_before_cmc() {
    let (setup, key, clients) = fixture(111_540_000);
    clients.0.lock().unwrap().lose_ledger_reply = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    let Some(RelaySetupEntry::Paused(p)) = get_entry(key) else {
        panic!("paused")
    };
    let original = p.cmc_transfer.clone();
    let bytes = RelaySetupEntry::Paused(p).into_bytes();
    insert_entry(key, RelaySetupEntry::from_bytes(Cow::Owned(bytes)));
    reconcile_interrupted_creating_entries_after_upgrade();
    advance_retry_window();
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.transfer_requests.len(), 3);
    assert_eq!(w.transfer_requests[0], w.transfer_requests[1]);
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(original.unwrap().block_index, Some(11));
    assert_eq!(w.creates, 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.balance, 0);
}

#[test]
fn temporarily_unavailable_after_a_lost_ledger_reply_retries_the_same_identity() {
    let (setup, key, clients) = fixture(111_540_000);
    clients.0.lock().unwrap().lose_ledger_reply = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    let Some(RelaySetupEntry::Paused(first)) = get_entry(key) else {
        panic!("lost reply must retain the uncertain transfer")
    };
    let original = first.cmc_transfer.clone();
    let transfer = first.continuation.unwrap().conversions[0].transfer.clone();
    let balance_after_acceptance = clients.0.lock().unwrap().balance;
    advance_retry_window();
    clients.0.lock().unwrap().reject_ledger_once = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    let Some(RelaySetupEntry::Paused(pending)) = get_entry(key) else {
        panic!("temporary unavailability must preserve the uncertain operation")
    };
    let continuation = pending.continuation.unwrap();
    assert!(!continuation.terminal);
    assert_eq!(continuation.conversions.len(), 1);
    let operation = &continuation.conversions[0];
    assert_eq!(operation.transfer, transfer);
    assert_eq!(operation.dispatches, 2);
    assert!(!operation.rejected);
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::CheckPending(_)
    ));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    {
        let w = clients.0.lock().unwrap();
        assert_eq!(w.transfer_requests.len(), 2);
        assert_eq!(w.transfers.len(), 1);
        assert_eq!(w.balance, balance_after_acceptance);
        assert!(w.notifications.is_empty());
    }
    advance_retry_window();
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfer_requests.len(), 4);
    assert!(w.transfer_requests[..3]
        .iter()
        .all(|arg| arg == &w.transfer_requests[0]));
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.notifications, vec![(w.child, 12)]);
    assert_eq!(w.minted.len(), 1);
    assert_eq!(w.creates, 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
    let Some(RelaySetupEntry::Active {
        completed: Some(completed),
        ..
    }) = get_entry(key)
    else {
        panic!("completed journal")
    };
    assert_eq!(completed.cmc_transfer, original);
    assert_eq!(completed.relay_canister_id, Some(w.child));
    assert_eq!(
        completed.relay_funding_transfer.unwrap().block_index,
        Some(13)
    );
}

#[test]
fn cmc_processing_and_lost_mint_reply_never_buy_again_even_after_child_topup() {
    for lost in [false, true] {
        let (setup, key, clients) = fixture(111_540_000);
        {
            let mut w = clients.0.lock().unwrap();
            w.processing = !lost;
            w.lose_cmc_reply = lost;
        }
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        advance_retry_window();
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::Active { .. }
        ));
        let w = clients.0.lock().unwrap();
        assert_eq!(w.transfers.len(), 2);
        assert_eq!(w.notifications.len(), 2);
        assert_eq!(w.notifications[0], w.notifications[1]);
        assert_eq!(w.minted.len(), 1);
        assert_eq!(w.installs, 1);
    }
}

#[test]
fn existing_funds_independent_cycles_or_installed_code_need_no_additional_payment() {
    for installed in [false, true] {
        let (setup, key, clients) = fixture(105_030_000);
        {
            let mut w = clients.0.lock().unwrap();
            w.cycles = 2_000_000_000_000;
            if installed {
                w.module = approved_relay_onchain_module_hash().map(|h| h.to_vec());
            }
        }
        assert!(matches!(
            quote(&setup, key, &clients),
            RelayRecoveryView::Ready(RelayRecoveryQuote {
                additional_e8s: 0,
                ..
            })
        ));
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::Active { .. }
        ));
        let w = clients.0.lock().unwrap();
        assert!(w.notifications.is_empty());
        assert_eq!(w.transfers.len(), 1);
        assert_eq!(w.installs, usize::from(!installed));
    }
}

#[test]
fn unsafe_states_and_nonfunding_install_rejection_never_ask_for_payment_or_repeat_install() {
    for case in 0..5 {
        let (setup, key, clients) = fixture(111_540_000);
        {
            let mut w = clients.0.lock().unwrap();
            match case {
                0 => w.module = Some(vec![99; 32]),
                1 => w.controllers.push(Principal::anonymous()),
                2 => w.unavailable = true,
                3 => {
                    let Some(RelaySetupEntry::ManualRecoveryRequired(mut p)) = get_entry(key)
                    else {
                        panic!()
                    };
                    p.relay_canister_id = None;
                    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(p));
                }
                _ => {
                    w.cycles = 2_000_000_000_000;
                    w.fail_install = true;
                }
            }
        }
        if case == 4 {
            assert!(matches!(
                resume(&setup, key, &clients),
                RelaySetupNotifyResult::ManualRecoveryRequired { .. }
            ));
            assert!(matches!(
                quote(&setup, key, &clients),
                RelayRecoveryView::Unsafe(_)
            ));
            let _ = resume(&setup, key, &clients);
            assert_eq!(clients.0.lock().unwrap().installs, 1);
        } else {
            assert!(matches!(
                quote(&setup, key, &clients),
                RelayRecoveryView::Unsafe(_) | RelayRecoveryView::CheckPending(_)
            ));
            let _ = resume(&setup, key, &clients);
        }
        assert!(clients.0.lock().unwrap().transfers.is_empty());
    }
}

#[test]
fn changed_fee_rate_partial_funds_and_exhausted_deduplication_fail_closed() {
    let (setup, key, clients) = fixture(111_540_000);
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Ready(_)
    ));
    {
        let mut w = clients.0.lock().unwrap();
        w.rate = 100_000;
        w.fee = 20_000;
    }
    let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
        panic!()
    };
    // 1.5T / 100,000 cycles per e8s, 5M exchange margin, the unchanged
    // 105.02M final reserve and two 20,000-e8s fees, less existing funds.
    assert_eq!(q.additional_e8s, 13_520_000);
    clients.0.lock().unwrap().balance += q.additional_e8s - 5_000_001;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::BelowCurrentRequirement {
            shortfall_e8s: 5_000_001,
            ..
        }
    ));
    assert!(clients.0.lock().unwrap().transfers.is_empty());
    clients.0.lock().unwrap().balance += 5_000_001;
    clients.0.lock().unwrap().lose_ledger_reply = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    TEST_NOW_NANOS.with(|clock| clock.set(clock.get() + 24 * 60 * 60 * 1_000_000_000));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::ManualRecoveryRequired { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 1);
    assert_eq!(w.transfer_requests.len(), 1);
    assert!(w.notifications.is_empty());
}

#[test]
fn maximum_new_journal_preserves_the_existing_4096_byte_stable_bound() {
    let (setup, key, _) = fixture(0);
    let targets = (1..=20).map(|n| Principal::from_slice(&[n; 29])).collect();
    let recipients = (100..105)
        .map(|n| RelaySurplusRecipient::Principal {
            principal: Principal::from_slice(&[n; 29]),
            memo: vec![255; 32],
        })
        .collect();
    let maximum = CanonicalRelaySetup::canonicalize(targets, recipients).unwrap();
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut p)) = get_entry(key) else {
        panic!()
    };
    let mut config = tests::config();
    config.ledger_canister_id = Principal::from_slice(&[252; 29]);
    config.cmc_canister_id = Some(Principal::from_slice(&[253; 29]));
    let mut c = recovery::RelayContinuation::new(&config, &maximum);
    c.identity.original_create_cycles = u128::MAX;
    c.identity.final_reserve_e8s = u64::MAX;
    c.identity.margin_e8s = u64::MAX;
    c.attempt = u64::MAX;
    c.retry_after_nanos = Some(u64::MAX);
    c.conversions = (0..4)
        .map(|_| recovery::RecoveryConversion {
            transfer: RelayTransferRecord {
                amount_e8s: u64::MAX,
                fee_e8s: u64::MAX,
                created_at_time_nanos: u64::MAX,
                block_index: Some(u64::MAX),
            },
            destination: Principal::from_slice(&[255; 29]),
            for_creation: false,
            to: cmc_deposit_account(
                config.cmc_canister_id.unwrap(),
                Principal::from_slice(&[255; 29]),
            ),
            memo: TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec(),
            notifications: u8::MAX,
            cycles_minted: Some(u128::MAX),
            rejected: false,
            dispatches: u8::MAX,
        })
        .collect();
    p.cmc_transfer = c.conversions.first().map(|op| op.transfer.clone());
    p.relay_funding_transfer = p.cmc_transfer.clone();
    p.relay_canister_id = Some(Principal::from_slice(&[251; 29]));
    p.cycles_minted = Some(u128::MAX);
    p.create_dispatched_at_ts = Some(u64::MAX);
    p.continuation = Some(c);
    p.last_error = Some("x".repeat(MAX_DIAGNOSTIC_BYTES));
    let bytes = RelaySetupEntry::Paused(p.clone()).into_bytes();
    eprintln!("maximum continuation stable bytes={}", bytes.len());
    assert!(
        bytes.len() <= MAX_RELAY_SETUP_ENTRY_BYTES as usize,
        "{}",
        bytes.len()
    );
    assert_eq!(
        RelaySetupEntry::from_bytes(Cow::Owned(bytes)),
        RelaySetupEntry::Paused(p.clone())
    );
    let active = RelaySetupEntry::Active {
        relay_canister_id: p.relay_canister_id.unwrap(),
        completed: Some(p),
    };
    let bytes = active.clone().into_bytes();
    assert!(bytes.len() <= MAX_RELAY_SETUP_ENTRY_BYTES as usize);
    assert_eq!(RelaySetupEntry::from_bytes(Cow::Owned(bytes)), active);
    assert_eq!(setup.target_count(), 1);
}

#[test]
fn retained_predecessor_payload_has_the_installed_release_identity() {
    use sha2::{Digest, Sha256};
    let hash =
        hex::decode("221765e5940e3d0721dac7677f0294e05f616a6d91385f984c59a34ceb71cde0").unwrap();
    let payload = relay_install_payload_for_hash(&hash).unwrap();
    assert_eq!(Sha256::digest(payload).as_slice(), hash);
    assert!(relay_install_payload_for_hash(&[99; 32]).is_none());
}

#[test]
fn same_key_continuation_is_single_flight_and_stale_status_cannot_replace_a_successor() {
    use std::future::Future;
    use std::task::{Context, Poll};
    let (setup, key, clients) = fixture(111_540_000);
    let (release, wait) = futures::channel::oneshot::channel();
    clients.0.lock().unwrap().status_barrier = Some(wait);
    let config = tests::config();
    let mut first = Box::pin(recovery::continue_setup(
        key,
        &setup,
        &config,
        self_canister_id(),
        &clients,
        &clients,
        &clients,
    ));
    let waker = futures::task::noop_waker();
    let mut context = Context::from_waker(&waker);
    assert!(matches!(first.as_mut().poll(&mut context), Poll::Pending));
    assert_eq!(resume(&setup, key, &clients), RelaySetupNotifyResult::Busy);
    let successor = Principal::from_slice(&[99]);
    insert_entry(
        key,
        RelaySetupEntry::Active {
            relay_canister_id: successor,
            completed: None,
        },
    );
    release.send(()).unwrap();
    assert_eq!(
        block_on(first),
        RelaySetupNotifyResult::Active {
            relay_canister_id: successor
        }
    );
    assert_eq!(
        get_entry(key),
        Some(RelaySetupEntry::Active {
            relay_canister_id: successor,
            completed: None,
        })
    );
    let world = clients.0.lock().unwrap();
    assert!(world.transfers.is_empty());
    assert!(world.notifications.is_empty());
    assert_eq!(world.installs, 0);
}

#[test]
fn precreate_mint_deficit_credits_original_cycles_and_only_funds_remaining_creation() {
    let (setup, key, clients) = fixture(111_540_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.phase = RelayCreationPhase::CmcTransferAccepted;
    progress.relay_canister_id = None;
    progress.create_dispatched_at_ts = None;
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    clients.0.lock().unwrap().creates = 0;
    clients.0.lock().unwrap().processing = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    let Some(RelaySetupEntry::Paused(progress)) = get_entry(key) else {
        panic!()
    };
    assert_eq!(progress.cycles_minted, Some(1_000_000_000_000));
    assert_eq!(progress.cmc_transfer.unwrap().block_index, Some(11));
    advance_retry_window();
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.creates, 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(
        w.notifications,
        vec![(self_canister_id(), 12), (self_canister_id(), 12)]
    );
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
}

#[test]
fn final_reserve_deficit_requests_only_missing_seed_and_dispatches_it_once() {
    let (setup, key, clients) = fixture(100_000_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.phase = RelayCreationPhase::CodeInstalled;
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    clients.0.lock().unwrap().module = approved_relay_onchain_module_hash().map(|h| h.to_vec());
    let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
        panic!()
    };
    assert_eq!(q.cycles_deficit, 0);
    assert_eq!(q.additional_e8s, 5_030_000);
    clients.0.lock().unwrap().balance += q.additional_e8s;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 1);
    assert!(w.notifications.is_empty());
    assert_eq!(w.installs, 0);
    assert_eq!(w.final_funding, 105_020_000);
}

#[test]
fn legacy_initialization_retains_predecessor_service_defaults_and_exact_memo() {
    #[derive(CandidType, Deserialize)]
    struct Inputs {
        ledger_canister_id: Option<Principal>,
        sns_rewards_canister_id: Option<Principal>,
        icp_index_canister_id: Option<Principal>,
        managed_canisters: Vec<Principal>,
    }
    let (setup, _, _) = fixture(0);
    let bytes = relay_init_arg_with_service_defaults(&tests::config(), &setup, true);
    let old: Inputs = candid::decode_one(&bytes).unwrap();
    assert_eq!(
        old.ledger_canister_id,
        Some(tests::config().ledger_canister_id)
    );
    assert_eq!(old.managed_canisters, setup.targets());
    assert_eq!(old.sns_rewards_canister_id, None);
    assert_eq!(old.icp_index_canister_id, None);
    let fresh: Inputs = candid::decode_one(&relay_init_arg(&tests::config(), &setup)).unwrap();
    assert_eq!(
        fresh.sns_rewards_canister_id,
        Some(tests::config().rewards_account.owner)
    );
    assert_eq!(
        fresh.icp_index_canister_id,
        Some(tests::config().index_canister_id)
    );
}

#[test]
fn exact_quoted_deposit_tolerates_small_idle_drift_without_a_second_payment() {
    let (setup, key, clients) = fixture(10_000_000);
    let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
        panic!()
    };
    {
        let mut world = clients.0.lock().unwrap();
        world.balance += q.additional_e8s;
        world.cycles -= 50_000_000;
    }
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Ready(RelayRecoveryQuote {
            additional_e8s: 0,
            ..
        })
    ));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    assert_eq!(clients.0.lock().unwrap().balance, 0);
}

#[test]
fn a_duplicate_pointing_to_original_conversion_is_never_notified_for_the_child() {
    let (setup, key, clients) = fixture(111_540_000);
    clients.0.lock().unwrap().duplicate_override = Some(11);
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::ManualRecoveryRequired { .. }
    ));
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Unsafe(_)
    ));
    let world = clients.0.lock().unwrap();
    assert!(world.notifications.is_empty());
    assert!(world.transfers.is_empty());
    assert_eq!(world.balance, 111_540_000);
}

#[test]
fn refunds_and_pruned_results_expose_no_new_payment() {
    use jupiter_ic_clients::cmc::{NotifyTerminalError, NotifyTopUpError};
    for error in [
        NotifyTopUpError::Terminal(NotifyTerminalError::Refunded {
            reason: "refunded".into(),
            block_index: Some(900),
        }),
        NotifyTopUpError::Terminal(NotifyTerminalError::TransactionTooOld(12)),
    ] {
        let (setup, key, clients) = fixture(111_540_000);
        clients.0.lock().unwrap().next_cmc_error = Some(error.clone());
        let first = resume(&setup, key, &clients);
        assert!(matches!(
            first,
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        {
            let mut world = clients.0.lock().unwrap();
            world.module = approved_relay_onchain_module_hash().map(|h| h.to_vec());
            world.cycles = 2_000_000_000_000;
        }
        assert!(matches!(
            quote(&setup, key, &clients),
            RelayRecoveryView::Unsafe(_)
        ));
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        let world = clients.0.lock().unwrap();
        assert_eq!(world.transfers.len(), 1);
        assert_eq!(world.notifications.len(), 1);
        assert_eq!(world.installs, 0);
        assert_eq!(world.final_funding, 0);
    }
}

#[test]
fn successful_install_with_lost_reply_is_reconciled_without_a_second_install() {
    let (setup, key, clients) = fixture(105_030_000);
    {
        let mut world = clients.0.lock().unwrap();
        world.cycles = 2_000_000_000_000;
        world.lose_install_reply = true;
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let world = clients.0.lock().unwrap();
    assert_eq!(world.installs, 1);
    assert!(world.notifications.is_empty());
    assert_eq!(world.transfers.len(), 1);
    assert_eq!(world.balance, 0);
}

#[test]
fn known_shortfall_in_the_notify_succeeded_phase_pauses_before_dispatch() {
    let (setup, key, clients) = fixture(111_540_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.phase = RelayCreationPhase::CmcNotifySucceeded;
    progress.relay_canister_id = None;
    progress.create_dispatched_at_ts = None;
    progress.continuation = Some(recovery::RelayContinuation::new(&tests::config(), &setup));
    insert_entry(key, RelaySetupEntry::Creating(progress));
    clients.0.lock().unwrap().creates = 0;
    let guard = admission::Guard::acquire(key, admission::Lane::Completion).unwrap();
    let result = block_on(continue_paid_creation(
        key,
        &setup,
        &tests::config(),
        self_canister_id(),
        &clients,
        &clients,
        &clients,
        guard.ticket(),
    ));
    assert!(matches!(
        result,
        RelaySetupNotifyResult::InProgress {
            phase: RelayCreationPhase::CmcNotifySucceeded,
            ..
        }
    ));
    assert!(matches!(get_entry(key), Some(RelaySetupEntry::Paused(_))));
    assert!(state::relay_creating_keys().is_empty());
    let world = clients.0.lock().unwrap();
    assert_eq!(world.creates, 0);
    assert!(world.notifications.is_empty());
    assert!(world.transfers.is_empty());
}

#[test]
fn a_larger_new_default_does_not_make_an_adequately_funded_legacy_failure_recoverable() {
    let (setup, key, clients) = fixture(600_000_000);
    state::set_relay_legacy_identity(Vec::new());
    let old = tests::config(); // 2T on a 13-node subnet already covers upfront install.
    enable_legacy_continuation(&old, &approved_relay_onchain_module_hash().unwrap());
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.cycles_minted = Some(old.relay_initial_cycles);
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    let mut current = old;
    current.relay_initial_cycles = 5_000_000_000_000;
    let result = block_on(recovery::quote_for_test(
        key, &setup, &current, &clients, &clients, &clients,
    ));
    assert!(matches!(result, RelayRecoveryView::Unsafe(_)));
    assert!(clients.0.lock().unwrap().transfers.is_empty());
}

#[test]
fn a_rejected_precreate_conversion_remains_reconcilable_through_a_later_final_reserve_pause() {
    let (setup, key, clients) = fixture(111_540_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.phase = RelayCreationPhase::CmcNotifySucceeded;
    progress.relay_canister_id = None;
    progress.create_dispatched_at_ts = None;
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    {
        let mut world = clients.0.lock().unwrap();
        world.creates = 0;
        world.reject_ledger_once = true;
        world.raise_fee_after_install = true;
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    advance_retry_window();
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress {
            phase: RelayCreationPhase::CodeInstalled,
            ..
        }
    ));
    let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
        panic!()
    };
    assert_eq!(q.additional_e8s, 10_000);
    clients.0.lock().unwrap().balance += q.additional_e8s;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let world = clients.0.lock().unwrap();
    assert_eq!(world.creates, 1);
    assert_eq!(world.installs, 1);
    assert_eq!(world.transfers.len(), 2);
    assert_eq!(world.transfer_requests.len(), 3);
    assert_eq!(world.notifications.len(), 1);
    assert_eq!(world.final_funding, 105_020_000);
    assert_eq!(world.balance, 0);
}

#[test]
fn contradictory_recovery_operation_evidence_never_authorizes_payment_or_dispatch() {
    for invalid in 0..7 {
        let (setup, key, clients) = fixture(200_000_000);
        let config = tests::config();
        let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
            panic!()
        };
        let child = progress.relay_canister_id.unwrap();
        let mut continuation = recovery::RelayContinuation::new(&config, &setup);
        continuation.identity.original_create_cycles = 750_000_000_000;
        continuation.funding_pause = true;
        let mut operation = recovery::RecoveryConversion {
            transfer: RelayTransferRecord {
                amount_e8s: 1_000_000,
                fee_e8s: 10_000,
                created_at_time_nanos: now_nanos(),
                block_index: None,
            },
            destination: child,
            for_creation: false,
            to: cmc_deposit_account(config.cmc_canister_id.unwrap(), child),
            memo: TOP_UP_CANISTER_MEMO.to_le_bytes().to_vec(),
            notifications: 0,
            cycles_minted: None,
            rejected: false,
            dispatches: 1,
        };
        match invalid {
            0 => operation.destination = Principal::anonymous(),
            1 => operation.to.subaccount = None,
            2 => operation.memo = vec![0],
            3 => operation.cycles_minted = Some(1),
            4 => {
                operation.rejected = true;
                operation.transfer.block_index = Some(12);
            }
            5 => operation.transfer.block_index = Some(11),
            6 => continuation.conversions.push(operation.clone()),
            _ => unreachable!(),
        }
        continuation.conversions.push(operation);
        progress.continuation = Some(continuation);
        insert_entry(key, RelaySetupEntry::Paused(progress));
        assert!(matches!(
            quote(&setup, key, &clients),
            RelayRecoveryView::Unsafe(_)
        ));
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::ManualRecoveryRequired { .. }
        ));
        let world = clients.0.lock().unwrap();
        assert!(world.transfer_requests.is_empty());
        assert!(world.notifications.is_empty());
        assert_eq!(world.installs, 0);
    }
}

#[test]
fn predecessor_paid_cmc_lane_pins_the_frozen_template_or_refuses_before_spending() {
    let (setup, key, clients) = fixture(200_000_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.phase = RelayCreationPhase::CmcNotifySucceeded;
    progress.relay_canister_id = None;
    progress.create_dispatched_at_ts = None;
    let original = progress.clone();
    insert_entry(key, RelaySetupEntry::Creating(progress));
    let mut changed = tests::config();
    changed.relay_initial_cycles = 5_000_000_000_000;
    changed.self_service_relay_interval_seconds = 123_456;
    {
        let guard = admission::Guard::acquire(key, admission::Lane::Completion).unwrap();
        assert!(block_on(recovery::pin_existing_paid_identity(
            key,
            &setup,
            &changed,
            self_canister_id(),
            &clients,
            guard.ticket()
        ))
        .is_ok());
    }
    let Some(RelaySetupEntry::Creating(pinned)) = get_entry(key) else {
        panic!()
    };
    assert_eq!(pinned.cmc_transfer, original.cmc_transfer);
    assert_eq!(pinned.cycles_minted, original.cycles_minted);
    let identity = pinned.continuation.unwrap().identity;
    let mut old = tests::config();
    old.relay_initial_cycles = 750_000_000_000;
    assert_eq!(identity.init_arg, relay_init_arg(&old, &setup));
    assert_ne!(identity.init_arg, relay_init_arg(&changed, &setup));
    assert_eq!(identity.original_create_cycles, old.relay_initial_cycles);

    state::set_relay_legacy_identity(Vec::new());
    insert_entry(key, RelaySetupEntry::Creating(original));
    let guard = admission::Guard::acquire(key, admission::Lane::Completion).unwrap();
    assert!(matches!(
        block_on(recovery::pin_existing_paid_identity(
            key,
            &setup,
            &changed,
            self_canister_id(),
            &clients,
            guard.ticket()
        )),
        Err(RelaySetupNotifyResult::ManualRecoveryRequired { .. })
    ));
    assert!(state::relay_creating_keys().is_empty());
}

#[test]
fn predecessor_active_mapping_decodes_without_a_completed_journal() {
    #[derive(CandidType)]
    enum OldEntry {
        Active { relay_canister_id: Principal },
    }
    let child = Principal::from_slice(&[99]);
    let old = candid::encode_one(OldEntry::Active {
        relay_canister_id: child,
    })
    .unwrap();
    assert_eq!(
        RelaySetupEntry::from_bytes(Cow::Owned(old)),
        RelaySetupEntry::Active {
            relay_canister_id: child,
            completed: None
        }
    );
}

#[test]
fn a_legacy_payment_outside_the_verified_writer_period_is_not_a_funding_request() {
    let (setup, key, clients) = fixture(200_000_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress
        .cmc_transfer
        .as_mut()
        .unwrap()
        .created_at_time_nanos = 0;
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Unsafe(_)
    ));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::ManualRecoveryRequired { .. }
    ));
    let world = clients.0.lock().unwrap();
    assert!(world.transfer_requests.is_empty());
    assert!(world.notifications.is_empty());
    assert_eq!(world.installs, 0);
}

fn advance_retry_window() {
    TEST_NOW_NANOS.with(|clock| clock.set(clock.get() + 61_000_000_000));
    admission::set_time_for_test(admission::time_for_test() + 61);
}

#[test]
fn processing_beyond_sixteen_checks_and_a_lost_success_reconcile_the_same_block() {
    let (setup, key, clients) = fixture(111_540_000);
    for _ in 0..24 {
        clients.0.lock().unwrap().processing = true;
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        advance_retry_window();
    }
    clients.0.lock().unwrap().lose_cmc_reply = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    advance_retry_window();
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.transfer_requests.len(), 2);
    assert_eq!(w.notifications.len(), 26);
    assert!(w
        .notifications
        .iter()
        .all(|notification| *notification == w.notifications[0]));
    assert_eq!(w.minted.len(), 1);
    assert_eq!(w.creates, 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
}

#[test]
fn definitive_temporary_no_spend_rejections_do_not_exhaust_conversion_capacity() {
    let (setup, key, clients) = fixture(111_540_000);
    for _ in 0..12 {
        clients.0.lock().unwrap().reject_ledger_once = true;
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        advance_retry_window();
        let w = clients.0.lock().unwrap();
        assert!(w.transfers.is_empty());
        assert!(w.notifications.is_empty());
        assert_eq!(w.balance, 111_540_000);
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfer_requests.len(), 14);
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
}

#[test]
fn a_second_proven_install_funding_rejection_can_pause_and_continue_without_replaying_work() {
    let (setup, key, clients) = fixture(111_540_000);
    clients.0.lock().unwrap().funding_install_rejections = 2;
    for expected_installs in 1..=2 {
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        assert_eq!(clients.0.lock().unwrap().installs, expected_installs);
        assert!(matches!(
            quote(&setup, key, &clients),
            RelayRecoveryView::CheckPending(_)
        ));
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        assert_eq!(clients.0.lock().unwrap().installs, expected_installs);
        advance_retry_window();
        let RelayRecoveryView::FundingRequired(q) = quote(&setup, key, &clients) else {
            panic!("proven residual funding quote")
        };
        assert!(q.additional_e8s > 0);
        clients.0.lock().unwrap().balance += q.additional_e8s;
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.installs, 3);
    assert_eq!(w.creates, 1);
    assert_eq!(w.notifications.len(), 3);
    assert_eq!(w.transfers.len(), 4);
    assert_eq!(w.final_funding, 105_020_000);
    assert_eq!(w.balance, 0);
}

#[test]
fn polling_two_paid_setups_cannot_consume_continuation_admission() {
    let (setup, key, clients) = fixture(111_540_000);
    let other = CanonicalRelaySetup::canonicalize(
        vec![Principal::from_slice(&[2])],
        setup.surplus_recipients().to_vec(),
    )
    .unwrap();
    insert_entry(other.key(), get_entry(key).unwrap());
    for (index, second) in [false, true, false, true, false, true, false, true]
        .into_iter()
        .enumerate()
    {
        admission::set_time_for_test([0, 1, 12, 13, 24, 25, 36, 37][index]);
        let selected = if second { &other } else { &setup };
        assert!(matches!(
            quote(selected, selected.key(), &clients),
            RelayRecoveryView::Ready(_)
        ));
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(w.installs, 1);
}

#[test]
fn ledger_reply_loss_beyond_eight_dispatches_reuses_the_original_identity() {
    let (setup, key, clients) = fixture(111_540_000);
    {
        let mut w = clients.0.lock().unwrap();
        w.lose_ledger_reply = true;
        w.lose_duplicate_replies = 12;
    }
    for _ in 0..13 {
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        advance_retry_window();
    }
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::Active { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.transfers.len(), 2);
    assert_eq!(w.transfer_requests.len(), 15);
    assert!(w.transfer_requests[..14]
        .iter()
        .all(|arg| arg == &w.transfer_requests[0]));
    assert_eq!(w.notifications.len(), 1);
    assert_eq!(w.installs, 1);
    assert_eq!(w.balance, 0);
}

#[test]
fn retry_backoff_survives_serialization_and_never_masks_expired_uncertain_spending() {
    let (setup, key, clients) = fixture(111_540_000);
    clients.0.lock().unwrap().lose_ledger_reply = true;
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    let entry = get_entry(key).unwrap();
    insert_entry(
        key,
        RelaySetupEntry::from_bytes(Cow::Owned(entry.into_bytes())),
    );
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::CheckPending(_)
    ));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::InProgress { .. }
    ));
    assert_eq!(clients.0.lock().unwrap().transfer_requests.len(), 1);
    let Some(RelaySetupEntry::Paused(mut progress)) = get_entry(key) else {
        panic!()
    };
    progress.continuation.as_mut().unwrap().retry_after_nanos = Some(u64::MAX);
    insert_entry(key, RelaySetupEntry::Paused(progress));
    TEST_NOW_NANOS.with(|clock| clock.set(clock.get() + 24 * 60 * 60 * 1_000_000_000));
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Unsafe(_)
    ));
    assert_eq!(clients.0.lock().unwrap().transfer_requests.len(), 1);
}

#[test]
fn an_interrupted_install_without_a_definitive_funding_rejection_cannot_be_repeated() {
    let (setup, key, clients) = fixture(111_540_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut progress)) = get_entry(key) else {
        panic!()
    };
    let mut continuation = recovery::RelayContinuation::new(&tests::config(), &setup);
    continuation.identity.original_create_cycles = 750_000_000_000;
    continuation.install_retried = true;
    continuation.funding_pause = false;
    progress.continuation = Some(continuation);
    insert_entry(key, RelaySetupEntry::ManualRecoveryRequired(progress));
    assert!(matches!(
        quote(&setup, key, &clients),
        RelayRecoveryView::Unsafe(_)
    ));
    assert!(matches!(
        resume(&setup, key, &clients),
        RelaySetupNotifyResult::ManualRecoveryRequired { .. }
    ));
    let w = clients.0.lock().unwrap();
    assert_eq!(w.installs, 0);
    assert!(w.transfers.is_empty());
}

#[test]
fn continuation_without_the_optional_backoff_field_decodes_without_losing_the_journal() {
    #[derive(CandidType)]
    struct FrozenContinuation {
        identity: recovery::RelayInstallationIdentity,
        conversions: Vec<recovery::RecoveryConversion>,
        install_retried: bool,
        funding_pause: bool,
        terminal: bool,
        attempt: u64,
    }
    #[derive(CandidType)]
    struct FrozenProgress {
        phase: RelayCreationPhase,
        cmc_transfer: Option<RelayTransferRecord>,
        cycles_minted: Option<u128>,
        create_dispatched_at_ts: Option<u64>,
        relay_canister_id: Option<Principal>,
        relay_funding_transfer: Option<RelayTransferRecord>,
        last_error: Option<String>,
        continuation: Option<FrozenContinuation>,
    }
    #[derive(CandidType)]
    enum FrozenEntry {
        Paused(FrozenProgress),
    }
    let (setup, key, _) = fixture(111_540_000);
    let Some(RelaySetupEntry::ManualRecoveryRequired(mut p)) = get_entry(key) else {
        panic!()
    };
    let mut c = recovery::RelayContinuation::new(&tests::config(), &setup);
    c.funding_pause = true;
    c.install_retried = true;
    c.attempt = 42;
    p.continuation = Some(c.clone());
    let old = FrozenEntry::Paused(FrozenProgress {
        phase: p.phase,
        cmc_transfer: p.cmc_transfer.clone(),
        cycles_minted: p.cycles_minted,
        create_dispatched_at_ts: p.create_dispatched_at_ts,
        relay_canister_id: p.relay_canister_id,
        relay_funding_transfer: p.relay_funding_transfer.clone(),
        last_error: p.last_error.clone(),
        continuation: Some(FrozenContinuation {
            identity: c.identity,
            conversions: c.conversions,
            install_retried: c.install_retried,
            funding_pause: c.funding_pause,
            terminal: c.terminal,
            attempt: c.attempt,
        }),
    });
    let decoded = RelaySetupEntry::from_bytes(Cow::Owned(candid::encode_one(old).unwrap()));
    assert_eq!(decoded, RelaySetupEntry::Paused(p));
}

#[test]
fn saturated_retry_diagnostics_cannot_prevent_safe_reconciliation() {
    for ledger_loss in [false, true] {
        let (setup, key, clients) = fixture(111_540_000);
        {
            let mut w = clients.0.lock().unwrap();
            w.lose_ledger_reply = ledger_loss;
            w.processing = !ledger_loss;
        }
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::InProgress { .. }
        ));
        let Some(RelaySetupEntry::Paused(mut p)) = get_entry(key) else {
            panic!()
        };
        let operation = &mut p.continuation.as_mut().unwrap().conversions[0];
        operation.dispatches = u8::MAX;
        operation.notifications = u8::MAX;
        insert_entry(key, RelaySetupEntry::Paused(p));
        advance_retry_window();
        assert!(matches!(
            resume(&setup, key, &clients),
            RelaySetupNotifyResult::Active { .. }
        ));
        let Some(RelaySetupEntry::Active {
            completed: Some(p), ..
        }) = get_entry(key)
        else {
            panic!()
        };
        let operation = &p.continuation.as_ref().unwrap().conversions[0];
        assert_eq!(operation.dispatches, u8::MAX);
        assert_eq!(operation.notifications, u8::MAX);
        let w = clients.0.lock().unwrap();
        assert_eq!(w.transfers.len(), 2);
        assert_eq!(w.notifications.len(), if ledger_loss { 1 } else { 2 });
        assert_eq!(w.installs, 1);
        assert_eq!(w.balance, 0);
    }
}
