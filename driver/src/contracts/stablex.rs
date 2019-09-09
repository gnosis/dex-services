//#[cfg(test)]
//extern crate mock_it;
//
//use web3::types::{H256, U128, U256};
//
//use crate::error::DriverError;
//
//use super::base_contract::SmartContract;
//
//type Result<T> = std::result::Result<T, DriverError>;
//
//struct StableXContractImpl {
//    contract: SmartContract
//}
//
//impl StableXContractImpl {
//    pub fn new(contract: SmartContract) -> Self {
//        // Should we assert that the contract is indeed a StableX contract?
//        StableXContractImpl {
//            contract
//        }
//    }
//}
//
//pub trait StableXContract {
//    fn get_current_auction_index(&self) -> Result<U256>;
//    // TODO - auction_data should parse and return relevant orders and account balances.
//    fn get_auction_data(&self, _index: u32) -> Result<H256>;
//
//    fn submit_solution(
//        &self,
//        _batch_index: u32,
//        _owners: Vec<H256>,
//        _order_ids: Vec<u16>,
//        _volumes: Vec<u128>,
//        _prices: Vec<U128>,
//        _token_ids_for_price: Vec<u16>,
//    ) -> Result<()>;
//}
//
//impl StableXContract for StableXContractImpl {
//    fn get_current_auction_index(&self) -> Result<U256> {
//        unimplemented!();
//    }
//
//    fn get_auction_data(&self, _index: u32) -> Result<H256> {
//        unimplemented!();
//    }
//
//    fn submit_solution(
//        &self,
//        _batch_index: u32,
//        _owners: Vec<H256>,
//        _order_ids: Vec<u16>,
//        _volumes: Vec<u128>,
//        _prices: Vec<U128>,
//        _token_ids_for_price: Vec<u16>,
//    ) -> Result<()> {
//        unimplemented!();
//    }
//}