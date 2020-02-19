use crate::models;
use crate::price_finding;
use ethcontract::H160;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::rust::display_fromstr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// An opaque token ID as understood by the contract.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialOrd, PartialEq)]
pub struct TokenId(pub u16);

impl<'de> Deserialize<'de> for TokenId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let key = Cow::<str>::deserialize(deserializer)?;
        if !key.starts_with('T') || key.len() != 5 {
            return Err(D::Error::custom("Token ID must be of the form 'Txxxx'"));
        }

        let id = key[1..].parse::<u16>().map_err(D::Error::custom)?;
        Ok(TokenId(id))
    }
}

impl Serialize for TokenId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        format!("T{:04}", self.0).serialize(serializer)
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Num(#[serde(with = "display_fromstr")] pub u128);

#[derive(Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutedOrder {
    #[serde(default)]
    pub exec_sell_amount: Num,
    #[serde(default)]
    pub exec_buy_amount: Num,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    pub orders: Vec<ExecutedOrder>,
    pub prices: HashMap<TokenId, Option<Num>>,
}

impl Output {
    pub fn to_solution(&self) -> models::Solution {
        let prices = self
            .prices
            .iter()
            .map(|(token, price)| (token.0, price.unwrap_or_default().0))
            .collect();
        let executed_sell_amounts = self
            .orders
            .iter()
            .map(|order| order.exec_sell_amount.0)
            .collect();
        let executed_buy_amounts = self
            .orders
            .iter()
            .map(|order| order.exec_buy_amount.0)
            .collect();

        models::Solution {
            prices,
            executed_sell_amounts,
            executed_buy_amounts,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fee {
    pub token: TokenId,
    pub ratio: f64,
}

impl From<&'_ price_finding::Fee> for Fee {
    fn from(fee: &price_finding::Fee) -> Self {
        Fee {
            token: TokenId(fee.token),
            ratio: fee.ratio,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    #[serde(rename = "accountID")]
    pub account_id: H160,
    pub sell_token: TokenId,
    pub buy_token: TokenId,
    pub sell_amount: Num,
    pub buy_amount: Num,
}

impl From<&'_ models::Order> for Order {
    fn from(order: &models::Order) -> Self {
        Order {
            account_id: order.account_id,
            sell_token: TokenId(order.sell_token),
            buy_token: TokenId(order.buy_token),
            sell_amount: Num(order.sell_amount),
            buy_amount: Num(order.buy_amount),
        }
    }
}

pub type Accounts = BTreeMap<H160, BTreeMap<TokenId, Num>>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub tokens: BTreeSet<TokenId>,
    pub ref_token: TokenId,
    pub accounts: Accounts,
    pub orders: Vec<Order>,
    pub fee: Option<Fee>,
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn token_id_serialization() {
        for (key, expected) in &[
            (json!("T0000"), Some(TokenId(0))),
            (json!("T0042"), Some(TokenId(42))),
            (json!("T1000"), Some(TokenId(1000))),
            (json!("T001"), None),
            (json!("T00001"), None),
            (json!("00001"), None),
            (json!("Tasdf"), None),
        ] {
            let result = TokenId::deserialize(key);
            if let Some(expected) = expected {
                assert_eq!(result.unwrap(), *expected);
                assert_eq!(json!(expected), *key);
            } else {
                assert!(result.is_err());
            }
        }
    }

    #[test]
    fn output_deserialization() {
        let json = json!({
            "prices": {
                "T0000": "14024052566155238000",
                "T0001": "170141183460469231731687303715884105728", // greater than max value of u64
                "T0002": null,
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0"
                },
                {
                    "execSellAmount": "318390084925498118944",
                    "execBuyAmount": "95042777139162480000"
                },
            ]
        });
        let output = Output::deserialize(&json).unwrap();

        let expected_prices = vec![
            (TokenId(0), Some(Num(14_024_052_566_155_238_000))),
            (
                TokenId(1),
                Some(Num(170_141_183_460_469_231_731_687_303_715_884_105_728)),
            ),
            (TokenId(2), None),
        ];
        assert_eq!(output.prices, expected_prices.into_iter().collect());

        let expected_orders = vec![
            ExecutedOrder {
                exec_sell_amount: Num(0),
                exec_buy_amount: Num(0),
            },
            ExecutedOrder {
                exec_sell_amount: Num(318_390_084_925_498_118_944),
                exec_buy_amount: Num(95_042_777_139_162_480_000),
            },
        ];
        assert_eq!(output.orders, expected_orders);
    }

    #[test]
    fn output_deserialization_errors() {
        let json = json!({
            "The Prices": {
                "TA": "1",
                "TB": "2",
            },
        });
        Output::deserialize(&json).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "tkn1": "1",
            },
        });
        Output::deserialize(&json).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "TX": "1",
            },
        });
        Output::deserialize(&json).expect_err("Should fail to parse");

        let json = json!({
            "orders": [],
            "prices": {
                "T9999999999": "1",
            },
        });
        Output::deserialize(&json).expect_err("Should fail to parse");
    }

    #[test]
    fn output_deserialization_fails_if_prices_missing() {
        let json = json!({
            "orders": []
        });
        Output::deserialize(&json).expect_err("Should fail to parse");
    }

    #[test]
    fn output_deserialization_fails_if_orders_missing() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
        });
        Output::deserialize(&json).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_sell_amount() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execBuyAmount": "0"
                }
            ]
        });
        let result = Output::deserialize(&json).expect("Should not fail to parse");
        assert_eq!(result.orders[0].exec_sell_amount, Num(0));
    }

    #[test]
    fn output_deserialization_fails_if_order_sell_volume_not_parsable() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0a",
                    "execBuyAmount": "0"
                }
            ]
        });
        Output::deserialize(&json).expect_err("Should fail to parse");
    }

    #[test]
    fn serialize_result_assumes_zero_if_order_does_not_have_buy_amount() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0"
                }
            ]
        });
        let result = Output::deserialize(&json).expect("Should not fail to parse");
        assert_eq!(result.orders[0].exec_buy_amount, Num(0));
    }

    #[test]
    fn output_deserialization_fails_if_order_buy_volume_not_parsable() {
        let json = json!({
            "prices": {
                "T0000": "100",
            },
            "orders": [
                {
                    "execSellAmount": "0",
                    "execBuyAmount": "0a"
                }
            ]
        });
        Output::deserialize(&json).expect_err("Should fail to parse");
    }

    #[test]
    fn test_balance_serialization() {
        let mut accounts = BTreeMap::new();

        // Balances should end up ordered by token ID
        let mut user1_balances = BTreeMap::new();
        user1_balances.insert(TokenId(3), Num(100));
        user1_balances.insert(TokenId(2), Num(100));
        user1_balances.insert(TokenId(1), Num(100));
        user1_balances.insert(TokenId(0), Num(100));

        // Accounts should end up sorted by account ID
        accounts.insert(
            "4fd7c947ca0aba9d8678885e2b8c4d6a4e946984".parse().unwrap(),
            user1_balances,
        );
        accounts.insert(
            "13a0b42b9c180065510615972858bf41d1972a55".parse().unwrap(),
            BTreeMap::new(),
        );

        let input = Input {
            // tokens should also end up sorted in the end
            tokens: [TokenId(3), TokenId(2), TokenId(1), TokenId(0)]
                .iter()
                .copied()
                .collect(),
            ref_token: TokenId(0),
            accounts,
            orders: vec![],
            fee: None,
        };
        let result = serde_json::to_string(&input).expect("Unable to serialize account state");
        assert_eq!(
            result,
            r#"{"tokens":["T0000","T0001","T0002","T0003"],"refToken":"T0000","accounts":{"0x13a0b42b9c180065510615972858bf41d1972a55":{},"0x4fd7c947ca0aba9d8678885e2b8c4d6a4e946984":{"T0000":"100","T0001":"100","T0002":"100","T0003":"100"}},"orders":[],"fee":null}"#
        );
    }
}
