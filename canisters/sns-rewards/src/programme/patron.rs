//! One canonical commitment cursor and one-attempt journal for direct rewards
//! and authenticated aggregate Relay pots.
use super::{discovery, storage, types::*};
use candid::Nat;
use ic_cdk::call::{Call, CallFailed};
use icrc_ledger_types::icrc1::{
    account::Account,
    transfer::{TransferArg, TransferError},
};
use jupiter_ic_clients::{
    account_identifier::account_identifier_bytes,
    index::{IcpIndexCanister, IndexOperation},
    ledger::IcrcLedgerCanister,
};

#[cfg(feature = "debug_api")]
thread_local! { static TRAP_CALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[cfg(feature = "debug_api")]
pub(crate) fn trap_callback_once() {
    TRAP_CALLBACK.with(|flag| flag.set(true));
}

fn is_minting_destination(account: Account, governance: candid::Principal) -> bool {
    account.owner == governance && account.subaccount.is_none_or(|bytes| bytes == [0; 32])
}

pub fn finish(record: &PatronRecord) {
    if matches!(
        record.outcome,
        PatronOutcome::Prepared
            | PatronOutcome::Dispatched { .. }
            | PatronOutcome::AwaitingRelayMembership { .. }
    ) {
        return;
    }
    super::refresh::enqueue(record);
    storage::mutate(|s| {
        if s.pending_block != Some(record.contribution_block) {
            return;
        }
        match record.outcome {
            PatronOutcome::Verified { .. } => s.verified_e8s += u128::from(record.amount_e8s),
            PatronOutcome::AssumedPaidUnverified { .. } => {
                s.assumed_e8s += u128::from(record.amount_e8s)
            }
            PatronOutcome::Failed { .. } => s.failed_items = s.failed_items.saturating_add(1),
            _ => {}
        }
        s.pending_block = None;
        if let Some(retry) = s
            .relay_retries
            .as_mut()
            .filter(|r| r.dispatching_block == Some(record.contribution_block))
        {
            retry.dispatching_block = None;
            retry.outstanding = retry
                .outstanding
                .checked_sub(1)
                .expect("deferred count underflow");
            if retry.outstanding == 0 {
                retry.cursor = None;
                retry.end_block = None;
            }
        } else {
            let batch = s.batch.as_mut().expect("pinned patron batch");
            batch.cursor = Some(record.contribution_block);
        }
    });
}

fn advance(block: u64) {
    storage::mutate(|s| s.batch.as_mut().expect("pinned patron batch").cursor = Some(block));
}

fn complete_batch() {
    storage::mutate(|s| {
        let batch = s.batch.take().expect("completed patron batch");
        s.completed_end_block = Some(batch.end_block);
    });
}

fn canonical_empty_prefix(end_block: u64, oldest: Option<u64>) -> bool {
    // An account's first-ever transfer may arrive after the finite global end
    // was frozen. The canonical oldest anchor then proves this prefix empty.
    oldest.is_none_or(|first| first > end_block)
}

pub async fn tick(now: u64) -> Result<(), String> {
    let state = storage::read();
    let Some(config) = state.config else {
        return Ok(());
    };
    if !config.patron_enabled {
        return Ok(());
    }
    if config.reserve_transport_neuron.is_some()
        && !matches!(
            super::reserve::read(),
            super::reserve::ReserveTransport::Verified { .. }
        )
    {
        return Err("patron issuance waits for verified genesis reserve availability".into());
    }
    if let Some(block) = state.pending_block {
        let record = storage::record(block).ok_or("missing dispatched patron record")?;
        if matches!(record.outcome, PatronOutcome::Prepared) {
            storage::mutate(|s| s.pending_block = None);
            return Ok(()); // never advance an opportunity that was not dispatched
        }
        // A surviving message checkpoint is not permission for another attempt.
        // recover_dispatch handles upgrade; a callback trap reaches here too.
        if matches!(record.outcome, PatronOutcome::Dispatched { .. }) {
            let mut assumed = record;
            assumed.outcome = PatronOutcome::AssumedPaidUnverified {
                reason: "callback did not finalize; automated opportunity consumed".into(),
            };
            storage::put(&assumed);
            finish(&assumed);
            return Ok(());
        }
        finish(&record);
        return Ok(());
    }
    // A retry failure must not prevent the primary history lane from advancing.
    let retry = retry_relay(&config, now).await;
    let history = history_tick(config, now).await;
    retry.and(history)
}

async fn history_tick(config: ProgrammeConfig, now: u64) -> Result<(), String> {
    let state = storage::read();
    if state.batch.is_none() {
        let snapshot = state
            .snapshot
            .ok_or("waiting for complete jUP recognition")?;
        if !snapshot.valid_for(&config, now) {
            return Err("waiting for a complete fresh ownership snapshot".into());
        }
        let ledger_id: candid::Principal = Call::bounded_wait(config.icp_index, "ledger_id")
            .await
            .map_err(|e| format!("Index membership: {e:?}"))?
            .candid()
            .map_err(|e| format!("Index ledger decode: {e:?}"))?;
        if ledger_id != config.icp_ledger {
            return Err("ICP Index belongs to another Ledger".into());
        }
        let chain_length = IcrcLedgerCanister::new(config.icp_ledger)
            .chain_length()
            .await
            .map_err(|e| e.to_string())?;
        if chain_length == 0 {
            return Ok(());
        }
        let synced = IcpIndexCanister::new(config.icp_index)
            .status()
            .await
            .map_err(|e| e.to_string())?;
        if synced.num_blocks_synced < chain_length {
            return Err("waiting for canonical ICP history to be indexed".into());
        }
        let current = storage::read();
        if current.config.as_ref() != Some(&config)
            || current.batch.is_some()
            || current.snapshot.as_ref() != Some(&snapshot)
        {
            return Ok(());
        }
        if current.completed_end_block == Some(chain_length - 1) {
            return Ok(());
        }
        // This finite range and complete generation survive every restart.
        storage::mutate(|s| {
            s.batch = Some(Batch {
                snapshot,
                end_block: chain_length - 1,
                previous_end: current.completed_end_block,
                cursor: None,
                oldest: None,
            })
        });
    }
    let batch = storage::read().batch.ok_or("batch unavailable")?;
    if !batch.snapshot.matches_configuration(&config) {
        return Err("pinned batch does not bind a complete single-source jUP generation".into());
    }
    if batch.cursor.is_some_and(|id| {
        Some(id) == batch.oldest || batch.previous_end.is_some_and(|old| id <= old)
    }) {
        complete_batch();
        return Ok(());
    }
    let account = hex::encode(account_identifier_bytes(
        config.staking_account.owner,
        config.staking_account.subaccount,
    ));
    let cursor = Some(
        batch.cursor.unwrap_or(
            batch
                .end_block
                .checked_add(1)
                .ok_or("ICP block index exhausted")?,
        ),
    );
    let page = IcpIndexCanister::new(config.icp_index)
        .get_account_identifier_transactions(account.clone(), cursor, 1)
        .await
        .map_err(|e| e.to_string())?;
    if storage::read().batch.as_ref() != Some(&batch) {
        return Ok(());
    }
    if page.transactions.len() > 1 {
        return Err("oversized patron history page".into());
    }
    let Some(tx) = page.transactions.first() else {
        if batch.cursor.is_none() && canonical_empty_prefix(batch.end_block, page.oldest_tx_id) {
            complete_batch();
            return Ok(());
        }
        return Err("partial history is not canonical exhaustion".into());
    };
    if tx.id > batch.end_block
        || cursor.is_some_and(|c| tx.id >= c)
        || page.oldest_tx_id.is_none()
        || batch
            .oldest
            .is_some_and(|old| Some(old) != page.oldest_tx_id)
    {
        return Err("non-progressing or inconsistent patron history".into());
    }
    storage::mutate(|s| s.batch.as_mut().expect("fenced batch").oldest = page.oldest_tx_id);
    if batch.previous_end.is_some_and(|old| tx.id <= old) {
        complete_batch();
        return Ok(());
    }
    if storage::record(tx.id).is_some_and(|r| !matches!(r.outcome, PatronOutcome::Prepared)) {
        // Retained deferred/terminal identities cannot be assessed twice, even if
        // canonical history is observed again. Prepared has never been sent.
        advance(tx.id);
        return Ok(());
    }
    let IndexOperation::Transfer {
        from,
        to,
        amount,
        spender,
        ..
    } = &tx.transaction.operation
    else {
        advance(tx.id);
        return Ok(());
    };
    if !to.eq_ignore_ascii_case(&account) {
        advance(tx.id);
        return Ok(());
    }
    let memo = tx.transaction.icrc1_memo.as_deref().unwrap_or_default();
    if amount.e8s() < crate::economics::get().minimum_faucet_commitment_e8s
        || jupiter_memo_policy::parse_memo_directive(memo).is_none()
    {
        advance(tx.id);
        return Ok(());
    }
    let sender: [u8; 32] = hex::decode(from)
        .map_err(|_| "invalid canonical sender")?
        .try_into()
        .map_err(|_| "invalid canonical sender length")?;
    let relay_candidate = super::relay::candidate(sender, memo);
    let mut awaiting = None;
    let route = if let Some(relay) = relay_candidate {
        if spender.is_some() {
            None
        } else {
            match super::relay::classify(&config, tx.id, relay, None).await {
                super::relay::Classification::Ready(route) => route,
                super::relay::Classification::Awaiting { lookup, reason } => {
                    awaiting = Some(PatronOutcome::AwaitingRelayMembership { lookup, reason });
                    Some(PatronRoute::PendingRelay { relay })
                }
            }
        }
    } else {
        Some(PatronRoute::Direct)
    };
    let timestamp = tx
        .transaction
        .timestamp
        .as_ref()
        .ok_or("missing canonical commitment block time")?
        .timestamp_nanos
        / 1_000_000_000;
    let source = &batch.snapshot.sources[0];
    let reserve = Account {
        owner: ic_cdk::api::canister_self(),
        subaccount: config.reserve_subaccount,
    };
    let owner = storage::lookup(batch.snapshot.slot, sender);
    let excluded = config
        .excluded_icp_accounts
        .iter()
        .any(|a| a.as_slice() == sender)
        || [reserve.owner, source.governance, config.canonical_relay]
            .into_iter()
            .any(|p| account_identifier_bytes(p, None) == sender);
    let skip = if relay_candidate.is_some() && route.is_none() {
        Some("unsupported Relay sender or spender")
    } else if matches!(
        route,
        Some(PatronRoute::RelayPot { .. } | PatronRoute::PendingRelay { .. })
    ) {
        None
    } else if excluded {
        Some("protocol capital excluded")
    } else if owner.is_none() {
        Some("absent from the pinned complete ownership snapshot; final")
    } else {
        None
    };
    let mut record = PatronRecord {
        contribution_block: tx.id,
        snapshot_id: batch.snapshot.id,
        ownership_snapshot: batch.snapshot.clone(),
        recognition: if matches!(route, Some(PatronRoute::Direct)) {
            owner.clone()
        } else {
            None
        },
        route: route.unwrap_or_else(|| PatronRoute::UnsupportedRelay {
            claimed_relay: relay_candidate.expect("unsupported Relay candidate"),
        }),
        relay_factory: config.relay_factory,
        icp_ledger: config.icp_ledger,
        staking_account: config.staking_account,
        source_spender: spender
            .as_ref()
            .map(|s| hex::decode(s).map_err(|_| "invalid canonical spender"))
            .transpose()?,
        commitment_memo: memo.to_vec(),
        config_version: config.version,
        contributor_icp_account: sender.to_vec(),
        commitment_timestamp_seconds: timestamp,
        qualifying_icp_e8s: amount.e8s(),
        launch_timestamp_seconds: config
            .launch_timestamp_seconds
            .ok_or("launch timestamp unresolved")?,
        compounded_neuron: None,
        ledger: source.ledger,
        from: reserve,
        to: reserve,
        amount_e8s: 0,
        fee_e8s: 0,
        memo: [
            b"jUP-patron-v1".as_slice(),
            &tx.id.to_be_bytes(),
            &batch.snapshot.id.to_be_bytes(),
        ]
        .concat(),
        created_at_time_nanos: ic_cdk::api::time(),
        outcome: PatronOutcome::Prepared,
        refresh_result: None,
    };
    if let Some(reason) = skip {
        record.outcome = PatronOutcome::Skipped {
            reason: reason.into(),
        };
    } else if let Some(outcome) = awaiting {
        if storage::read().config.as_ref() != Some(&config) {
            return Ok(());
        }
        record.outcome = outcome;
        storage::defer_relay(&record);
        advance(record.contribution_block);
        return Ok(());
    } else {
        prepare_payment(&config, &mut record).await?;
    }
    if storage::read().config.as_ref() != Some(&config) {
        return Ok(());
    }
    dispatch(record, false).await
}

async fn prepare_payment(
    config: &ProgrammeConfig,
    record: &mut PatronRecord,
) -> Result<(), String> {
    let direct_owner = matches!(record.route, PatronRoute::Direct)
        .then(|| record.recognition.clone().expect("recognised owner"));
    let source = &record.ownership_snapshot.sources[0];
    let reserve = record.from;
    let ledger = IcrcLedgerCanister::new(source.ledger);
    let fee = ledger.fee_e8s().await.map_err(|e| e.to_string())?;
    let balance = ledger
        .balance_of_e8s(reserve)
        .await
        .map_err(|e| e.to_string())?;
    // Last external prerequisite: recheck the selected jUP neuron immediately
    // before freezing the payment. A revoked preference receives base liquid.
    let compound = if let Some((owner, id)) = direct_owner
        .as_ref()
        .and_then(|o| o.compound_neuron.clone().map(|id| (o, id)))
    {
        match discovery::lookup_neuron(source.governance, id.clone()).await? {
            Some(neuron) => {
                if neuron.id.as_ref().map(|n| &n.id) != Some(&id) {
                    return Err("selected neuron lookup returned a different identity".into());
                }
                discovery::compound_eligible(
                    &neuron,
                    owner.principal,
                    config,
                    ic_cdk::api::canister_self(),
                )
                .then_some(id)
            }
            None => None, // a canonical NotFound is a definitive loss of bonus eligibility
        }
    } else {
        None
    };
    record.amount_e8s = crate::patron_math::reward_e8s(
        record.qualifying_icp_e8s,
        record.commitment_timestamp_seconds,
        record.launch_timestamp_seconds,
        compound.is_some(),
    );
    record.fee_e8s = fee;
    record.to = if let Some(id) = &compound {
        Account {
            owner: source.governance,
            subaccount: Some(id.clone().try_into().map_err(|_| "neuron ID length")?),
        }
    } else {
        Account {
            owner: match &record.route {
                PatronRoute::Direct => {
                    direct_owner
                        .as_ref()
                        .expect("recognised direct owner")
                        .principal
                }
                PatronRoute::RelayPot { relay, .. } => *relay,
                PatronRoute::UnsupportedRelay { .. } | PatronRoute::PendingRelay { .. } => {
                    return Err("unsupported Relay cannot be dispatched".into())
                }
            },
            subaccount: None,
        }
    };
    if is_minting_destination(record.to, source.governance) {
        return Err("Governance default/minting Account cannot receive patron rewards".into());
    }
    record.compounded_neuron = compound;
    record.outcome = PatronOutcome::Prepared;
    record.created_at_time_nanos = ic_cdk::api::time();
    if record.amount_e8s == 0 {
        record.outcome = PatronOutcome::Skipped {
            reason: "final eight-decimal amount rounds to zero".into(),
        };
    } else if balance
        < crate::patron_math::reserve_cost(record.amount_e8s, fee).ok_or("patron cost overflow")?
    {
        return Err("reserve funds unavailable; contribution has not been dispatched".into());
    }
    Ok(())
}

async fn dispatch(mut record: PatronRecord, retry: bool) -> Result<(), String> {
    storage::put(&record);
    storage::mutate(|s| {
        s.pending_block = Some(record.contribution_block);
        if retry {
            s.relay_retries
                .as_mut()
                .expect("relay retry state")
                .dispatching_block = Some(record.contribution_block);
        }
    });
    if matches!(record.outcome, PatronOutcome::Skipped { .. }) {
        finish(&record);
        return Ok(());
    }
    // Durable checkpoint precedes the actual message boundary. Never retry it.
    record.outcome = PatronOutcome::Dispatched {
        at_nanos: ic_cdk::api::time(),
    };
    storage::put(&record);
    #[cfg(feature = "debug_api")]
    let trap_callback = TRAP_CALLBACK.with(|flag| flag.replace(false));
    let reply = Call::bounded_wait(record.ledger, "icrc1_transfer")
        .with_arg(TransferArg {
            from_subaccount: record.from.subaccount,
            to: record.to,
            amount: Nat::from(record.amount_e8s),
            fee: Some(Nat::from(record.fee_e8s)),
            memo: Some(record.memo.clone().into()),
            created_at_time: Some(record.created_at_time_nanos),
        })
        .change_timeout(60)
        .await;
    if matches!(
        &reply,
        Err(CallFailed::InsufficientLiquidCycleBalance(_) | CallFailed::CallPerformFailed(_))
    ) {
        let reason = format!("patron call was not dispatched: {:?}", reply.err());
        record.outcome = if retry {
            PatronOutcome::AwaitingRelayMembership {
                lookup: None,
                reason: reason.clone(),
            }
        } else {
            PatronOutcome::Prepared
        };
        storage::put(&record);
        storage::mutate(|s| {
            s.pending_block = None;
            if retry {
                s.relay_retries
                    .as_mut()
                    .expect("relay retry state")
                    .dispatching_block = None;
            }
        });
        return Err(reason);
    }
    let result: Result<Result<Nat, TransferError>, String> = reply
        .map_err(|error| error.to_string())
        .and_then(|response| response.candid().map_err(|error| error.to_string()));
    #[cfg(feature = "debug_api")]
    if trap_callback {
        ic_cdk::trap("debug patron callback trap after actual Ledger dispatch");
    }
    record.outcome = match result {
        Ok(Ok(block)) => PatronOutcome::Verified {
            block,
            duplicate: false,
        },
        Ok(Err(TransferError::Duplicate { duplicate_of })) => PatronOutcome::Verified {
            block: duplicate_of,
            duplicate: true,
        },
        Ok(Err(error)) => PatronOutcome::Failed {
            error: format!("{error:?}"),
        },
        Err(error) => PatronOutcome::AssumedPaidUnverified {
            reason: error.to_string(),
        },
    };
    if record.compounded_neuron.is_some() {
        record.refresh_result = Some("not confirmed; transfer must never be repeated".into());
    }
    storage::put(&record);
    finish(&record);
    Ok(())
}

/// Pin a finite journal end for each round. New records cannot perpetually
/// extend the current pass and starve earlier unresolved opportunities.
/// At most 32 existing journal records are inspected per invocation.
pub(super) fn select_relay_retry() -> Result<Option<PatronRecord>, String> {
    let Some(retries) = storage::read().relay_retries.filter(|r| r.outstanding > 0) else {
        return Ok(None);
    };
    let end = match retries.end_block {
        Some(end) => end,
        None => {
            let end =
                storage::last_record_block().ok_or("deferred count without journal records")?;
            storage::mutate(|s| {
                s.relay_retries
                    .as_mut()
                    .expect("relay retry state")
                    .end_block = Some(end)
            });
            end
        }
    };
    let page = storage::retry_page(retries.cursor, end, 32);
    if page.is_empty() {
        storage::mutate(|s| {
            let retry = s.relay_retries.as_mut().expect("relay retry state");
            retry.cursor = None;
            retry.end_block = None;
        });
        return Ok(None);
    }
    for record in page {
        storage::mutate(|s| {
            s.relay_retries.as_mut().expect("relay retry state").cursor =
                Some(record.contribution_block)
        });
        if matches!(
            record.outcome,
            PatronOutcome::AwaitingRelayMembership { .. }
        ) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

/// One membership page/financial preparation per tick, independent of primary
/// history progress. A permanently pending record is revisited without blocking.
async fn retry_relay(config: &ProgrammeConfig, now: u64) -> Result<(), String> {
    let Some(mut record) = select_relay_retry()? else {
        return Ok(());
    };
    let state = storage::read();
    let PatronOutcome::AwaitingRelayMembership { lookup, .. } = &record.outcome else {
        return Err("retry selected a non-deferred record".into());
    };
    let relay = match record.route {
        PatronRoute::PendingRelay { relay } | PatronRoute::RelayPot { relay, .. } => relay,
        _ => return Err("deferred opportunity has no Relay source".into()),
    };
    let snapshot = state
        .snapshot
        .as_ref()
        .filter(|s| s.valid_for(config, now))
        .ok_or("Relay retry waits for a complete fresh jUP snapshot")?;
    if snapshot.sources != record.ownership_snapshot.sources
        || record.relay_factory != config.relay_factory
        || record.icp_ledger != config.icp_ledger
        || record.staking_account != config.staking_account
    {
        return Err("deferred Relay financial binding changed".into());
    }
    let classification =
        super::relay::classify(config, record.contribution_block, relay, lookup.clone()).await;
    if storage::read().config.as_ref() != Some(config) {
        return Ok(());
    }
    match classification {
        super::relay::Classification::Awaiting { lookup, reason } => {
            record.outcome = PatronOutcome::AwaitingRelayMembership { lookup, reason };
            storage::put(&record);
            return Ok(());
        }
        super::relay::Classification::Ready(None) => {
            record.route = PatronRoute::UnsupportedRelay {
                claimed_relay: relay,
            };
            record.outcome = PatronOutcome::Skipped {
                reason: "authoritatively unsupported Relay sender".into(),
            };
        }
        super::relay::Classification::Ready(Some(route)) => {
            record.route = route;
            if let Err(reason) = prepare_payment(config, &mut record).await {
                record.outcome = PatronOutcome::AwaitingRelayMembership {
                    lookup: None,
                    reason: reason.clone(),
                };
                storage::put(&record);
                return Err(reason);
            }
        }
    }
    if storage::read().config.as_ref() != Some(config) {
        return Ok(());
    }
    dispatch(record, true).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn governance_default_and_zero_alias_are_never_reward_destinations() {
        use icrc_ledger_types::icrc1::account::Account;
        let governance = candid::Principal::from_slice(&[1, 1]);
        for subaccount in [None, Some([0; 32])] {
            assert!(super::is_minting_destination(
                Account {
                    owner: governance,
                    subaccount
                },
                governance
            ));
        }
        assert!(!super::is_minting_destination(
            Account {
                owner: governance,
                subaccount: Some([1; 32])
            },
            governance
        ));
    }
    #[test]
    fn a_later_first_transfer_does_not_strand_an_empty_pinned_prefix() {
        assert!(super::canonical_empty_prefix(10, None));
        assert!(super::canonical_empty_prefix(10, Some(11)));
        assert!(!super::canonical_empty_prefix(10, Some(10)));
        assert!(!super::canonical_empty_prefix(10, Some(1)));
    }
}
