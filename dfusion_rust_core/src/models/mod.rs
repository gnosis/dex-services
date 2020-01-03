pub mod account_state;
pub mod flux;
pub mod order;
pub mod solution;
pub mod standing_order;
pub mod util;

pub use crate::models::account_state::AccountState;
pub use crate::models::flux::PendingFlux;
pub use crate::models::order::BatchInformation;
pub use crate::models::order::Order;
pub use crate::models::solution::Solution;
pub use crate::models::standing_order::StandingOrder;

use sha2::{Digest, Sha256};
use web3::types::H256;

// TODO: get variables from database
pub const NUM_RESERVED_ACCOUNTS: usize = 50;
pub const NUM_SLOTS_REGULAR_ORDERS: u16 = 500;
pub const NUM_SLOTS_PER_STANDING_ORDER_ACCOUNT: u16 = 10;

pub trait RollingHashable {
    fn rolling_hash(&self, nonce: u32) -> H256;
}

pub trait ConcatenatingHashable {
    fn concatenating_hash(&self, init_hash: H256) -> H256;
}

pub trait RootHashable {
    fn root_hash(&self, valid_items: &[bool]) -> H256;
}

pub trait Serializable {
    fn bytes(&self) -> Vec<u8>;
}

pub trait Deserializable {
    // TODO: encode num_tokens in the first byte(s)
    fn from_bytes(bytes: Vec<u8>) -> Self;
}

fn merkleize(leafs: Vec<Vec<u8>>) -> H256 {
    if leafs.len() == 1 {
        return H256::from_slice(leafs[0].as_slice());
    }
    let next_layer = leafs
        .chunks(2)
        .map(|pair| {
            let mut hasher = Sha256::new();
            hasher.input(&pair[0]);
            hasher.input(&pair[1]);
            hasher.result().to_vec()
        })
        .collect();
    merkleize(next_layer)
}

fn iter_hash<T: Serializable>(item: &T, prev_hash: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.input(prev_hash);
    hasher.input(item.bytes());
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    H256::from_slice(b.as_slice())
}
