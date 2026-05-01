use candid::{CandidType, Nat};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

const REQUIRED_ATTACHED_CYCLES: u128 = 1_000_000_000;
const ACCEPTED_CYCLES: u128 = 260_000_000;

#[derive(Clone, Debug, CandidType, Deserialize, PartialEq, Eq)]
pub enum AssetClass {
    Cryptocurrency,
    FiatCurrency,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct Asset {
    pub symbol: String,
    #[serde(rename = "class")]
    pub class_: AssetClass,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct GetExchangeRateRequest {
    pub base_asset: Asset,
    pub quote_asset: Asset,
    pub timestamp: Option<u64>,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ExchangeRateMetadata {
    pub decimals: u32,
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct ExchangeRate {
    pub rate: u64,
    pub metadata: ExchangeRateMetadata,
    pub timestamp: u64,
}

#[derive(Clone, Debug, CandidType, Deserialize, Serialize)]
pub enum ExchangeRateError {
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
pub enum GetExchangeRateResult {
    Ok(ExchangeRate),
    Err(ExchangeRateError),
}

#[derive(Clone, Debug, CandidType, Deserialize)]
pub struct DebugExchangeRateCall {
    pub base_symbol: String,
    pub quote_symbol: String,
    pub requested_timestamp: Option<u64>,
    pub attached_cycles: Nat,
    pub accepted_cycles: Nat,
}

#[derive(Default)]
struct State {
    rate: u64,
    decimals: u32,
    timestamp: u64,
    error: Option<ExchangeRateError>,
    calls: Vec<DebugExchangeRateCall>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State {
        rate: 100_000,
        decimals: 4,
        timestamp: 1_700_000_000,
        error: None,
        calls: Vec::new(),
    });
}

#[ic_cdk::init]
fn init() {}

#[ic_cdk::update]
fn get_exchange_rate(req: GetExchangeRateRequest) -> GetExchangeRateResult {
    let attached_cycles = ic_cdk::api::msg_cycles_available();
    if attached_cycles < REQUIRED_ATTACHED_CYCLES {
        ST.with(|s| {
            s.borrow_mut().calls.push(DebugExchangeRateCall {
                base_symbol: req.base_asset.symbol.clone(),
                quote_symbol: req.quote_asset.symbol.clone(),
                requested_timestamp: req.timestamp,
                attached_cycles: Nat::from(attached_cycles),
                accepted_cycles: Nat::from(0_u8),
            });
        });
        return GetExchangeRateResult::Err(ExchangeRateError::NotEnoughCycles);
    }

    let accepted_cycles = ic_cdk::api::msg_cycles_accept(ACCEPTED_CYCLES);
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.calls.push(DebugExchangeRateCall {
            base_symbol: req.base_asset.symbol.clone(),
            quote_symbol: req.quote_asset.symbol.clone(),
            requested_timestamp: req.timestamp,
            attached_cycles: Nat::from(attached_cycles),
            accepted_cycles: Nat::from(accepted_cycles),
        });

        if let Some(error) = st.error.clone() {
            return GetExchangeRateResult::Err(error);
        }
        if req.base_asset.symbol != "ICP" || req.base_asset.class_ != AssetClass::Cryptocurrency {
            return GetExchangeRateResult::Err(ExchangeRateError::CryptoBaseAssetNotFound);
        }
        if req.quote_asset.symbol != "XDR" || req.quote_asset.class_ != AssetClass::FiatCurrency {
            return GetExchangeRateResult::Err(ExchangeRateError::ForexQuoteAssetNotFound);
        }
        GetExchangeRateResult::Ok(ExchangeRate {
            rate: st.rate,
            metadata: ExchangeRateMetadata { decimals: st.decimals },
            timestamp: req.timestamp.unwrap_or(st.timestamp),
        })
    })
}

#[ic_cdk::update]
fn debug_reset() {
    ST.with(|s| {
        *s.borrow_mut() = State {
            rate: 100_000,
            decimals: 4,
            timestamp: 1_700_000_000,
            error: None,
            calls: Vec::new(),
        };
    });
}

#[ic_cdk::update]
fn debug_set_rate(rate: u64, decimals: u32, timestamp: u64) {
    ST.with(|s| {
        let mut st = s.borrow_mut();
        st.rate = rate;
        st.decimals = decimals;
        st.timestamp = timestamp;
        st.error = None;
    });
}

#[ic_cdk::update]
fn debug_set_error(error: Option<ExchangeRateError>) {
    ST.with(|s| s.borrow_mut().error = error);
}

#[ic_cdk::query]
fn debug_get_calls() -> Vec<DebugExchangeRateCall> {
    ST.with(|s| s.borrow().calls.clone())
}

ic_cdk::export_candid!();
