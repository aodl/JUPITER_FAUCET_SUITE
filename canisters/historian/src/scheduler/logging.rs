use super::*;
pub(super) fn nat_to_u128(n: &Nat) -> Option<u128> {
    use num_traits::ToPrimitive;
    n.0.to_u128()
}

pub(super) fn log_cycles_once_per_week(cycles: u128) {
    #[cfg(test)]
    {
        let _ = cycles;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("Cycles: {}", cycles);
    }
}

pub(super) fn log_current_config() {
    #[cfg(test)]
    {}
    #[cfg(not(test))]
    {
        let line = state::with_state(|st| state::runtime_config_log_line(&st.config));
        ic_cdk::println!("{}", line);
    }
}

pub(super) fn log_error(message: &str) {
    #[cfg(test)]
    {
        let _ = message;
    }
    #[cfg(not(test))]
    {
        ic_cdk::println!("ERR:{}", message);
    }
}

pub(super) fn latch_commitment_index_fault(now_secs: u64, last_cursor_tx_id: Option<u64>, offending_tx_id: u64, message: String) -> String {
    state::with_root_state_mut(|st| {
        match st.commitment_index_fault.as_mut() {
            Some(existing) => {
                existing.last_cursor_tx_id = last_cursor_tx_id;
                existing.offending_tx_id = offending_tx_id;
                existing.message = message.clone();
            }
            None => {
                st.commitment_index_fault = Some(CommitmentIndexFault {
                    observed_at_ts: now_secs,
                    last_cursor_tx_id,
                    offending_tx_id,
                    message: message.clone(),
                });
            }
        }
    });
    message
}
