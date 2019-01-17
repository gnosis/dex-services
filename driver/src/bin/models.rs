pub const ACCOUNTS: i32 = 2;
pub const TOKENS: i32 = 2;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;

#[derive(Serialize, Deserialize)]
pub struct State {
  	pub curState: String,
   	pub prevState: String,
  	pub nextState: String,
   	pub slot: i32,
   	pub balances: Vec<i64>,
}

#[derive(Serialize, Deserialize)]
pub struct Deposits {
  	pub depositHash: String,
    depositIndex: String,
    slot: i32,
    addressId: i32,
    tokenId: i32,
    amount: i32,
}