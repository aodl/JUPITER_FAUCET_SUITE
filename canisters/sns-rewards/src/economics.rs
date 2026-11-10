use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub(crate) struct Economics {
    pub decimals: u8,
    pub patron_icp_per_jup: u64,
    pub initial_supply_e8s: u64,
    pub governance_stake_e8s: u64,
    pub sale_allocation_e8s: u64,
    pub proposer_stake_e8s: u64,
    pub reserve_transport_e8s: u64,
    pub builtin_treasury_e8s: u64,
    pub year_seconds: u64,
    pub halving_days: u64,
    pub minimum_faucet_commitment_e8s: u64,
    pub proposer_low_e8s: u64,
    pub proposer_high_e8s: u64,
    pub proposal_rejection_e8s: u64,
    pub jupiter_voting_seconds: u64,
    pub target_voting_seconds: u64,
}

pub(crate) fn get() -> &'static Economics {
    static CONFIG: OnceLock<Economics> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let e: Economics =
            serde_json::from_str(include_str!("../../../deploy/acceptance/economics.json"))
                .expect("reviewed economic configuration");
        assert_eq!(e.decimals, 8);
        assert_eq!(e.patron_icp_per_jup, 10);
        assert_eq!(
            e.governance_stake_e8s
                + e.sale_allocation_e8s
                + e.proposer_stake_e8s
                + e.reserve_transport_e8s
                + e.builtin_treasury_e8s,
            e.initial_supply_e8s
        );
        assert_eq!(e.halving_days * 86_400, e.year_seconds * 4);
        assert_eq!(e.halving_days, 1461); // DAILY_FACTOR_Q96 is pinned to this programme.
        assert!(e.target_voting_seconds > e.jupiter_voting_seconds);
        e
    })
}
