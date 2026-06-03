use super::*;
pub(super) fn recognition_delay_seconds() -> u64 {
    state::with_state(|st| {
        st.config
            .stake_recognition_delay_seconds
            .unwrap_or(DEFAULT_STAKE_RECOGNITION_DELAY_SECONDS)
    })
}

pub(super) fn effective_denom_scan_complete(job: &ActivePayoutJob) -> bool {
    job.effective_denom_scan_complete.unwrap_or(true)
}

pub(super) fn effective_denom_e8s(job: &ActivePayoutJob) -> u64 {
    job.effective_denom_staking_balance_e8s
        .unwrap_or(job.denom_staking_balance_e8s)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FundingTranche {
    pub tx_id: u64,
    pub timestamp_nanos: u64,
    pub amount_e8s: u64,
}

impl From<state::FundingTrancheState> for FundingTranche {
    fn from(value: state::FundingTrancheState) -> Self {
        Self {
            tx_id: value.tx_id,
            timestamp_nanos: value.timestamp_nanos,
            amount_e8s: value.amount_e8s,
        }
    }
}

impl From<FundingTranche> for state::FundingTrancheState {
    fn from(value: FundingTranche) -> Self {
        Self {
            tx_id: value.tx_id,
            timestamp_nanos: value.timestamp_nanos,
            amount_e8s: value.amount_e8s,
        }
    }
}

pub(super) fn ensure_active_job_with_boundary(
    now_nanos: u64,
    fee_e8s: u64,
    pot_start_e8s: u64,
    denom_e8s: u64,
    round_end_time_nanos: u64,
    round_end_latest_tx_id: Option<u64>,
    funding_tranche: Option<FundingTranche>,
) {
    state::with_state_mut(|st| {
        if st.active_payout_job.is_some() {
            return;
        }
        let id = st.payout_nonce;
        st.payout_nonce = st.payout_nonce.saturating_add(1);
        let mut job = ActivePayoutJob::new(id, fee_e8s, pot_start_e8s, denom_e8s, now_nanos);
        if let Some(tranche) = funding_tranche {
            job.configure_funding_tranche(
                tranche.tx_id,
                tranche.timestamp_nanos,
                tranche.amount_e8s,
            );
        }
        match (
            st.current_round_start_time_nanos,
            st.current_round_start_staking_balance_e8s,
            st.current_round_start_latest_tx_id,
        ) {
            (
                Some(round_start_time_nanos),
                Some(round_start_staking_balance_e8s),
                round_start_latest_tx_id,
            ) => {
                let stake_unchanged_since_round_start =
                    round_start_staking_balance_e8s == denom_e8s;
                let effective_round_end_latest_tx_id = if stake_unchanged_since_round_start {
                    round_start_latest_tx_id
                } else {
                    round_end_latest_tx_id
                };
                job.configure_round_accounting(
                    Some(round_start_time_nanos),
                    Some(round_start_staking_balance_e8s),
                    round_start_latest_tx_id,
                    round_end_time_nanos,
                    effective_round_end_latest_tx_id,
                    round_start_staking_balance_e8s,
                    stake_unchanged_since_round_start,
                );
                if !stake_unchanged_since_round_start {
                    // Recognition delay means tx id alone cannot prove baseline membership.
                    // Re-scan staking history and let recognition timestamps decide whether
                    // each commitment is baseline, current-round delta, or still unrecognized.
                    job.next_start = None;
                }
            }
            _ => {
                // Fresh strict-tranche installs start from genesis. The first round must still
                // scan indexed staking history up to the funding transfer boundary; the live
                // staking balance is only an operational snapshot.
                job.configure_round_accounting(
                    None,
                    Some(0),
                    None,
                    round_end_time_nanos,
                    round_end_latest_tx_id,
                    0,
                    false,
                );
            }
        }
        st.active_payout_job = Some(job);
    });
}
