//! Direct commitment processing. Unknown Relay provenance is held, never treated
//! as an unmatched user or rewarded under the Relay's own principal.
use super::{discovery, storage, types::*};
use candid::{CandidType, Deserialize, Nat};
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

pub fn finish(record: &PatronRecord) {
    if matches!(
        record.outcome,
        PatronOutcome::Prepared | PatronOutcome::Dispatched { .. }
    ) {
        return;
    }
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
        let batch = s.batch.as_mut().expect("pinned patron batch");
        batch.cursor = Some(record.contribution_block);
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
    if state.batch.is_none() {
        let snapshot = state
            .snapshot
            .ok_or("waiting for both required ownership sources")?;
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
    let IndexOperation::Transfer {
        from, to, amount, ..
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
    let relay_one = {
        let mut sub = [0; 32];
        sub[31] = 1;
        account_identifier_bytes(config.canonical_relay, Some(sub))
    };
    if sender == relay_one {
        return Err(format!("Relay contribution {} awaits complete original-contributor provenance; not classified or paid", tx.id));
    }
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
    let skip = if excluded {
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
        recognition: owner.clone(),
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
    } else {
        let owner = owner.expect("recognised owner");
        let ledger = IcrcLedgerCanister::new(source.ledger);
        let fee = ledger.fee_e8s().await.map_err(|e| e.to_string())?;
        let balance = ledger
            .balance_of_e8s(reserve)
            .await
            .map_err(|e| e.to_string())?;
        // Last external prerequisite: recheck the selected jUP neuron immediately
        // before freezing the payment. A revoked preference receives base liquid.
        let compound = if let Some(id) = owner.compound_neuron {
            let neuron = discovery::get_neuron(source.governance, id.clone()).await?;
            discovery::compound_eligible(&neuron, owner.principal, &config).then_some(id)
        } else {
            None
        };
        record.amount_e8s = crate::patron_math::reward_e8s(
            amount.e8s(),
            timestamp,
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
                owner: owner.principal,
                subaccount: None,
            }
        };
        record.compounded_neuron = compound;
        if record.amount_e8s == 0 {
            record.outcome = PatronOutcome::Skipped {
                reason: "final eight-decimal amount rounds to zero".into(),
            };
        } else if balance
            < crate::patron_math::reserve_cost(record.amount_e8s, fee)
                .ok_or("patron cost overflow")?
        {
            return Err("reserve funds unavailable; contribution has not been dispatched".into());
        }
    }
    if storage::read().config.as_ref() != Some(&config) {
        return Ok(());
    }
    storage::put(&record);
    storage::mutate(|s| s.pending_block = Some(record.contribution_block));
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
        record.outcome = PatronOutcome::Prepared;
        storage::put(&record);
        storage::mutate(|s| s.pending_block = None);
        return Err(format!("patron call was not dispatched: {:?}", reply.err()));
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
    if let (PatronOutcome::Verified { .. }, Some(id)) = (&record.outcome, &record.compounded_neuron)
    {
        record.refresh_result = Some(
            refresh(source.governance, id.clone())
                .await
                .map_or_else(|e| e, |()| "confirmed stake refresh".into()),
        );
        storage::put(&record);
    }
    Ok(())
}

async fn refresh(governance: candid::Principal, id: Vec<u8>) -> Result<(), String> {
    #[derive(CandidType)]
    struct Empty {}
    #[derive(CandidType)]
    enum By {
        NeuronId(Empty),
    }
    #[derive(CandidType)]
    struct Refresh {
        by: Option<By>,
    }
    #[derive(CandidType)]
    enum Command {
        ClaimOrRefresh(Refresh),
    }
    #[derive(CandidType)]
    struct Request {
        subaccount: Vec<u8>,
        command: Option<Command>,
    }
    #[derive(CandidType, Deserialize)]
    enum Reply {
        ClaimOrRefresh(candid::Reserved),
        Error(candid::Reserved),
    }
    #[derive(CandidType, Deserialize)]
    struct Response {
        command: Option<Reply>,
    }
    let response: Response = Call::bounded_wait(governance, "manage_neuron")
        .with_arg(Request {
            subaccount: id,
            command: Some(Command::ClaimOrRefresh(Refresh {
                by: Some(By::NeuronId(Empty {})),
            })),
        })
        .await
        .map_err(|e| format!("stake refresh uncertain: {e:?}"))?
        .candid()
        .map_err(|e| format!("stake refresh decode: {e:?}"))?;
    match response.command {
        Some(Reply::ClaimOrRefresh(_)) => Ok(()),
        _ => Err("stake refresh rejected".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_later_first_transfer_does_not_strand_an_empty_pinned_prefix() {
        assert!(super::canonical_empty_prefix(10, None));
        assert!(super::canonical_empty_prefix(10, Some(11)));
        assert!(!super::canonical_empty_prefix(10, Some(10)));
        assert!(!super::canonical_empty_prefix(10, Some(1)));
    }
}
