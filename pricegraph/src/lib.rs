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
    /// Transitive "ask" orders, i.e. transitive orders buying the base token
    /// and selling the quote token.
    pub asks: Vec<TransitiveOrder>,
    /// Transitive "bid" orders, i.e. transitive orders buying the quote token
    /// and selling the base token.
    pub bids: Vec<TransitiveOrder>,
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

    /// Gets a copy of the full orderbook for operations that need to contain
    /// the existing overlapping transitive orders for accuracy.
    ///
    /// This method returns a clone of the reduced orderbook because orderbook
    /// operations are destructive (as they require filling orders).
    pub fn full_orderbook(&self) -> Orderbook {
        self.full_orderbook.clone()
    }

    /// Gets a copy of the reduced orderbook for operations that prefer there to
    /// be no overlapping transitive orders.
    ///
    /// This method returns a clone of the reduced orderbook because orderbook
    /// operations are destructive (as they require filling orders).
    pub fn reduced_orderbook(&self) -> Orderbook {
        self.reduced_orderbook.clone()
    }

    /// Estimates an exchange rate for the specified token pair and sell volume.
    /// Returns `None` if the volume cannot be fully filled because there are
    /// not enough liquidity in the current batch.
    ///
    /// Note that this price is in exchange format, that is, it is expressed as
    /// the ratio between buy and sell amounts, with implicit fees.
    pub fn estimate_exchange_rate(&self, pair: TokenPair, sell_amount: f64) -> Option<f64> {
        self.reduced_orderbook()
            .fill_market_order(pair, sell_amount as _)
    }

    /// Returns a transitive order with a buy amount calculated such that there
    /// exists overlapping transitive orders to completely fill the speicified
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_orderbooks() {
        // The output of this test can be seen with:
        // ```
        // cargo test -p pricegraph real_orderbooks -- --nocapture
        // ```

        let base_unit = 10.0f64.powi(18);

        let dai_weth = TokenPair { buy: 7, sell: 1 };
        let volume = 1.0 * base_unit;

        for (batch_id, raw_orderbook) in data::ORDERBOOKS.iter() {
            let pricegraph = Pricegraph::from_orderbook(Orderbook::read(raw_orderbook).unwrap());
            let order = pricegraph.order_for_sell_amount(dai_weth, volume).unwrap();
            println!(
                "#{}: estimated order for buying {} DAI for {} WETH",
                batch_id,
                order.buy / base_unit,
                order.sell / base_unit,
            );
        }
    }
}
