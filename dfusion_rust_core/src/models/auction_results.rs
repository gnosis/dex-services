#[derive(Clone, Debug)]
pub struct AuctionResults {
    pub prices: Vec<u128>,
    pub buy_amounts: Vec<u128>,
    pub sell_amounts: Vec<u128>,
}  //TODO - Use Solution from driver/src/price_finding/price_finder_interface.rs


impl From<Vec<u8>> for AuctionResults {
    fn from(solution_data: Vec<u8>) -> Self {
        println!("{:?}", solution_data[0]);
        // TODO - parse solution_data!
        AuctionResults {
            prices: vec![0 as u128; 1],
            buy_amounts: vec![0 as u128; 1],
            sell_amounts: vec![0 as u128; 1],
        }
    }
}