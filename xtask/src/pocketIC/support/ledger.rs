use pocket_ic::common::rest::{IcpFeatures, IcpFeaturesConfig};
use pocket_ic::{PocketIc, PocketIcBuilder};
use slog::Level;

pub const ICP_LEDGER_FEE_E8S: u64 = 10_000;

pub fn build_pic_with_real_icp() -> PocketIc {
    let icp_features = IcpFeatures {
        registry: Some(IcpFeaturesConfig::DefaultConfig),
        cycles_minting: Some(IcpFeaturesConfig::DefaultConfig),
        icp_token: Some(IcpFeaturesConfig::DefaultConfig),
        ..Default::default()
    };

    PocketIcBuilder::new()
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build()
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

    let mut builder = PocketIcBuilder::new();
    if let Some(level) = log_level {
        builder = builder.with_log_level(level);
    }

    builder
        .with_nns_subnet()
        .with_application_subnet()
        .with_icp_features(icp_features)
        .build()
}
