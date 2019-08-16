use super::*;
use web3::types::U256;

use std::iter::once;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub surplus: Option<U256>,
    pub prices: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
}

impl Solution {
    pub fn trivial() -> Self {
        Solution {
            surplus: Some(U256::zero()),
            prices: vec![0; TOKENS as usize],
            executed_sell_amounts: vec![],
            executed_buy_amounts: vec![],
        }
    }
}

impl Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        let alternating_buy_sell_amounts: Vec<u128> = self.executed_buy_amounts
            .iter()
            .zip(self.executed_sell_amounts.iter())
            .flat_map(|tup| once(tup.0).chain(once(tup.1)))
            .cloned()
            .collect();
        [&self.prices, &alternating_buy_sell_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(Serializable::bytes)
            .collect()
    }
}

impl Deserializable for Solution {
    fn from_bytes(mut bytes: Vec<u8>) -> Self {
        let volumes = bytes.split_off(TOKENS as usize * 12);
        let prices = bytes
            .chunks(12)
            .map(|chunk| {
                util::read_amount(
                    &util::get_amount_from_slice(chunk)
                )
            })
            .collect();

        let mut executed_sell_amounts: Vec<u128> = vec![];
        let mut executed_buy_amounts: Vec<u128> = vec![];
        volumes.chunks(2 * 12)
            .for_each(|chunk| {
                executed_buy_amounts.push(util::read_amount(
                    &util::get_amount_from_slice(&chunk[0..12])
                ));
                executed_sell_amounts.push(util::read_amount(
                    &util::get_amount_from_slice(&chunk[12..24])
                ));
            });
        Solution {
            surplus: None,
            prices,
            executed_sell_amounts,
            executed_buy_amounts,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;

    #[test]
    fn test_serialize_deserialize() {
        let solution = Solution {
            surplus: None,
            prices: vec![42; TOKENS as usize],
            executed_sell_amounts: vec![1, 2, 3],
            executed_buy_amounts: vec![4, 5, 6],
        };

        let bytes = solution.bytes();
        let parsed_solution = Solution::from_bytes(bytes);

        assert_eq!(solution, parsed_solution);
    }
}