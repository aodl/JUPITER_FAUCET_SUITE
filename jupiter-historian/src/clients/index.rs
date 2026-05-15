use async_trait::async_trait;
#[allow(unused_imports)]
pub use jupiter_ic_clients::account_identifier::{
    account_identifier_text, account_identifier_text_for_account,
};
#[allow(unused_imports)]
pub use jupiter_ic_clients::index::{
    GetAccountIdentifierTransactionsArgs, GetAccountIdentifierTransactionsError,
    GetAccountIdentifierTransactionsResponse, GetAccountIdentifierTransactionsResult,
    IcpIndexCanister, IndexOperation, IndexTimeStamp, IndexTransaction, IndexTransactionWithId,
    Tokens,
};

use crate::clients::{ClientError, IndexClient};

#[async_trait]
impl IndexClient for IcpIndexCanister {
    async fn get_account_identifier_transactions(
        &self,
        account_identifier: String,
        start: Option<u64>,
        max_results: u64,
    ) -> Result<GetAccountIdentifierTransactionsResponse, ClientError> {
        IcpIndexCanister::get_account_identifier_transactions(self, account_identifier, start, max_results)
            .await
            .map_err(Into::into)
    }
}
