pub mod state;
pub mod flux;
pub mod order;
pub mod standing_order;

pub use crate::models::state::State;
pub use crate::models::flux::PendingFlux;
pub use crate::models::order::Order;
pub use crate::models::standing_order::StandingOrder;

use sha2::{Digest, Sha256};
use web3::types::H256;

//ToDo: get variables from database
pub const TOKENS: u8 = 30;
pub const NUM_RESERVED_ACCOUNTS: u8 = 50;
pub const DB_NAME: &str = "dfusion2";

pub trait RollingHashable {
    fn rolling_hash(&self, nonce: i32) -> H256;
}

pub trait DataHashable {
    fn data_hash(&self, init_hash: H256) -> H256;
}

pub trait RootHashable {
    fn root_hash(&self, valid_items: &[bool]) -> H256;
}

pub trait Serializable {
    fn bytes(&self) -> Vec<u8>;
}

fn merkleize(leafs: Vec<Vec<u8>>) -> H256 {
    if leafs.len() == 1 {
        return H256::from(leafs[0].as_slice());
    }
    let next_layer = leafs.chunks(2).map(|pair| {
        let mut hasher = Sha256::new();
        hasher.input(&pair[0]);
        hasher.input(&pair[1]);
        hasher.result().to_vec()
    }).collect();
    merkleize(next_layer)
}

fn iter_hash<T: Serializable>(item: &T, prev_hash: &H256) -> H256 {
    let mut hasher = Sha256::new();
    hasher.input(prev_hash);
    hasher.input(item.bytes());
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    H256::from(b.as_slice())
}
