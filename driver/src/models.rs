#[macro_use]
extern crate serde_derive;

extern crate serde_json;
extern crate serde;
extern crate rustc_hex;
extern crate tiny_keccak;
extern crate byteorder;

use rustc_hex::{ToHex};
use byteorder::{LittleEndian, WriteBytesExt};
use tiny_keccak::Keccak;

pub const ACCOUNTS: i32 = 2;
pub const TOKENS: i32 = 2;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Clone)]
pub struct State {
  	pub curState: String,
   	pub slot: i32,
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
          println!("Intermediate Hash:{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}", bs[0],bs[1],bs[2],bs[3], bs[4],bs[5],bs[6],bs[7]);  
          let mut h = Keccak::new_keccak256();
          h.update(&bs);
          let mut res: [u8; 32] = [0; 32];
          h.finalize(&mut res);
          hash = res.clone();
        }
        hash.to_hex()
    } 
}

#[allow(non_snake_case)]
#[derive(Serialize, Deserialize)]
pub struct Deposits {
  	pub depositHash: String,
    pub slotIndex: i32,
    pub slot: i32,
    pub accountId: i32,
    pub tokenId: i32,
    pub amount: i64,
}

impl Deposits {

  /*//All these hash functions still need to be coded
  pub fn calc_hash(&self, prev_hash: [u8; 32]) -> [u8; 32]{


    // rust deposit hash calculation:
    //    '0x136dd1a7d0a62859f2077a62b7673c5c712fb750604a15f5f6140ab2c5112327'
    /// depositHashWithOnlyBytes32 0x66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925'
    // does not matter whether I am hashing bytes32 or uint256, as log as they are the same numbers
    //   '0x2b32db6c2c0a6235fb1397e8225ea85e0f0e6e8c7b126d0016ccbde0e667151e'

    // try also:
    //        let deserialized1: H128 = serde_json::from_str(r#""0x00000000000000000000000a00010f00""#).unwrap();


    let mut hash: [u8; 32] = prev_hash;  
    let mut bs: [u8; 64] = [0; 64];
    bs.as_mut();
    //    .write_i64::<LittleEndian>(*i)
    //    .expect("Unable to write");
    //let mut hex = "0000000000000000000000000000000000000000000000000000000000000000";
    //let bs = &mut hex.from_hex().collect();
    //let bs = hex::decode("000000000000000");
    let s =String::from("0000000000000000000000000000000000000000000000000000000000000000");
    //               0000000000000000000000000000000000000000000000000000000000000000
    let bs = s.as_bytes();
    //let bs = s.from_hex().unwrap().collect();

    //println!("{:?}", bs);        
    let mut h = Keccak::new_keccak256();
    h.update(&bs);
    let mut res: [u8; 32] = [0; 32];
    h.finalize(&mut res);
    hash = res.clone();

    println!("{:?}", hash[0]);    
    hash.clone()   
  }
*/
}