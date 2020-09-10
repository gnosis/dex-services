//! This module contains implementation of the price finding algorithm for
//! providing price estimates to the solver.
//!
//! This algorithm works by reducing ring trades (i.e. negative cycles) in the
//! orderbook and computing a price vector for that cycle. For each token, the
//! last discovered price is used as the price estimate. The last price is used
//! to ensure that the estimate is closest to the intersection point of the
//! orderbook graph in the market the trade was on.

use crate::{
    graph::{path::NegativeCycle, shortest_paths::ShortestPathGraph, subgraph::Subgraphs},
    num,
    orderbook::{self, reduced::ReducedOrderbook, scalar::ExchangeRate, Orderbook},
    TokenId, TokenPair, FEE_TOKEN,
};
use petgraph::graph::NodeIndex;
use std::collections::HashMap;

/// Price estimates to be given to the solver as a starting point for its
/// optimization problem.
pub struct Prices {
    traded_tokens: HashMap<TokenId, ExchangeRate>,
    orderbook: ReducedOrderbook,
}

impl Prices {
    /// Constructs a new price estimate from the specified orderbook.
    pub fn new(mut orderbook: Orderbook) -> Self {
        let mut traded_tokens = HashMap::new();
        traded_tokens.insert(FEE_TOKEN, ExchangeRate::IDENTITY);

        Subgraphs::new(orderbook.projection.node_indices()).for_each(|token| loop {
            match ShortestPathGraph::new(&orderbook.projection, token) {
                Ok(shortest_path_graph) => break shortest_path_graph.connected_nodes(),
                Err(mut cycle) => {
                    set_prices_for_cycle(&mut traded_tokens, &orderbook, &mut cycle);
                    orderbook.fill_path(&cycle).unwrap_or_else(|| {
                        panic!(
                            "failed to fill path along detected negative cycle {}",
                            orderbook::format_path(&cycle),
                        )
                    });
                }
            }
        });

        Self {
            traded_tokens,
            orderbook: ReducedOrderbook(orderbook),
        }
    }

    /// Returns the price estimate for the specified token. Returns `None` if no
    /// price estimate could be determined.
    ///
    /// Note that the prices are in price vector format, that is they are the
    /// amount of fee token required to buy `1e18` of the specified token.
    pub fn token_price(&self, token: TokenId) -> Option<f64> {
        let xrate = self.traded_tokens.get(&token).copied().or_else(|| {
            // NOTE: If the token was not part of a negative cycle (i.e.
            // ring trade) then estimate its price by finding the best order
            // **selling fee token**, as this is the direction required by the
            // solver in order to balance the fees.
            let flow = self.orderbook.find_optimal_transitive_order(TokenPair {
                buy: token,
                sell: FEE_TOKEN,
            })?;
            Some(flow.exchange_rate)
        })?;

        Some(xrate.value() * 1e18)
    }
}

/// Computes the transitive exchange for each pair on the cycle as well and
/// maximum capacity for the cycle with uniform clearing prices.
///
/// This means that the resulting exchange rates are such that for all token
/// pair orders that are not fully filled, the exchange rate is exactly their
/// limit exchange rate, and the total transitive exchange rate of the cycle is
/// the identity exchange rate (`1.0`).
fn find_uniform_prices(
    orderbook: &Orderbook,
    cycle: &NegativeCycle<NodeIndex>,
) -> (Vec<(ExchangeRate, f64)>, f64) {
    debug_assert!(
        cycle.len() > 2,
        "cycle must have at least two pairs {}",
        orderbook::format_path(&cycle)
    );

    // NOTE: Start by computing executed trades using the limit prices. Because
    // we were given a negative cycle, then the final transitive exchange rate
    // will be negative.
    let mut executed_trades = orderbook::pairs_on_path(&cycle)
        .scan(ExchangeRate::IDENTITY, |transitive_xrate, pair| {
            let order = orderbook
                .orders
                .best_order_for_pair(pair)
                .unwrap_or_else(|| {
                    panic!(
                        "failed to find order pair {}->{} on detected path",
                        pair.buy, pair.sell,
                    )
                });

            *transitive_xrate *= order.exchange_rate;
            let capacity = order.get_effective_amount(&orderbook.users) * transitive_xrate.value();

            Some((*transitive_xrate, capacity))
        })
        .collect::<Vec<_>>();
    let (cycle_xrate, _) = executed_trades
        .last()
        .expect("cycle has at least two pairs");

    // NOTE: Now we have to adjust the executed exchange rate of the smallest
    // orders (i.e. the "market orders") such that the cycle's total transitive
    // exchange rate is `1.0`.
    let total_adjustment = cycle_xrate.inverse().value();

    let min_capacity = executed_trades
        .iter()
        .map(|(_, capacity)| *capacity)
        .min_by(|a, b| num::compare(*a, *b))
        .expect("cycle has at least two pairs");
    let target_capacity = min_capacity * total_adjustment;

    let mut current_adjustment = 1.0;
    for (xrate, capacity) in &mut executed_trades {
        if *capacity * current_adjustment < target_capacity {
            // NOTE: This means that this executed trade is a "market order" in
            // that it limits the flow through the path. Its exchange rate can
            // be increased so that the capacity is the targeted capacity while
            // still being fully filled.
            current_adjustment = target_capacity / *capacity;
        }

        *xrate *= ExchangeRate::new(current_adjustment)
            .expect("executed trade adjustment factor is [1.0, +∞)");
        *capacity *= current_adjustment;
    }

    debug_assert!({
        let (cycle_xrate, _) = executed_trades
            .last()
            .expect("cycle has at least two pairs");
        cycle_xrate.is_identity()
    });

    (executed_trades, target_capacity)
}

fn set_prices_for_cycle(
    prices: &mut HashMap<TokenId, ExchangeRate>,
    orderbook: &Orderbook,
    cycle: &mut NegativeCycle<NodeIndex>,
) {
    // NOTE: Cycles that contain the fee token need are handled differently than
    // ones that don't, so try and rotate the cycle to start with the fee token,
    // so we know which case we are dealing with.
    let fee_token_cycle = cycle
        .rotate_to_starting_node(orderbook::node_index(FEE_TOKEN))
        .is_ok();

    // NOTE: Compute uniform clearing prices for the starting token. For this we
    // need to find the "market orders", i.e. the orders that limit the flow
    // around this cycle. The end result is a price vector such that the xrate
    // for all orders that are not fully filled are exactly their limit xrates
    // and the xrate of the cycle is `1.0` (i.e. weight of `0`).

    todo!();
}
