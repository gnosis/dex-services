pub const ACCOUNTS: i32 = 8;
pub const TOKENS: i32 = 8;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;

use pairing::{ PrimeField };

#[derive(Serialize, Deserialize)]
pub struct State {
  	pub curState: String,
   	pub prevState: String,
  	pub nextState: String,
   	pub slot: i32,
   	pub balances: [ i32; SIZE_BALANCE],
}