use icrc_ledger_types::icrc1::account::Subaccount;

/// Deterministic per-disbursement staging subaccount.
///
/// Layout:
/// - bytes[0..8]  = b"MATSPLT\0"
/// - bytes[8..24] = 0
/// - bytes[24..32]= pending_id (big-endian)
pub fn staging_subaccount(pending_id: u64) -> Subaccount {
    let mut s = [0u8; 32];
    s[0..8].copy_from_slice(b"MATSPLT\0");
    s[24..32].copy_from_slice(&pending_id.to_be_bytes());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_unique() {
        let a = staging_subaccount(1);
        let b = staging_subaccount(1);
        let c = staging_subaccount(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
