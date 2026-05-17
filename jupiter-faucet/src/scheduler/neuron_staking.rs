use super::*;
pub(super) fn recognition_delay_seconds() -> u64 {
    state::with_state(|st| st.config.stake_recognition_delay_seconds.unwrap_or(DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS))
}

pub(super) fn effective_denom_scan_complete(job: &ActivePayoutJob) -> bool {
    job.effective_denom_scan_complete.unwrap_or(true)
}

pub(super) fn effective_denom_e8s(job: &ActivePayoutJob) -> u64 {
    job.effective_denom_staking_balance_e8s.unwrap_or(job.denom_staking_balance_e8s)
}

pub(super) fn ensure_active_job(
    now_nanos: u64,
    fee_e8s: u64,
    pot_start_e8s: u64,
    denom_e8s: u64,
    round_end_latest_tx_id: Option<u64>,
) {
    state::with_state_mut(|st| {
        if st.active_payout_job.is_some() {
            return;
        }
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        let mut job = ActivePayoutJob::new(id, fee_e8s, pot_start_e8s, denom_e8s, now_nanos);
        match (
            st.current_round_start_time_nanos,
            st.current_round_start_staking_balance_e8s,
            st.current_round_start_latest_tx_id,
        ) {
            (Some(round_start_time_nanos), Some(round_start_staking_balance_e8s), round_start_latest_tx_id) => {
                let stake_unchanged_since_round_start = round_start_staking_balance_e8s == denom_e8s;
                let effective_round_end_latest_tx_id = if stake_unchanged_since_round_start {
                    round_start_latest_tx_id
                } else {
                    round_end_latest_tx_id
                };
                job.configure_round_accounting(
                    Some(round_start_time_nanos),
                    Some(round_start_staking_balance_e8s),
                    round_start_latest_tx_id,
                    now_nanos,
                    effective_round_end_latest_tx_id,
                    round_start_staking_balance_e8s,
                    stake_unchanged_since_round_start,
                );
                if !stake_unchanged_since_round_start {
                    job.next_start = round_start_latest_tx_id;
                }
            }
            _ => {
                // Fresh installs / upgrades do not yet know the prior round boundary. Fall back to
                // the legacy live-denominator model for exactly one transition payout, then store
                // the current boundary as the next round's start snapshot when this job finalizes.
                job.configure_round_accounting(
                    Some(now_nanos),
                    Some(denom_e8s),
                    round_end_latest_tx_id,
                    now_nanos,
                    round_end_latest_tx_id,
                    denom_e8s,
                    true,
                );
            }
        }
        st.active_payout_job = Some(job);
    });
}

