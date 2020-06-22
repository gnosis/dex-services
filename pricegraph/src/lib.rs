mod encoding;
mod graph;
mod num;
mod orderbook;

#[cfg(test)]
#[path = "../data/mod.rs"]
mod data;

pub use encoding::*;
pub use orderbook::Orderbook;

/// The fee factor that is applied to each order's buy price.
const FEE_FACTOR: f64 = 1.0 / 0.999;

/// A struct representing a transitive orderbook for a base and quote token.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitiveOrderbook {
    /// Transitive "ask" orders, i.e. transitive orders buying the quote token
    /// and selling the base token.
    pub asks: Vec<TransitiveOrder>,
    /// Transitive "bid" orders, i.e. transitive orders buying the base token
    /// and selling the quote token.
    pub bids: Vec<TransitiveOrder>,
}

impl TransitiveOrderbook {
    /// Returns an iterator with ask prices (expressed in the quote token) and
    /// corresponding volumes.
    ///
    /// Note that the prices are effective prices and include fees.
    pub fn ask_prices(&self) -> impl DoubleEndedIterator<Item = (f64, f64)> + '_ {
        self.asks
            .iter()
            .map(|order| ((order.buy / order.sell) * FEE_FACTOR, order.sell))
    }

    /// Returns an iterator with bid prices (expressed in the quote token) and
    /// corresponding volumes.
    ///
    /// Note that the prices are effective prices and include fees.
    pub fn bid_prices(&self) -> impl DoubleEndedIterator<Item = (f64, f64)> + '_ {
        self.bids
            .iter()
            .map(|order| ((order.sell / order.buy) / FEE_FACTOR, order.buy))
    }
}

/// A struct representing a transitive order for trading between two tokens.
///
/// A transitive order is defined as the transitive combination of multiple
/// orders into a single equivalent order. For example consider the following
/// two orders:
/// - *A*: buying 1.0 token 1 selling 2.0 token 2
/// - *B*: buying 4.0 token 2 selling 1.0 token 3
///
/// We can define a transitive order *C* buying 1.0 token 1 selling 0.5 token 3
/// by combining *A* and *B*. Note that the sell amount of token 3 is limited by
/// the token 2 capacity for this transitive order.
///
/// Additionally, a transitive order over a single order is equal to that order.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitiveOrder {
    /// The effective buy amount for this transient order.
    pub buy: f64,
    /// The effective sell amount for this transient order.
    pub sell: f64,
}

impl TransitiveOrder {
    /// Retrieves the exchange rate for this order.
    pub fn exchange_rate(&self) -> f64 {
        self.buy / self.sell
    }

    /// Retrieves the effective exchange rate for this order after fees are
    /// condidered.
    ///
    /// Note that `effective_exchange_rate > exchange_rate`.
    pub fn effective_exchange_rate(&self) -> f64 {
        self.exchange_rate() * FEE_FACTOR
    }
}

/// API entry point for computing price estimates and transitive orderbooks for
/// a give auction.
#[derive(Clone, Debug)]
pub struct Pricegraph {
    full_orderbook: Orderbook,
    reduced_orderbook: Orderbook,
}

impl Pricegraph {
    /// Create a new `Pricegraph` instance given an iterator of auction elements
    /// for the batch.
    ///
    /// The auction elements are in the standard exchange format.
    pub fn new(elements: impl IntoIterator<Item = Element>) -> Self {
        let orderbook = Orderbook::from_elements(elements);
        Pricegraph::from_orderbook(orderbook)
    }

    /// Create a new `Pricegraph` instance from an `Orderbook`.
    pub fn from_orderbook(mut orderbook: Orderbook) -> Self {
        let full_orderbook = orderbook.clone();
        let reduced_orderbook = {
            orderbook.reduce_overlapping_orders();
            orderbook
        };

        Pricegraph {
            full_orderbook,
            reduced_orderbook,
        }
    }

    /// Gets a clone of the full orderbook for operations that need to contain
    /// the existing overlapping transitive orders for accuracy. A clone is
    /// returned because orderbook operations are destructive.
    pub fn full_orderbook(&self) -> Orderbook {
        self.full_orderbook.clone()
    }

