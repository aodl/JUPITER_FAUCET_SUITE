use super::*;
pub(crate) fn original_blackhole_id() -> Principal {
    Principal::from_text("e3mmv-5qaaa-aaaah-aadma-cai")
        .expect("invalid hardcoded original blackhole principal")
}

pub(crate) fn secure_mainnet_blackhole_id() -> Principal {
    Principal::from_text("77deu-baaaa-aaaar-qb6za-cai")
        .expect("invalid hardcoded secure mainnet blackhole principal")
}

pub(crate) fn should_try_secure_blackhole_first(configured_blackhole_id: Principal) -> bool {
    configured_blackhole_id == secure_mainnet_blackhole_id()
}

pub(crate) struct FallbackBlackholeClient<'a, P: BlackholeClient, F: BlackholeClient> {
    primary: &'a P,
    fallback: &'a F,
}

impl<'a, P: BlackholeClient, F: BlackholeClient> FallbackBlackholeClient<'a, P, F> {
    pub(crate) fn new(primary: &'a P, fallback: &'a F) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl<P: BlackholeClient, F: BlackholeClient> BlackholeClient for FallbackBlackholeClient<'_, P, F> {
    async fn canister_status(
        &self,
        canister_id: Principal,
    ) -> Result<crate::clients::blackhole::BlackholeCanisterStatus, ClientError> {
        match self.primary.canister_status(canister_id).await {
            Ok(status) => Ok(status),
            Err(primary_err) => self
                .fallback
                .canister_status(canister_id)
                .await
                .map_err(|fallback_err| {
                    ClientError::Call(format!(
                        "primary blackhole failed: {primary_err}; fallback blackhole failed: {fallback_err}"
                    ))
                }),
        }
    }
}
