pub const ACCOUNTS: i32 = 2;
pub const TOKENS: i32 = 2;
pub const SIZE_BALANCE: usize = (ACCOUNTS * TOKENS) as usize;

#[derive(Serialize, Deserialize, Clone)]
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
    pub depositIndex: String,
    pub slot: i32,
    pub addressId: i32,
    pub tokenId: i32,
    pub amount: i64,
}