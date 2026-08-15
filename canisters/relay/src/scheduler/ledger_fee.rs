use candid::Nat;

use crate::clients::LedgerClient;
use crate::scheduler::logging::log_structured_error;
use crate::state;

/// Bootstrap/emergency contingency used only when neither a live ledger fee nor a heap-cached
/// last-known fee is available. This is not an invariant of the ICP ledger.
pub(super) const ICP_LEDGER_FEE_BOOTSTRAP_FALLBACK_E8S: u64 = 10_000;

#[derive(Clone, Copy)]
pub(super) enum LedgerFeeResolutionContext {
    Splitter,
    SubaccountOne,
    DefaultAccount,
}

impl LedgerFeeResolutionContext {
    fn as_str(self) -> &'static str {
        match self {
            Self::Splitter => "splitter",
            Self::SubaccountOne => "subaccount_1",
            Self::DefaultAccount => "default_account",
        }
    }
}

pub(super) async fn resolve_icp_ledger_fee_e8s<L: LedgerClient>(
    ledger: &L,
    context: LedgerFeeResolutionContext,
) -> u64 {
    match ledger.fee_e8s().await {
        Ok(fee_e8s) => {
            state::with_state_mut(|st| st.last_known_ledger_fee_e8s = Some(fee_e8s));
            fee_e8s
        }
        Err(err) => {
            let cached = state::with_state(|st| st.last_known_ledger_fee_e8s);
            let (fallback_source, fee_e8s) = cached
                .map(|fee_e8s| ("cached", fee_e8s))
                .unwrap_or(("bootstrap", ICP_LEDGER_FEE_BOOTSTRAP_FALLBACK_E8S));
            log_structured_error(
                "ledger_fee_fallback",
                &[
                    ("context", context.as_str().to_string()),
                    ("error", err.to_string()),
                    ("fallback_source", fallback_source.to_string()),
                    ("fee_e8s", fee_e8s.to_string()),
                ],
            );
            fee_e8s
        }
    }
}

pub(super) fn record_bad_fee(context: &'static str, planned_fee_e8s: u64, expected_fee: &Nat) {
    match u64::try_from(expected_fee.0.clone()) {
        Ok(expected_fee_e8s) => {
            state::with_state_mut(|st| {
                st.last_known_ledger_fee_e8s = Some(expected_fee_e8s);
            });
            log_structured_error(
                "ledger_fee_changed",
                &[
                    ("context", context.to_string()),
                    ("planned_fee_e8s", planned_fee_e8s.to_string()),
                    ("expected_fee_e8s", expected_fee_e8s.to_string()),
                ],
            );
        }
        Err(_) => {
            log_structured_error(
                "ledger_fee_changed",
                &[
                    ("context", context.to_string()),
                    ("planned_fee_e8s", planned_fee_e8s.to_string()),
                    ("expected_fee", expected_fee.to_string()),
                    (
                        "conversion_error",
                        "expected_fee_out_of_u64_range".to_string(),
                    ),
                ],
            );
        }
    }
}
