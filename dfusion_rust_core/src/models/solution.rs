use super::*;

use log::info;

use byteorder::{BigEndian, ByteOrder};
use std::iter::once;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution {
    pub prices: Vec<u128>,
    pub executed_buy_amounts: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
}

impl Solution {
    pub fn trivial(num_orders: usize) -> Self {
        Solution {
            prices: vec![],
            executed_buy_amounts: vec![0; num_orders],
            executed_sell_amounts: vec![0; num_orders],
        }
    }

    /// Returns true if a solution is non-trivial and false if it is the trivial
    /// solution
    pub fn is_non_trivial(&self) -> bool {
        self.executed_sell_amounts.iter().any(|&amt| amt > 0)
    }
}

impl Serializable for Solution {
    fn bytes(&self) -> Vec<u8> {
        let mut res = (self.prices.len() as u16).to_be_bytes().to_vec();
        let alternating_buy_sell_amounts: Vec<u128> = self
            .executed_buy_amounts
            .iter()
            .zip(self.executed_sell_amounts.iter())
            .flat_map(|tup| once(tup.0).chain(once(tup.1)))
            .cloned()
            .collect();

        let prices_and_volumes: Vec<u8> = [&self.prices, &alternating_buy_sell_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(Serializable::bytes)
            .collect();
        res.extend(prices_and_volumes);
        res
    }
}

impl Deserializable for Solution {
    fn from_bytes(mut bytes: Vec<u8>) -> Self {
        // First 2 bytes encode the length of price vector (i.e. num_tokens)
        let len_prices = BigEndian::read_u16(&bytes[0..2]);
        let volumes = bytes.split_off(2 + len_prices as usize * 12);
        let prices = bytes
            .split_off(2)
            .chunks_exact(12)
            .map(|chunk| util::read_amount(&util::get_amount_from_slice(chunk)))
            .collect();
        info!("Parsed prices as: {:?}", prices);

        let mut executed_buy_amounts: Vec<u128> = vec![];
        let mut executed_sell_amounts: Vec<u128> = vec![];
        volumes.chunks_exact(2 * 12).for_each(|chunk| {
            executed_buy_amounts.push(util::read_amount(&util::get_amount_from_slice(
                &chunk[0..12],
            )));
            executed_sell_amounts.push(util::read_amount(&util::get_amount_from_slice(
                &chunk[12..24],
            )));
        });
        Solution {
            prices,
            executed_buy_amounts,
            executed_sell_amounts,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;

    const NUM_TOKENS: usize = 30;

    #[test]
    fn test_is_non_trivial() {
        let trivial = Solution::trivial(3);
        assert!(!trivial.is_non_trivial());

        let non_trivial = Solution {
            prices: vec![42; 10],
            executed_buy_amounts: vec![4, 5, 6],
            executed_sell_amounts: vec![1, 2, 3],
        };
        assert!(non_trivial.is_non_trivial());
    }

    #[test]
    fn test_to_bytes() {
        let solution = Solution {
            prices: vec![5, 2],
            executed_buy_amounts: vec![2u128.pow(8) + 1],
            executed_sell_amounts: vec![2u128.pow(16) + 2],
        };

        let bytes = solution.bytes();

        assert_eq!(
            bytes,
            vec![
                0, 2, // len(prices)
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, // price0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // price1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // buyAmount0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, // sellAmount0
            ]
        );
    }
    #[test]
    fn test_serialize_deserialize() {
        let solution = Solution {
            prices: vec![42; NUM_TOKENS],
            executed_buy_amounts: vec![4, 5, 6],
            executed_sell_amounts: vec![1, 2, 3],
        };

        let bytes = solution.bytes();
        let parsed_solution = Solution::from_bytes(bytes);

        assert_eq!(solution, parsed_solution);
    }

    #[test]
    fn test_deserialize_e2e_example() {
        let bytes = vec![
            0, 5, // num_tokens
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // price0
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, // price1
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 0, // price2
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, // price3
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // price4
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 1, // buyAmount0
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 2, // sellAmount0
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 3, // buyAmount1
            0, 0, 0, 0, 13, 224, 182, 179, 167, 100, 0, 4, // sellAmount1
        ];
        let parsed_solution = Solution::from_bytes(bytes);
        let expected = Solution {
            prices: vec![1, 10u128.pow(18), 10u128.pow(18), 256, 257],
            executed_buy_amounts: vec![10u128.pow(18) + 1, 10u128.pow(18) + 3],
            executed_sell_amounts: vec![10u128.pow(18) + 2, 10u128.pow(18) + 4],
        };
        assert_eq!(parsed_solution, expected);
    }
}
