#[macro_use]
extern crate serde_derive;
extern crate byteorder;
extern crate hex;
extern crate rustc_hex;
extern crate serde;
extern crate serde_json;
extern crate sha2;

use byteorder::{LittleEndian, WriteBytesExt};
use rustc_hex::{FromHex, ToHex};
use sha2::{Digest, Sha256};
use std::num::ParseIntError;
use web3::types::H256;

pub const ACCOUNTS: i32 = 100;
pub const BITS_PER_ACCOUNT: i32 = 4;
pub const TOKENS: i32 = 30;
pub const BITS_PER_TOKENS: i32 = 3;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;
pub const BITS_PER_BALANCE: i32 = 30;

pub const DB_NAME: &str = "dfusion2";

pub fn decode_hex_uint8(s: &mut str, size: i32) -> Result<Vec<u8>, ParseIntError> {
  // add prefix 0, in case s has not even length
  let mut pretail: &str = "";
  if s.len() % 2 == 1 {
    pretail = "0";
  }
  let p: &'static str = pretail.into();
  let s = format!("{}{}", p, s);

  let v: Result<Vec<u8>, ParseIntError> = (0..s.len())
    .step_by(2)
    .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
    .collect();

  let mut v = v.unwrap();
  let mut vv = Vec::with_capacity(size as usize);
  for _i in 0..size {
    vv.push(0);
  }
  for i in 1..v.len() + 1 {
    vv[size as usize - i] = v.pop().unwrap();
  }
  println!("{:?}", vv.clone());
  Ok(vv.clone())
}

pub fn decode_hex(s: &str) -> Result<Vec<u8>, ParseIntError> {
  (0..s.len())
    .step_by(2)
    .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
    .collect()
}

pub fn from_slice2(bytes: &[u8]) -> [u8; 32] {
  let mut array = [0; 32];
  let bytes = &bytes[..array.len()]; // panics if not enough data
  array.copy_from_slice(bytes);
  array
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone, Debug)]
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
        bs[i + 32] = bs[i];
        bs[i] = hash[i];
      }
      let bytes: Vec<u8> = bs.to_vec();
      let mut hasher = Sha256::new();
      hasher.input(&bytes);
      let result = hasher.result();
      let b: Vec<u8> = result.to_vec();
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
  //calcalutes the iterative hash of deposits
  pub fn iter_hash(&self, prev_hash: &H256) -> H256 {
    let _current_deposithash: H256 = H256::zero();
    let s = prev_hash.hex();
    let mut bytes: Vec<u8> = s[2..].from_hex().unwrap();

    // add two byte for uint16 accountID
    let mut s = format!("{:X}", self.accountId);
    let decoded = decode_hex_uint8(&mut s, 2).expect("Decoding failed");
    let mut temp: Vec<u8> = decoded;
    bytes.append(&mut temp);

    // add one byte for uint8 tokenIndex,
    let mut s = format!("{:x}", self.tokenId);
    let decoded = decode_hex_uint8(&mut s, 1).expect("Decoding failed");
    let mut temp: Vec<u8> = decoded;
    bytes.append(&mut temp);

    // add 32 byte for amount u256
    let mut s = format!("{:X}", self.amount);
    let decoded = decode_hex_uint8(&mut s, 16).expect("Decoding failed");
    let mut temp: Vec<u8> = decoded;
    bytes.append(&mut temp);

    println!("{:?}", bytes);
    println!("Length of bytes is{:?}", bytes.len());

    let mut hasher = Sha256::new();
    hasher.input(&bytes);
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    let hash: H256 = H256::from_slice(&b);
    hash
  }

  //Just a function for a test case
  pub fn hash_zero_512(&self) -> H256 {
    let _current_deposithash: H256 = H256::zero();
    let s = _current_deposithash.hex();
    let mut bytes: Vec<u8> = s[2..].from_hex().unwrap();
    let mut bytes2: Vec<u8> = s[2..].from_hex().unwrap();

    bytes.append(&mut bytes2);
    println!("{:?}", bytes);
    let mut hasher = Sha256::new();
    hasher.input(&bytes);
    let result = hasher.result();
    let b: Vec<u8> = result.to_vec();
    let hash: H256 = H256::from_slice(&b);
    hash
  }
}

impl From<mongodb::ordered::OrderedDocument> for Deposits {
    fn from(document: mongodb::ordered::OrderedDocument) -> Self {
        let json = serde_json::to_string(&document).unwrap();
        serde_json::from_str(&json).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(deposits.hash_zero_512(), target);
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
        let target: H256 = serde_json::from_str(r#""0x8e8fe47e4a33b178bf0433d8050cb0ad7ec323fbdeeab3ecfd857b4ce1805b7a""#).unwrap();
        assert_eq!(deposits.iter_hash(&current_deposithash), target);
    }
}
