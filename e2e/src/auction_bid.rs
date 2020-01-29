use ethcontract::web3::types::{H160, H256, U256};

#[derive(Debug)]
pub struct AuctionBid {
    order_hash: H256,
    num_orders: u64,
    creation_time_stamp: U256,
    pub solver: H160,
    objective_value: U256,
    pub tentative_state: H256,
    solution_hash: H256,
}

type PreAuctionBid = (
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

impl AuctionBid {
    pub fn from(contract_bid: &PreAuctionBid) -> Self {
        let order_hash = H256::from_slice(&contract_bid.0);
        let num_orders = contract_bid.1;
        let creation_time_stamp = contract_bid.2;
        let solver = contract_bid.3;
        let objective_value = contract_bid.4;
        let tentative_state = H256::from_slice(&contract_bid.5);
        let solution_hash = H256::from_slice(&contract_bid.6);
        let _auction_applied_time = contract_bid.7;
        let _applied_account_state_index = contract_bid.8;
        AuctionBid {
            order_hash,
            num_orders,
            creation_time_stamp,
            solver,
            objective_value,
            tentative_state,
            solution_hash,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_auction_bid_new() {
        let bid_data = (
            [3u8; 32],
            6u64,
            U256::from(12345),
            H160::from_str("90f8bf6a479f320ead074411a4b0e7944ea8c9c1").unwrap(),
            U256::from(0),
            [1u8; 32],
            [2u8; 32],
            U256::from(1),
            U256::from(2),
        );

        let bid = AuctionBid::from(&bid_data);

        // The following hashes are the only fields altered by the struct's construction
        assert_eq!(
            bid.order_hash,
            H256::from_str("0303030303030303030303030303030303030303030303030303030303030303")
                .unwrap()
        );
        assert_eq!(
            bid.order_hash,
            H256::from_str("0101010101010101010101010101010101010101010101010101010101010101")
                .unwrap()
        );
        assert_eq!(
            bid.order_hash,
            H256::from_str("0202020202020202020202020202020202020202020202020202020202020202")
                .unwrap()
        );
    }
}
