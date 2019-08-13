use web3::types::U256;


#[derive(Clone, Debug)]
pub struct AuctionResults {
    pub prices: Vec<U256>,
    pub buy_amounts: Vec<U256>,
    pub sell_amounts: Vec<U256>
}

impl From<Vec<u8>> for AuctionResults {
    fn from(solution_data: Vec<u8>) -> Self {
        println!("{:?}", solution_data[0]);
        // TODO - parse solution_data!
        AuctionResults {
            prices: vec![U256::zero();1],
            buy_amounts: vec![U256::zero();1],
            sell_amounts: vec![U256::zero();1]
        }
    }
}