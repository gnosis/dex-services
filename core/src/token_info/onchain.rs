use anyhow::Result;
use futures::future::{BoxFuture, FutureExt};

use super::{TokenBaseInfo, TokenId, TokenInfoFetching};
use crate::contracts::stablex_contract::StableXContractImpl;

impl TokenInfoFetching for StableXContractImpl {
    fn get_token_info<'a>(&'a self, id: TokenId) -> BoxFuture<'a, Result<TokenBaseInfo>> {
        async move {
            let (address, alias, decimals) = self.get_token_info(id.into()).await?;
            Ok(TokenBaseInfo { address, alias, decimals })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::web3_provider;
    use crate::http::HttpFactory;
    use crate::util::FutureWaitExt;
    use ethcontract::secret::PrivateKey;
    use std::time::Duration;

    fn create_contract() -> impl TokenInfoFetching {
        let http_factory = HttpFactory::default();
        let web3 = web3_provider(
            &http_factory,
            "https://staging-openethereum.mainnet.gnosisdev.com",
            Duration::from_secs(10),
        )
        .expect("Error creating web3");
        StableXContractImpl::new(
            &web3,
            PrivateKey::from_hex_str(
                "0x0102030405060708091011121314151617181920212223242526272829303132",
            )
            .expect("Invalid private key"),
            1,
        )
        .wait()
        .expect("Error creating contract")
    }

    #[test]
    #[ignore]
    fn integration_test_fetch_mkr() {
        let contract = create_contract();
        let info = contract
            .get_token_info(23.into() /*MKR*/)
            .wait()
            .expect("Error fetching token info");
        assert_eq!(&info.alias, "MKR");
        assert_eq!(info.decimals, 18);
    }

    #[test]
    #[ignore]
    fn integration_test_all_ids() {
        let contract = create_contract();
        let all_ids = contract.all_ids().wait().expect("Error fetching all ids");
        // On 2020/07/02 68 tokens were listed
        assert!(all_ids.len() >= 68);
    }
}
