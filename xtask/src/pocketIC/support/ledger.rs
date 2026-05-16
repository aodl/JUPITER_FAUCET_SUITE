use pocket_ic::common::rest::{IcpFeatures, IcpFeaturesConfig};
use pocket_ic::{PocketIc, PocketIcBuilder};

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
