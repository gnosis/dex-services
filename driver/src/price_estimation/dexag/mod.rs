mod api;

use super::{PriceSource, Token};
use crate::models::TokenId;
use anyhow::Result;
use api::{DexagApi, DexagApiImpl};
use std::collections::HashMap;

pub struct DexagClient<Api> {
    api: Api,
}

impl DexagClient<DexagApiImpl> {
    pub fn new() -> Result<Self> {
        let api = DexagApiImpl::new()?;
        Ok(Self::with_api(api))
    }
}

impl<Api> DexagClient<Api>
where
    Api: DexagApi,
{
    pub fn with_api(api: Api) -> Self {
        Self { api }
    }
}

impl<Api> PriceSource for DexagClient<Api>
where
    Api: DexagApi,
{
    fn get_prices(&self, tokens: &[Token]) -> Result<HashMap<TokenId, u128>> {
        unimplemented!();
    }
}
