fn log_error(code: u32) {
    ic_cdk::println!("ERR:{}", code);
}

fn log_cycles() {
    #[cfg(test)]
    {
        return;
    }
    #[cfg(not(test))]
    {
        let cycles: u128 = ic_cdk::api::canister_cycle_balance();
        ic_cdk::println!("Cycles: {}", cycles);
    }
}

fn log_current_config() {
    let line = state::with_state(|st| state::runtime_config_log_line(&st.config));
    ic_cdk::println!("{}", line);
}

fn self_canister_principal() -> Principal {
    #[cfg(test)]
    {
        Principal::anonymous()
    }
    #[cfg(not(test))]
    {
        ic_cdk::api::canister_self()
    }
}

fn should_clear_payout_plan_on_transfer_error(err: &TransferError) -> bool {
    match err {
        TransferError::TemporarilyUnavailable => false,
        // Duplicate is handled as success at the match-site above, so if it ever reaches here
        // we still choose the conservative "do not clear" behavior.
        TransferError::Duplicate { .. } => false,
        // Policy choice: treat non-transport, non-duplicate typed ledger rejections as terminal
        // for the current persisted plan so the canister does not wedge forever retrying the same
        // identity. A later tick will rebuild from the current staging balance. If some earlier
        // transfers in the same plan already succeeded, that rebuild uses the remaining balance
        // rather than preserving the exact original split.
        TransferError::BadFee { .. }
        | TransferError::BadBurn { .. }
        | TransferError::InsufficientFunds { .. }
        | TransferError::TooOld
        | TransferError::CreatedInFuture { .. }
        | TransferError::GenericError { .. } => true,
    }
}
