#![deny(clippy::unreadable_literal)]

#[cfg(test)]
#[macro_use]
mod test;

mod api;
mod encoding;
mod graph;
pub mod num;
mod orderbook;

pub use self::api::*;
pub use self::encoding::*;
pub use self::orderbook::*;

/// The fee factor that is applied to each order's buy price.
pub const FEE_FACTOR: f64 = 1.0 / 0.999;

/// The fee token ID.
const FEE_TOKEN: TokenId = 0;

/// The minimum amount that must be traded for an order to be valid within a
/// solution. Orders with effective sell amounts smaller than this amount can
/// safely be ignored, and transitive orders with flows that trade amounts
/// smaller than this are not considered for price estimates.
pub const MIN_AMOUNT: u128 = 10_000;

/// API entry point for computing price estimates and transitive orderbooks for
/// a give auction.
#[derive(Clone, Debug)]
pub struct Pricegraph {
    full_orderbook: Orderbook,
    reduced_orderbook: ReducedOrderbook,
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

    /// Create a new `Pricegraph` instance from encoded auction elements.
    ///
    /// The orderbook is expected to be encoded as an indexed order as encoded
    /// by `BatchExchangeViewer::getFilteredOrdersPaginated`. Specifically, each
    /// order has a `114` byte stride with the following values (appearing in
    /// encoding order, all values are little endian encoded).
    /// - `20` bytes: owner's address
    /// - `32` bytes: owners's sell token balance
    /// - `2` bytes: buy token ID
    /// - `2` bytes: sell token ID
    /// - `4` bytes: valid from batch ID
    /// - `4` bytes: valid until batch ID
    /// - `16` bytes: price numerator
    /// - `16` bytes: price denominator
    /// - `16` bytes: remaining order sell amount
    /// - `2` bytes: order ID
    pub fn read(bytes: impl AsRef<[u8]>) -> Result<Self, InvalidLength> {
        let elements = Element::read_all(bytes.as_ref())?;
        Ok(Pricegraph::new(elements))
    }

    /// Create a new `Pricegraph` instance from an `Orderbook`.
    pub fn from_orderbook(orderbook: Orderbook) -> Self {
        let full_orderbook = orderbook.clone();
        let reduced_orderbook = orderbook.reduce_overlapping_orders();

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
    pub fn reduced_orderbook(&self) -> ReducedOrderbook {
        self.reduced_orderbook.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::prelude::*;

    #[test]
    fn real_orderbooks() {
        // The output of this test can be seen with:
        // ```
        // cargo test -p pricegraph real_orderbooks -- --nocapture
        // ```

        let base_unit = 1e18;

        let dai_weth = Market { base: 7, quote: 1 };
        let volume = 1.0 * base_unit;
        let spread = 0.05;

        for (batch_id, raw_orderbook) in data::ORDERBOOKS.iter() {
            let pricegraph = Pricegraph::read(raw_orderbook).unwrap();

            let order = pricegraph
                .order_for_sell_amount(dai_weth.bid_pair().into_unbounded_range(), volume)
                .unwrap();
            println!(
                "#{}: estimated order for buying {} DAI for {} WETH",
                batch_id,
                order.buy / base_unit,
                order.sell / base_unit,
            );

            let TransitiveOrderbook { asks, bids } =
                pricegraph.transitive_orderbook(dai_weth, None, Some(spread));
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