    /// Gets a clone of the reduced orderbook for operations that prefer there
    /// to be no overlapping transitive orders. A clone is returned because
    /// orderbook operations are destructive.
    pub fn reduced_orderbook(&self) -> Orderbook {
        self.reduced_orderbook.clone()
    }

    /// Estimates an exchange rate for the specified token pair and sell volume.
    /// Returns `None` if no combination of orders is able to trade this amount
    /// of the sell token into the buy token. This usually occurs if there is
    /// not enough buy token liquidity in the exchange or if there is no inverse
    /// transitive orders buying the specified sell token for the specified buy
    /// token.
    ///
    /// Note that this price is in exchange format, that is, it is expressed as
    /// the ratio between buy and sell amounts, with implicit fees.
    pub fn estimate_exchange_rate(&self, pair: TokenPair, sell_amount: f64) -> Option<f64> {
        self.reduced_orderbook()
            .fill_market_order(pair, sell_amount as _)
    }

    /// Returns a transitive order with a buy amount calculated such that there
    /// exists overlapping transitive orders to completely fill the specified
    /// `sell_amount`. As such, this is an estimated order that is *likely* to
    /// be matched given the **current** state of the batch.
    pub fn order_for_sell_amount(
        &self,
        pair: TokenPair,
        sell_amount: f64,
    ) -> Option<TransitiveOrder> {
        let price = self.estimate_exchange_rate(pair, sell_amount)?;
        Some(TransitiveOrder {
            buy: sell_amount * price,
            sell: sell_amount,
        })
    }

