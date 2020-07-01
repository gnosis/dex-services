use anyhow::Result;
use futures::future::{BoxFuture, FutureExt};

use super::{TokenBaseInfo, TokenId, TokenInfoFetching};
use crate::contracts::stablex_contract::StableXContractImpl;

impl TokenInfoFetching for StableXContractImpl {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        async move {
            let info = self.get_token_info(id.into()).await?;
            Ok(TokenBaseInfo {
                alias: info.1,
                decimals: info.2,
            })
        }
        .boxed()
    }

    fn all_ids<'a>(&'a self) -> BoxFuture<'a, Result<Vec<TokenId>>> {
        async move {
            let num_tokens = self.num_tokens().await?;
            let ids: Vec<TokenId> = (0..num_tokens).map(|token| token.into()).collect();
            Ok(ids)
        }
        .boxed()
    }
}
