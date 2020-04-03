#![allow(dead_code)]

mod state;

use ethcontract::Address;

type UserId = Address;
type TokenAddress = Address;
type OrderId = u16;
type TokenId = u16;
type BatchId = u32;

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Deserialize, Serialize)]
    pub struct EventBackup {
        pub event: crate::contracts::stablex_contract::batch_exchange::Event,
        pub block_timestamp: u64,
    }
}
