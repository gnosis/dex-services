use crate::models::TOKENS;
use super::util::PopFromLogData;

#[derive(Clone, Debug)]
pub struct AuctionResults {
    pub prices: Vec<u128>,
    pub buy_amounts: Vec<u128>,
    pub sell_amounts: Vec<u128>,
}  //TODO - Use Solution from driver/src/price_finding/price_finder_interface.rs


impl From<Vec<u8>> for AuctionResults {
    fn from(mut solution_data: Vec<u8>) -> Self {
        let mut prices: Vec<u128> = vec![];
        for _i in 0..TOKENS {
            prices.push(u128::pop_from_log_data(&mut solution_data))
        }
        println!("Found prices {:?}", prices);

        let mut buy_amounts: Vec<u128> = vec![];
        let mut sell_amounts: Vec<u128> = vec![];
        while !solution_data.is_empty() {
            buy_amounts.push(u128::pop_from_log_data(&mut solution_data));
            sell_amounts.push(u128::pop_from_log_data(&mut solution_data));
        }

        AuctionResults {
            prices,
            buy_amounts,
            sell_amounts,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;

    #[test]
    fn test_from_vec() {
        let mut bytes: Vec<Vec<u8>> = vec![];
        let mut expected_prices: Vec<u128> = vec![];
        // Load token prices.
        for i in 0..TOKENS {
            bytes.push(vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, i]);
            expected_prices.push(i as u128);
        }


        bytes.push(
            /* buy_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3]
        );
        bytes.push(
            /* sell_amount_1 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]
        );
        bytes.push(
            /* buy_amount_2 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0]
        );
        bytes.push(
            /* sell_amount_2 */ vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255],
        );

        let test_data: Vec<u8> = bytes.iter().flat_map(|i| i.iter()).cloned().collect();

        let res = AuctionResults::from(test_data);

        let expected_buy_amounts: Vec<u128> = vec![3, 4311810048];
        let expected_sell_amounts: Vec<u128> = vec![2, 340282366920938463463374607431768211455];

        assert_eq!(res.prices, expected_prices);
        assert_eq!(res.buy_amounts, expected_buy_amounts);
        assert_eq!(res.sell_amounts, expected_sell_amounts);
    }
}