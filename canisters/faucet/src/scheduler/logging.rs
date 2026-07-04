use super::*;
#[cfg(test)]
thread_local! {
    pub(super) static TEST_LOG_LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn emit_log_line(line: String) {
    #[cfg(test)]
    {
        TEST_LOG_LINES.with(|logs| logs.borrow_mut().push(line));
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("{}", line);
    }
}

pub(super) fn log_error(code: u32) {
    emit_log_line(format!("ERR:{}", code));
}
pub(super) fn log_cycles() {
    #[cfg(not(test))]
    {
        let cycles: u128 = ic_cdk::api::canister_cycle_balance();
        emit_log_line(format!("Cycles: {}", cycles));
    }
}

pub(super) fn format_summary_log(summary: &state::Summary) -> String {
    format!(
        "SUMMARY:funding_tx_id={} funding_amount_e8s={} pot_start_e8s={} round_end_latest_tx_id={} round_end_time_nanos={} effective_denom_e8s={} last_processed_funding_tx_id={} topped_up_count={} topped_up_sum_e8s={} failed_topups={} ambiguous_topups={} ignored_under_threshold={} ignored_bad_memo={} remainder_to_self_e8s={} pot_remaining_e8s={}",
        summary.funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        summary.funding_amount_e8s.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        summary.pot_start_e8s,
        summary.round_end_latest_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        summary.round_end_time_nanos.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        summary.effective_denom_staking_balance_e8s.unwrap_or(summary.denom_staking_balance_e8s),
        summary.last_processed_funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        summary.topped_up_count,
        summary.topped_up_sum_e8s,
        summary.failed_topups,
        summary.ambiguous_topups,
        summary.ignored_under_threshold,
        summary.ignored_bad_memo,
        summary.remainder_to_self_e8s,
        summary.pot_remaining_e8s
    )
}

pub(super) fn log_summary(summary: &state::Summary) {
    emit_log_line(format_summary_log(summary));
}

pub(super) fn log_current_config() {
    let line = state::with_state(|st| state::runtime_config_log_line(&st.config));
    emit_log_line(line);
}

pub(super) fn log_current_state() {
    let line = state::with_state(|st| state::runtime_state_log_line(st));
    emit_log_line(line);
}

pub(super) fn format_funding_discovery_log(
    discovery: FundingDiscovery,
    last_processed_funding_tx_id: Option<u64>,
) -> String {
    match discovery {
        FundingDiscovery::Found(tranche) => format!(
            "FUNDING:result=found tx_id={} amount_e8s={} timestamp_nanos={} last_processed_funding_tx_id={}",
            tranche.tx_id,
            tranche.amount_e8s,
            tranche.timestamp_nanos,
            last_processed_funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        ),
        FundingDiscovery::Empty => format!(
            "FUNDING:result=empty last_processed_funding_tx_id={}",
            last_processed_funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        ),
        FundingDiscovery::InProgress => state::with_state(|st| {
            let scan = st.active_funding_scan.as_ref();
            format!(
                "FUNDING:result=in_progress cursor={} candidate_tx_id={} candidate_amount_e8s={} anchor_last_processed_funding_tx_id={}",
                scan.and_then(|scan| scan.cursor).map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
                scan.and_then(|scan| scan.candidate).map(|candidate| candidate.tx_id.to_string()).unwrap_or_else(|| "none".to_string()),
                scan.and_then(|scan| scan.candidate).map(|candidate| candidate.amount_e8s.to_string()).unwrap_or_else(|| "none".to_string()),
                scan.and_then(|scan| scan.anchor_last_processed_funding_tx_id).map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
            )
        }),
        FundingDiscovery::Unreadable(reason) => format!(
            "FUNDING:result=unreadable reason={reason:?} last_processed_funding_tx_id={}",
            last_processed_funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
        ),
    }
}

pub(super) fn log_funding_discovery(
    discovery: FundingDiscovery,
    last_processed_funding_tx_id: Option<u64>,
) {
    emit_log_line(format_funding_discovery_log(
        discovery,
        last_processed_funding_tx_id,
    ));
}

pub(super) fn log_funding_balance_mismatch(
    funding_tranche: FundingTranche,
    payout_balance_e8s: u64,
    last_processed_funding_tx_id: Option<u64>,
) {
    emit_log_line(format!(
        "FUNDING:result=balance_mismatch funding_tx_id={} funding_amount_e8s={} payout_balance_e8s={} last_processed_funding_tx_id={}",
        funding_tranche.tx_id,
        funding_tranche.amount_e8s,
        payout_balance_e8s,
        last_processed_funding_tx_id.map(|v| v.to_string()).unwrap_or_else(|| "none".to_string()),
    ));
}