    /// Computes a transitive orderbook for the given market specified by the
    /// base and quote tokens.
    ///
    /// This method optionally accepts a spread that is a decimal fraction that
    /// defines the maximume transitive order price with the equation:
    /// `first_transitive_price + first_transitive_price * spread`. This means
    /// that given a spread of 0.5 (or 50%), and if the cheapest transitive
    /// order has a price of 1.2, then the maximum price will be `1.8`.
    ///
    /// The spread applies to both `asks` and `bids` transitive orders.
    ///
    /// # Panics
    ///
    /// This method panics if the spread is zero or negative.
    pub fn transitive_orderbook(
        &self,
        base: TokenId,
        quote: TokenId,
        spread: Option<f64>,
    ) -> TransitiveOrderbook {
        let mut orderbook = self.full_orderbook();

        let mut transitive_orderbook =
            orderbook.reduce_overlapping_transitive_orderbook(base, quote);
        transitive_orderbook
            .asks
            .extend(orderbook.clone().fill_transitive_orders(
                TokenPair {
                    buy: quote,
                    sell: base,
                },
                spread,
            ));
        transitive_orderbook
            .bids
            .extend(orderbook.fill_transitive_orders(
                TokenPair {
                    buy: base,
                    sell: quote,
                },
                spread,
            ));

        for orders in &mut [
            &mut transitive_orderbook.asks,
            &mut transitive_orderbook.bids,
        ] {
            orders.sort_unstable_by(|a, b| num::compare(a.exchange_rate(), b.exchange_rate()));
        }

        transitive_orderbook
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_approx_eq::assert_approx_eq;
    use primitive_types::U256;

    #[test]
    fn transitive_orderbook_empty_same_token() {
        let pricegraph = Pricegraph::new(std::iter::empty());
        let orderbook = pricegraph.transitive_orderbook(0, 0, None);
        assert!(orderbook.asks.is_empty());
        assert!(orderbook.bids.is_empty());
    }

    #[test]
    fn transitive_orderbook_simple() {
        let user0 = UserId::from_low_u64_le(0);
        let base: u128 = 1_000_000_000_000;
        let pricegraph = Pricegraph::new(vec![Element {
            user: user0,
            balance: U256::from(2) * U256::from(base),
            pair: TokenPair { buy: 0, sell: 1 },
            valid: Validity { from: 0, to: 1 },
            price: Price {
                numerator: 2 * base,
                denominator: base,
            },
            remaining_sell_amount: base,
            id: 0,
        }]);

        let orderbook = pricegraph.transitive_orderbook(0, 1, None);
        assert_eq!(orderbook.asks, vec![]);
        assert_eq!(
            orderbook.bids,
            vec![TransitiveOrder {
                buy: 2.0 * base as f64,
                sell: base as f64,
            }]
        );
        let bid_price = orderbook.bid_prices().next().unwrap();
        assert_approx_eq!(bid_price.0, 0.5 / FEE_FACTOR);

        let orderbook = pricegraph.transitive_orderbook(1, 0, None);
        assert_eq!(
            orderbook.asks,
            vec![TransitiveOrder {
                buy: 2.0 * base as f64,
                sell: base as f64,
            }]
        );
        let ask_price = orderbook.ask_prices().next().unwrap();
        assert_approx_eq!(ask_price.0, 2.0 * FEE_FACTOR);
        assert_eq!(orderbook.bids, vec![]);
    }

    #[test]
    fn transitive_orderbook_prices() {
        let transitive_orderbook = TransitiveOrderbook {
            asks: vec![
                TransitiveOrder {
                    buy: 20_000_000.0,
                    sell: 10_000_000.0,
                },
                TransitiveOrder {
                    buy: 1_500_000.0,
                    sell: 900_000.0,
                },
            ],
            bids: vec![
                TransitiveOrder {
                    buy: 1_000_000.0,
                    sell: 2_000_000.0,
                },
                TransitiveOrder {
                    buy: 500_000.0,
                    sell: 900_000.0,
                },
            ],
        };

        let ask_prices = transitive_orderbook.ask_prices().collect::<Vec<_>>();
        assert_approx_eq!(ask_prices[0].0, 2.0 * FEE_FACTOR);
        assert_approx_eq!(ask_prices[0].1, 10_000_000.0);
        assert_approx_eq!(ask_prices[1].0, 1.5 / 0.9 * FEE_FACTOR);
        assert_approx_eq!(ask_prices[1].1, 900_000.0);

        let bid_prices = transitive_orderbook.bid_prices().collect::<Vec<_>>();
        assert_approx_eq!(bid_prices[0].0, 2.0 / FEE_FACTOR);
        assert_approx_eq!(bid_prices[0].1, 1_000_000.0);
        assert_approx_eq!(bid_prices[1].0, 9.0 / 5.0 / FEE_FACTOR);
        assert_approx_eq!(bid_prices[1].1, 500_000.0);
    }

    #[test]
    fn real_orderbooks() {
        // The output of this test can be seen with:
        // ```
        // cargo test -p pricegraph real_orderbooks -- --nocapture
        // ```

        let base_unit = 10.0f64.powi(18);

        let dai_weth = TokenPair { buy: 7, sell: 1 };
        let volume = 1.0 * base_unit;
        let spread = 0.05;

        for (batch_id, raw_orderbook) in data::ORDERBOOKS.iter() {
            let pricegraph = Pricegraph::from_orderbook(Orderbook::read(raw_orderbook).unwrap());

            let order = pricegraph.order_for_sell_amount(dai_weth, volume).unwrap();
            println!(
                "#{}: estimated order for buying {} DAI for {} WETH",
                batch_id,
                order.buy / base_unit,
                order.sell / base_unit,
            );

            let TransitiveOrderbook { asks, bids } =
                pricegraph.transitive_orderbook(dai_weth.buy, dai_weth.sell, Some(spread));
            println!(
                "#{}: DAI-WETH market contains {} ask orders and {} bid orders within a {}% spread:",
                batch_id,
                asks.len(),
                bids.len(),
                100.0 * spread,
            );

            for (name, buy_token, sell_token, orders) in
                &[("Ask", "DAI", "WETH", asks), ("Bid", "WETH", "DAI", bids)]
            {
                println!(" - {} orders", name);

                let mut last_xrate = orders[0].exchange_rate();
                for order in orders {
                    assert!(last_xrate <= order.exchange_rate());
                    last_xrate = order.exchange_rate();

                    println!(
                        "    buy {} {} for {} {}",
                        order.buy / base_unit,
                        buy_token,
                        order.sell / base_unit,
                        sell_token,
                    );
                }
            }
        }
    }
}
