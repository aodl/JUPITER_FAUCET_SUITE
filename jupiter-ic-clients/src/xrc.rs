use candid::{CandidType, Principal};
use serde::Deserialize;

use crate::ClientError;

pub const XRC_CANISTER_ID: &str = "uf6dk-hyaaa-aaaaq-qaaaq-cai";
const XRC_CALL_CYCLES: u128 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IcpXdrRate {
    pub rate: u64,
    pub decimals: u32,
    pub timestamp: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum AssetClass {
    Cryptocurrency,
    FiatCurrency,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct Asset {
    pub symbol: String,
    #[serde(rename = "class")]
    pub class_: AssetClass,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct GetExchangeRateRequest {
    pub base_asset: Asset,
    pub quote_asset: Asset,
    pub timestamp: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ExchangeRateMetadata {
    pub decimals: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
struct ExchangeRate {
    pub rate: u64,
    pub metadata: ExchangeRateMetadata,
    pub timestamp: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum ExchangeRateError {
    AnonymousPrincipalNotAllowed,
    CryptoBaseAssetNotFound,
    CryptoQuoteAssetNotFound,
    StablecoinRateTooFewRates,
    StablecoinRateZeroRate,
    ForexAssetsNotFound,
    ForexBaseAssetNotFound,
    ForexQuoteAssetNotFound,
    ForexInvalidTimestamp,
    RateLimited,
    NotEnoughCycles,
    NotEnoughStablecoinRates,
    StablecoinRateNotFound,
    InconsistentRatesReceived,
    FailedToAcceptCycles,
    Pending,
    Other { code: u32, description: String },
}

#[derive(Clone, Debug, CandidType, Deserialize)]
enum GetExchangeRateResult {
    Ok(ExchangeRate),
    Err(ExchangeRateError),
}

pub struct XrcCanister {
    canister_id: Principal,
}

impl XrcCanister {
    pub fn new() -> Self {
        Self::with_canister_id(mainnet_xrc_canister_id())
    }

    pub fn with_canister_id(canister_id: Principal) -> Self {
        Self { canister_id }
    }

    pub async fn get_icp_xdr_rate(&self) -> Result<IcpXdrRate, ClientError> {
        let decoded: GetExchangeRateResult =
            ic_cdk::call::Call::unbounded_wait(self.canister_id, "get_exchange_rate")
                .with_arg(icp_xdr_request())
                .with_cycles(XRC_CALL_CYCLES)
                .await
                .map_err(|err| {
                    ClientError::Call(format!("XRC get_exchange_rate rejected: {err:?}"))
                })?
                .candid()
                .map_err(|err| {
                    ClientError::Call(format!("XRC get_exchange_rate decode failed: {err}"))
                })?;

        match decoded {
            GetExchangeRateResult::Ok(rate) => Ok(IcpXdrRate {
                rate: rate.rate,
                decimals: rate.metadata.decimals,
                timestamp: rate.timestamp,
            }),
            GetExchangeRateResult::Err(err) => Err(ClientError::Call(format!(
                "XRC get_exchange_rate returned error: {err:?}"
            ))),
        }
    }
}

impl Default for XrcCanister {
    fn default() -> Self {
        Self::new()
    }
}

pub fn mainnet_xrc_canister_id() -> Principal {
    Principal::from_text(XRC_CANISTER_ID).expect("invalid hardcoded XRC principal")
}

fn icp_xdr_request() -> GetExchangeRateRequest {
    GetExchangeRateRequest {
        base_asset: Asset {
            symbol: "ICP".to_string(),
            class_: AssetClass::Cryptocurrency,
        },
        quote_asset: Asset {
            symbol: "XDR".to_string(),
            class_: AssetClass::FiatCurrency,
        },
        timestamp: None,
    }
}
