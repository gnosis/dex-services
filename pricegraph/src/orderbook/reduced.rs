//! Module containing reduced orderbook wrapper type.

use crate::encoding::TokenPair;
use crate::orderbook::{Flow, Orderbook};

/// A graph representation of a reduced orderbook. Reduced orderbooks are
/// guaranteed to not contain any negative cycles.
#[derive(Clone, Debug)]
pub struct ReducedOrderbook(pub(super) Orderbook);

impl ReducedOrderbook {
    /// Returns the number of orders in the orderbook.
    pub fn num_orders(&self) -> usize {
        self.0.num_orders()
    }

    /// Fills the optimal transitive order for the specified token pair. This
    /// method is similar to
    /// `ReducedOrderbook::fill_optimal_transitive_order_if` except it does not
    /// check a condition on the discovered path's flow before filling.
    pub fn fill_optimal_transitive_order(&mut self, pair: TokenPair) -> Option<Flow> {
        self.fill_optimal_transitive_order_if(pair, |_| true)
    }

    /// Finds and returns the optimal transitive order for the specified token
    /// pair without filling it. Returns `None` if no such transitive order
    /// exists.
    pub fn find_optimal_transitive_order(&mut self, pair: TokenPair) -> Option<Flow> {
        self.0
            .find_optimal_transitive_order(pair)
            .expect("negative cycle in reduced orderbook")
    }

    /// Fills the optimal transitive order (i.e. with the lowest exchange rate)
    /// for the specified token pair by pushing flow from the buy token to the
    /// sell token, if the condition is met. The trading path through the
    /// orderbook graph is filled to maximum capacity, reducing the remaining
    /// order amounts and user balances along the way, returning the flow for
    /// the path.
    ///
    /// Returns `None` if the condition is not met or there is no path between
    /// the token pair.
    pub fn fill_optimal_transitive_order_if(
        &mut self,
        pair: TokenPair,
        condition: impl FnMut(&Flow) -> bool,
    ) -> Option<Flow> {
        self.0
            .fill_optimal_transitive_order_if(pair, condition)
            .expect("negative cycle in reduced orderbook")
    }

    /// Unwraps the reduced orderbook into its inner `Orderbook` instance.
    pub fn into_inner(self) -> Orderbook {
        self.0
    }
}
