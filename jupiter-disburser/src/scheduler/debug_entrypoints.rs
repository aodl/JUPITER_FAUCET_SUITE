#[cfg(feature = "debug_api")]
pub async fn debug_main_tick_impl() {
    main_tick(true).await;
}

#[cfg(feature = "debug_api")]
pub async fn debug_rescue_tick_impl() {
    rescue_tick().await;
}

#[cfg(feature = "debug_api")]
pub async fn debug_execute_payout_plan_impl() -> bool {
    let now_nanos = ic_cdk::api::time() as u64;
    let now_secs = now_nanos / 1_000_000_000;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);
    process_payout(&ledger, &cfg, now_nanos, now_secs).await
}

#[cfg(feature = "debug_api")]
pub async fn debug_build_payout_plan_impl() -> bool {
    let now_nanos = ic_cdk::api::time() as u64;
    let cfg = state::with_state(|st| st.config.clone());
    let ledger = IcrcLedgerCanister::new(cfg.ledger_canister_id);

    let staging = Account {
        owner: self_canister_principal(),
        subaccount: None,
    };

    let balance = match ledger.balance_of_e8s(staging).await {
        Ok(b) => b,
        Err(_) => return false,
    };

    if balance == 0 {
        state::with_state_mut(|st| st.payout_plan = None);
        return true;
    }

    let already = state::with_state(|st| st.payout_plan.is_some());
    if already {
        return true;
    }

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

    true
}
