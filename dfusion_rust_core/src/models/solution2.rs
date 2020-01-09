use super::*;

use log::info;

use byteorder::{BigEndian, ByteOrder};
use std::collections::HashMap;
use std::iter::once;

#[derive(Clone, Debug, PartialEq)]
pub struct Solution2 {
    // token_id => price
    pub prices: HashMap<u16, u128>,
    pub executed_buy_amounts: Vec<u128>,
    pub executed_sell_amounts: Vec<u128>,
}

impl Solution2 {
    pub fn trivial(num_orders: usize) -> Self {
        Solution2 {
            prices: HashMap::new(),
            executed_buy_amounts: vec![0; num_orders],
            executed_sell_amounts: vec![0; num_orders],
        }
    }

    pub fn max_token(&self) -> Option<u16> {
        self.prices.keys().max().copied()
    }

    /// Returns true if a solution is non-trivial and false otherwise
    pub fn is_non_trivial(&self) -> bool {
        self.executed_sell_amounts.iter().any(|&amt| amt > 0)
    }
}

impl Serializable for Solution2 {
    fn bytes(&self) -> Vec<u8> {
        let max_token = self.max_token().unwrap_or(0u16);
        let mut res = (max_token + 1).to_be_bytes().to_vec();

        // Convert HashMap of prices to a price vector.
        let prices: Vec<u128> = (0..=max_token)
            .map(|x| *self.prices.get(&x).unwrap_or(&0u128))
            .collect();

        let alternating_buy_sell_amounts: Vec<u128> = self
            .executed_buy_amounts
            .iter()
            .zip(self.executed_sell_amounts.iter())
            .flat_map(|tup| once(tup.0).chain(once(tup.1)))
            .cloned()
            .collect();

        let prices_and_volumes: Vec<u8> = [prices, alternating_buy_sell_amounts]
            .iter()
            .flat_map(|list| list.iter())
            .flat_map(Serializable::bytes)
            .collect();
        res.extend(prices_and_volumes);
        res
    }
}

impl Deserializable for Solution2 {
    fn from_bytes(mut bytes: Vec<u8>) -> Self {
        // First 2 bytes encode the length of price vector (i.e. num_tokens)
        let len_prices = BigEndian::read_u16(&bytes[0..2]);
        let volumes = bytes.split_off(2 + len_prices as usize * 12);
        let price_vector: Vec<u128> = bytes[2..]
            .chunks_exact(12)
            .map(|chunk| util::read_amount(&util::get_amount_from_slice(chunk)))
            .collect();
        let prices = price_vector
            .iter()
            .enumerate()
            .filter(|t| *t.1 > 0)
            .map(|(i, v)| (i as u16, *v))
            .into_iter()
            .collect();
        info!("Parsed price vector as: {:?}", price_vector);

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
        Solution2 {
            prices,
            executed_buy_amounts,
            executed_sell_amounts,
        }
    }
}

#[cfg(test)]
pub mod unit_test {
    use super::*;

    fn map_from_list(arr: &[(u16, u128)]) -> HashMap<u16, u128> {
        arr.into_iter().copied().collect()
    }

    fn generic_non_trivial_solution() -> Solution2 {
        Solution2 {
            prices: map_from_list(&[(0, 42), (2, 42)]),
            executed_buy_amounts: vec![4, 5, 6],
            executed_sell_amounts: vec![1, 2, 3],
        }
    }

    #[test]
    fn test_is_non_trivial() {
        assert!(generic_non_trivial_solution().is_non_trivial());
        assert!(!Solution2::trivial(1).is_non_trivial());
    }

    #[test]
    fn test_max_token() {
        assert_eq!(generic_non_trivial_solution().max_token().unwrap(), 2);
        assert_eq!(Solution2::trivial(1).max_token(), None);
    }

    #[test]
    fn test_serialize() {
        assert_eq!(
            generic_non_trivial_solution().bytes(),
            vec![
                0, 3, // len(prices)
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, // price0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // price1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, // price2
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, // buyAmount0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, // sellAmount0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, // buyAmount1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // sellAmount1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 6, // buyAmount2
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, // sellAmount2
            ]
        );

        let solution = Solution2 {
            prices: [(0, 5), (1, 2)].into_iter().copied().collect(),
            executed_buy_amounts: vec![2u128.pow(8) + 1, 2u128.pow(24) + 3],
            executed_sell_amounts: vec![2u128.pow(16) + 2, 2u128.pow(32) + 4],
        };

        assert_eq!(
            solution.bytes(),
            vec![
                0, 2, // len(prices)
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, // price0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, // price1
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, // buyAmount0
                0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, // sellAmount0
                0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 3, // buyAmount1
                0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4, // sellAmount1
            ]
        );
    }

    #[test]
    fn test_serialize_deserialize() {
        let solution = generic_non_trivial_solution();
        let solution_bytes = solution.bytes();
        let parsed_solution = Solution2::from_bytes(solution_bytes);

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
        let parsed_solution = Solution2::from_bytes(bytes);
        let expected = Solution2 {
            prices: map_from_list(&[
                (0, 1),
                (1, 10u128.pow(18)),
                (2, 10u128.pow(18)),
                (3, 256),
                (4, 257),
            ]),
            executed_buy_amounts: vec![10u128.pow(18) + 1, 10u128.pow(18) + 3],
            executed_sell_amounts: vec![10u128.pow(18) + 2, 10u128.pow(18) + 4],
        };
        assert_eq!(parsed_solution, expected);
    }
}
