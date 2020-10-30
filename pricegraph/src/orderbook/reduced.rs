//! Module containing reduced orderbook wrapper type.

use crate::encoding::TokenPairRange;
use crate::orderbook::{Flow, Orderbook, TransitiveOrders};

/// A graph representation of a reduced orderbook. Reduced orderbooks are
/// guaranteed to not contain any negative cycles.
#[derive(Clone, Debug)]
pub struct ReducedOrderbook(pub(super) Orderbook);

impl ReducedOrderbook {
    /// Returns the number of orders in the orderbook.
    pub fn num_orders(&self) -> usize {
        self.0.num_orders()
    }

    /// Returns an iterator over all transitive orders from lowest to highest
    /// limit price for the orderbook.
    pub fn transitive_orders(self, pair_range: TokenPairRange) -> TransitiveOrders {
        TransitiveOrders::new(self.0, pair_range).expect("negative cycle in reduced orderbook")
    }

    /// Returns an iterator over all significant transitive orders (i.e. **not**
    /// dust transitive orders) from lowest to highest limit price for the
    /// orderbook.
    ///
    /// This is a convenience method for:
    /// `orderbook.transtive_orders().filter(|flow| !flow.is_dust_trade())`.
    pub fn significant_transitive_orders(
        self,
        pair_range: TokenPairRange,
    ) -> impl Iterator<Item = Flow> {
        self.transitive_orders(pair_range)
            .filter(|flow| !flow.is_dust_trade())
    }

    /// Finds and returns the optimal transitive order for the specified token
    /// pair without filling it. Returns `None` if no such transitive order
    /// exists.
    pub fn find_optimal_transitive_order(&mut self, pair_range: TokenPairRange) -> Option<Flow> {
        self.0
            .find_optimal_transitive_order(pair_range)
            .expect("negative cycle in reduced orderbook")
    }

    /// Unwraps the reduced orderbook into its inner `Orderbook` instance.
    pub fn into_inner(self) -> Orderbook {
        self.0
    }
}
