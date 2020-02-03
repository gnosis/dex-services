use ethcontract::web3::types::{H160, H256, U256};

type AuctionBidData = (
    [u8; 32],
    u64,
    U256,
    H160,
    U256,
    [u8; 32],
    [u8; 32],
    U256,
    U256,
);

#[derive(Debug)]
pub struct AuctionBid {
    data: AuctionBidData,
}

impl AuctionBid {
    pub fn from(data: AuctionBidData) -> Self {
        AuctionBid { data }
    }
}

impl AuctionBid {
    //    pub fn order_hash(&self) -> H256 {
    //        H256::from_slice(&self.data.0)
    //    }
    //    pub fn num_orders(&self) -> u64 {
    //        self.data.1
    //    }
    //    pub fn creation_time_stamp(&self) -> U256 {
    //        self.data.2
    //    }
    //    pub fn solver(&self) -> H160 {
    //        self.data.3
    //    }
    //    pub fn objective_value(&self) -> U256 {
    //        self.data.4
    //    }
    pub fn tentative_state(&self) -> H256 {
        H256::from_slice(&self.data.5)
    }
    //    pub fn solution_hash(&self) -> H256 {
    //        H256::from_slice(&self.data.6)
    //    }
}
