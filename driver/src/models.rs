#[macro_use]
extern crate serde_derive;
extern crate sha2;
extern crate serde_json;
extern crate serde;
extern crate rustc_hex;
extern crate byteorder;

use byteorder::{LittleEndian, WriteBytesExt};
use web3::types:: {H256};
use rustc_hex::{FromHex, ToHex};
use sha2::{Sha256, Sha512, Digest};

pub const ACCOUNTS: i32 = 100;
pub const BITS_PER_ACCOUNT: i32 = 4;
pub const TOKENS: i32 = 30;
pub const BITS_PER_TOKENS: i32 = 3;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;
pub const BITS_PER_BALANCE: i32 = 30;

pub const DB_NAME: &str = "dfusion2";

pub fn from_slice2(bytes: &[u8]) -> [u8; 32] {
      let mut array = [0; 32];
      let bytes = &bytes[..array.len()]; // panics if not enough data
      array.copy_from_slice(bytes); 
      array
    }  

fn parse_hex(hex_asm: &str) -> Vec<u8> {
    let mut hex_bytes = hex_asm.as_bytes().iter().filter_map(|b| {
        match b {
            b'0'...b'9' => Some(b - b'0'),
            b'a'...b'f' => Some(b - b'a' + 10),
            b'A'...b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }).fuse();

    let mut bytes = Vec::new();
    while let (Some(h), Some(l)) = (hex_bytes.next(), hex_bytes.next()) {
        bytes.push(h << 4 | l)
    }
    bytes
}    

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone)]
pub struct State {
  	pub stateHash: String,
   	pub stateIndex: i32,
   	pub balances: Vec<i64>,
}

impl State {
    //Todo: Exchange sha with pederson hash
    pub fn hash(&self) -> String {

        let mut hash: [u8; 32] = [0; 32];
        for i in &self.balances {
         
          let mut bs = [0u8; 64];
          bs.as_mut()
            .write_i64::<LittleEndian>(*i)
            .expect("Unable to write");
          for i in 0..32 {
            bs[i+32] = bs[i];
            bs[i] = hash[i];
          }  
          let bytes: Vec< u8> = bs.to_vec();
          let mut hasher = Sha256::new();
          hasher.input(&bytes);
          let result = hasher.result();
          let b: Vec<u8>  = result.to_vec();
          hash = from_slice2(&b); 
        }
      hash.to_hex()
    } 
    
    
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize)]
pub struct Deposits {
    pub slotIndex: i32,
    pub slot: i32,
    pub accountId: i32,
    pub tokenId: i32,
    pub amount: i64,
}

impl Deposits {

  //All these hash functions still need to be coded
  pub fn hash_zero(&self, prev_hash: &H256
    ) -> H256 {
         
          let _current_deposithash: H256 = H256::zero();
          let s = _current_deposithash.hex();
          let bytes: Vec< u8> = s[2..].from_hex().unwrap();
          let mut hasher = Sha256::new();
          hasher.input(&bytes);
          let result = hasher.result();
          let b: Vec<u8>  = result.to_vec();
          let hash: H256 = H256::from_slice(&b);
          hash
  }

  //All these hash functions still need to be coded
  pub fn hash_zero_512(&self, prev_hash: &H256
    ) -> H256 {
         
          let _current_deposithash: H256 = H256::zero();
          let s = _current_deposithash.hex();
          let mut bytes: Vec< u8> = s[2..].from_hex().unwrap();
          let mut bytes2: Vec< u8> = s[2..].from_hex().unwrap();

          bytes.append(&mut bytes2);
          println!("{:?}", bytes);
          let mut hasher = Sha256::new();
          hasher.input(&bytes);
          let result = hasher.result();
          let b: Vec<u8>  = result.to_vec();
          let hash: H256 = H256::from_slice(&b);
          hash
  }

  pub fn iter_hash(&self, prev_hash: &H256) -> H256 {    
        let _current_deposithash: H256 = H256::zero();
        let s = prev_hash.hex();
        let mut bytes: Vec< u8> = s[2..].from_hex().unwrap();

        // add two byte for uint16 accountID
        let s = format!("{:X}", self.accountId);
        let mut temp:Vec<u8> = parse_hex(&s);
        bytes.append(&mut temp);
        // add one byte for uint8 tokenIndex,
        let s = format!("{:X}", self.tokenId);
        let mut temp:Vec<u8> = parse_hex(&s);
        bytes.append(&mut temp);
        // add 32 byte for amount u256
        let s = format!("{:X}", self.amount);
        let mut temp:Vec<u8> = parse_hex(&s);
        bytes.append(&mut temp);

        println!("{:?}", bytes);
        let mut hasher = Sha256::new();
        hasher.input(&bytes);
        let result = hasher.result();
        let b: Vec<u8>  = result.to_vec();
        let hash: H256 = H256::from_slice(&b);
        hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_hash_zero() {
        //check transformations
        let deposits = Deposits { slotIndex: 0, slot: 0, accountId: 0, tokenId: 0, amount: 0 };
        let current_deposithash: H256 = H256::zero();

        let s = current_deposithash.hex();
        let bytes: Vec< u8> = s[2..].from_hex().unwrap();
        println!("{:?}", bytes);
        let hash: H256 = H256::from_slice(&bytes);

         assert_eq!(current_deposithash, hash);

        //Check actual hashing 
        let target: H256 = serde_json::from_str(r#""0x66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925""#).unwrap();
        assert_eq!(deposits.hash_zero(&current_deposithash), target);
    }

    #[test]
    fn check_hash_zero_512bits() {
        //check transformations
        let deposits = Deposits { slotIndex: 0, slot: 0, accountId: 0, tokenId: 0, amount: 0 };
        let current_deposithash: H256 = H256::zero();

        let s = current_deposithash.hex();
        let bytes: Vec< u8> = s[2..].from_hex().unwrap();
        println!("{:?}", bytes);
        let hash: H256 = H256::from_slice(&bytes);

         assert_eq!(current_deposithash, hash);

        //Check actual hashing 
        let target: H256 = serde_json::from_str(r#""0xf5a5fd42d16a20302798ef6ed309979b43003d2320d9f0e8ea9831a92759fb4b""#).unwrap();
        assert_eq!(deposits.hash_zero_512(&current_deposithash), target);
    }

    #[test]
    fn check_iter_hash() {
        //check transformations
        let deposits = Deposits { slotIndex: 0, slot: 0, accountId: 0, tokenId: 0, amount: 0 };
        let current_deposithash: H256 = H256::zero();

        let s = current_deposithash.hex();
        let bytes: Vec< u8> = s[2..].from_hex().unwrap();
        println!("{:?}", bytes);
        let hash: H256 = H256::from_slice(&bytes);

         assert_eq!(current_deposithash, hash);

        //Check actual hashing 
        let target: H256 = serde_json::from_str(r#""0x1be2b3990b410ca4fb38d1f79019c4018cd8820b69618646c81d22dfcbddc802""#).unwrap();
        assert_eq!(deposits.hash_zero_512(&current_deposithash), target);
    }
}