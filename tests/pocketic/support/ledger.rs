use anyhow::{anyhow, bail, Result};
use candid::{Nat, Principal};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use pocket_ic::common::rest::{IcpFeatures, IcpFeaturesConfig};
use pocket_ic::PocketIc;
use slog::Level;

pub const ICP_LEDGER_FEE_E8S: u64 = 10_000;

pub fn build_pic_with_real_icp() -> PocketIc {
    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    super::pocketic::builder()
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build()
}

pub fn nat_to_u64(n: &Nat) -> Result<u64> {
    u64::try_from(n.0.clone()).map_err(|_| anyhow!("Nat does not fit into u64: {n}"))
}

pub fn icrc1_balance(pic: &PocketIc, ledger: Principal, account: &Account) -> Result<u64> {
    let balance: Nat = super::calls::query_one(
        pic,
        ledger,
        Principal::anonymous(),
        "icrc1_balance_of",
        *account,
    )?;
    nat_to_u64(&balance)
}

pub fn icrc1_fee(pic: &PocketIc, ledger: Principal) -> Result<u64> {
    let fee: Nat = super::calls::query_one(pic, ledger, Principal::anonymous(), "icrc1_fee", ())?;
    nat_to_u64(&fee)
}

pub fn icrc1_transfer(
    pic: &PocketIc,
    ledger: Principal,
    from: Principal,
    arg: TransferArg,
) -> Result<u64> {
    let result: std::result::Result<Nat, TransferError> =
        super::calls::update_one(pic, ledger, from, "icrc1_transfer", arg)?;
    match result {
        Ok(block_index) => nat_to_u64(&block_index),
        Err(err) => bail!("icrc1_transfer failed: {err:?}"),
    }
}

pub fn build_pic_with_real_icp_and_nns_governance(log_level: Option<Level>) -> PocketIc {
    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        // Required for maturity disbursement finalization: governance needs maturity modulation,
        // which depends on the cycles minting canister being present in the NNS subnet.
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        nns_governance: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    let mut builder = super::pocketic::builder();
    if let Some(level) = log_level {
        builder = builder.with_log_level(level);
    }

    builder
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build()
}
