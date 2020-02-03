use ethcontract::web3::types::{H160, H256, U256};

/// This is the AuctionBid as received from the EVM via smart contract query
/// the slices are meant to be parsed as H256 (all other values require no alteration).
type AuctionBidData = (
    [u8; 32], // order_hash
    u64,      // num_orders
    U256,     // creation_timestamp
    H160,     // solver (address)
    U256,     // objective_value
    [u8; 32], // tentative_state
    [u8; 32], // solution_hash
    U256,     // _auction_applied_time (always zero at time of bid)
    U256,     // _applied_account_state_index (always 0 at time of bid)
);

#[derive(Debug)]
pub struct AuctionBid {
    data: AuctionBidData,
}

impl From<AuctionBidData> for AuctionBid {
    fn from(data: AuctionBidData) -> Self {
        AuctionBid { data }
    }
}

impl AuctionBid {
    pub fn tentative_state(&self) -> H256 {
        H256::from_slice(&self.data.5)
    }
}
