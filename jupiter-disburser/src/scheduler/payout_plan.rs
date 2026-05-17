use super::*;
/// PAYOUT:
/// - uses default staging account
/// - persists a payout plan for deterministic retries
/// - plans up to 3 transfers; skips any share <= fee (leaves it in staging)
pub(super) async fn process_payout<L: LedgerClient>(
    ledger: &L,
    cfg: &state::Config,
    now_nanos: u64,
    now_secs: u64,
) -> bool {
    debug_reset_successful_transfer_counter();

    let staging = Account {
        owner: self_canister_principal(),
        subaccount: None,
    };

    let balance = match ledger.balance_of_e8s(staging).await {
        Ok(b) => b,
        Err(_) => return false,
    };

    // If empty, clear any stale plan and succeed.
    if balance == 0 {
        state::with_state_mut(|st| st.payout_plan = None);
        return true;
    }

    // Create plan if none exists.
    let need_plan = state::with_state(|st| st.payout_plan.is_none());
    if need_plan {
        let fee = match ledger.fee_e8s().await {
            Ok(f) => f,
            Err(_) => return false,
        };

        let (payout_id, prev_age) = state::with_state_mut(|st| {
            let id = st.payout_nonce;
            st.payout_nonce = st.payout_nonce.saturating_add(1);
            (id, st.prev_age_seconds)
        });

        let (_gross, planned) = logic::plan_payout_transfers(
            payout_id,
            now_nanos,
            balance,
            fee,
            prev_age,
            &cfg.normal_recipient,
            &cfg.age_bonus_recipient_1,
            &cfg.age_bonus_recipient_2,
        );

        let transfers = planned
            .into_iter()
            .map(|p| state::PlannedTransfer {
                to: p.to,
                gross_share_e8s: p.gross_share_e8s,
                amount_e8s: p.amount_e8s,
                created_at_time_nanos: p.created_at_time_nanos,
                memo: p.memo.to_vec(),
                status: state::TransferStatus::Pending,
            })
            .collect::<Vec<_>>();

        state::with_state_mut(|st| {
            st.payout_plan = Some(state::PayoutPlan {
                id: payout_id,
                fee_e8s: fee,
                created_at_base_nanos: now_nanos,
                transfers,
            });
        });
    }

    #[cfg(feature = "debug_api")]
    if debug_pause_after_planning() {
        // Keep plan persisted and force the caller to retry later.
        return false;
    }

    // Execute the plan until done or a transient error occurs.
    loop {
        let plan_opt = state::with_state(|st| st.payout_plan.clone());
        let Some(plan) = plan_opt else { return true };

        let next_idx = plan
            .transfers
            .iter()
            .position(|t| matches!(t.status, state::TransferStatus::Pending));

        let Some(i) = next_idx else {
            state::with_state_mut(|st| st.payout_plan = None);
            return true;
        };

        let t = &plan.transfers[i];
        let arg = TransferArg {
            from_subaccount: None,
            to: t.to.clone(),
            fee: Some(Nat::from(plan.fee_e8s)),
            created_at_time: Some(t.created_at_time_nanos),
            memo: Some(Memo::from(t.memo.clone())),
            amount: Nat::from(t.amount_e8s),
        };

        let res = match ledger.transfer(arg).await {
            Ok(r) => r,
            Err(_) => return false,
        };

        match res {
            Ok(block) => {
                #[cfg(feature = "debug_api")]
                match debug_successful_transfer_injection() {
                    DebugSuccessfulTransferInjection::None => {}
                    DebugSuccessfulTransferInjection::Abort => return false,
                    DebugSuccessfulTransferInjection::Trap => ic_cdk::trap("debug trap after successful disburser transfer"),
                };
                let block_str = block.to_string();
                state::with_state_mut(|st| {
                    if let Some(p) = st.payout_plan.as_mut() {
                        if let Some(tt) = p.transfers.get_mut(i) {
                            tt.status = state::TransferStatus::Sent { block_index: block_str };
                        }
                    }
                    st.last_successful_transfer_ts = Some(now_secs);
                });
            }
            Err(TransferError::Duplicate { duplicate_of }) => {
                #[cfg(feature = "debug_api")]
                match debug_successful_transfer_injection() {
                    DebugSuccessfulTransferInjection::None => {}
                    DebugSuccessfulTransferInjection::Abort => return false,
                    DebugSuccessfulTransferInjection::Trap => ic_cdk::trap("debug trap after successful disburser transfer"),
                };
                let block_str = duplicate_of.to_string();
                state::with_state_mut(|st| {
                    if let Some(p) = st.payout_plan.as_mut() {
                        if let Some(tt) = p.transfers.get_mut(i) {
                            tt.status = state::TransferStatus::Sent { block_index: block_str };
                        }
                    }
                    st.last_successful_transfer_ts = Some(now_secs);
                });
            }
            Err(err) => {
                if should_clear_payout_plan_on_transfer_error(&err) {
                    state::with_state_mut(|st| st.payout_plan = None);
                }
                return false;
            }
        }
    }
}
