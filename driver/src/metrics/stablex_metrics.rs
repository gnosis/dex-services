use crate::error::DriverError;
use crate::price_finding::error::PriceFindingError;

use chrono::Utc;
use dfusion_core::models::{AccountState, Order, Solution};
use prometheus::{linear_buckets, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry};
use std::collections::HashSet;
use std::convert::TryInto;
use std::sync::Arc;
use web3::types::U256;

pub struct StableXMetrics {
    processing_times: HistogramVec,
    failures: IntCounterVec,
    successes: IntCounterVec,
    orders: IntCounterVec,
    tokens: IntCounterVec,
    users: IntCounterVec,
}

impl StableXMetrics {
    pub fn new(registry: Arc<Registry>) -> Self {
        let processing_time_opts = HistogramOpts::new(
            "processing_times",
            "timings between different processing stages",
        )
        .buckets(linear_buckets(0.0, 10.0, 30).unwrap()); // 5 minutes split into 10 seconds
        let processing_times = HistogramVec::new(processing_time_opts, &["stage"]).unwrap();
        registry
            .register(Box::new(processing_times.clone()))
            .unwrap();

        let failure_opts = Opts::new("failures", "number of auctions failed");
        let failures = IntCounterVec::new(failure_opts, &["stage"]).unwrap();
        registry.register(Box::new(failures.clone())).unwrap();

        let success_opts = Opts::new("success", "number of auctions successfully processed");
        let successes = IntCounterVec::new(success_opts, &["type"]).unwrap();
        registry.register(Box::new(successes.clone())).unwrap();

        let order_opts = Opts::new("orders", "number of orders in a batch");
        let orders = IntCounterVec::new(order_opts, &["type"]).unwrap();
        registry.register(Box::new(orders.clone())).unwrap();

        let token_opts = Opts::new("tokens", "number of distinct tokens in a batch");
        let tokens = IntCounterVec::new(token_opts, &["type"]).unwrap();
        registry.register(Box::new(tokens.clone())).unwrap();

        let users_opts = Opts::new("users", "number of distinct users in a batch");
        let users = IntCounterVec::new(users_opts, &["type"]).unwrap();
        registry.register(Box::new(users.clone())).unwrap();

        Self {
            processing_times,
            failures,
            successes,
            orders,
            tokens,
            users,
        }
    }

    pub fn auction_processing_started(&self, res: &Result<U256, DriverError>) {
        let label = &["start"];
        match res {
            Ok(batch) => {
                self.processing_times
                    .with_label_values(label)
                    .observe(time_elapsed_since_batch_start(*batch));
            }
            Err(_) => self.failures.with_label_values(label).inc(),
        };
    }

    pub fn auction_orders_fetched(
        &self,
        batch: U256,
        res: &Result<(AccountState, Vec<Order>), DriverError>,
    ) {
        let label = &["orders"];
        self.processing_times
            .with_label_values(label)
            .observe(time_elapsed_since_batch_start(batch));
        match res {
            Ok((_, orders)) => {
                self.orders
                    .with_label_values(label)
                    .inc_by(orders.len().try_into().unwrap_or(std::i64::MAX));
                self.tokens
                    .with_label_values(label)
                    .inc_by(tokens_from_orders(&orders));
                self.users
                    .with_label_values(label)
                    .inc_by(users_from_orders(&orders));
            }
            Err(_) => self.failures.with_label_values(label).inc(),
        }
    }

    pub fn auction_solution_computed(
        &self,
        batch: U256,
        orders: &Vec<Order>,
        res: &Result<Solution, PriceFindingError>,
    ) {
        let label = &["solution"];
        self.processing_times
            .with_label_values(label)
            .observe(time_elapsed_since_batch_start(batch));
        match res {
            Ok(solution) => {
                let touched_orders = orders
                    .iter()
                    .zip(&solution.executed_buy_amounts)
                    .filter(|(_, amount)| amount > &&0)
                    .map(|(o, _)| o.clone())
                    .collect::<Vec<Order>>();
                self.orders
                    .with_label_values(label)
                    .inc_by(touched_orders.len().try_into().unwrap_or(std::i64::MAX));
                self.tokens
                    .with_label_values(label)
                    .inc_by(tokens_from_orders(&touched_orders));
                self.users
                    .with_label_values(label)
                    .inc_by(users_from_orders(&touched_orders));
            }
            Err(_) => self.failures.with_label_values(label).inc(),
        }
    }

    pub fn auction_solution_verified(&self, batch: U256, res: &Result<U256, DriverError>) {
        let label = &["verification"];
        self.processing_times
            .with_label_values(label)
            .observe(time_elapsed_since_batch_start(batch));
        match res {
            Ok(_) => (),
            Err(_) => self.failures.with_label_values(label).inc(),
        }
    }

    pub fn auction_solution_submitted(&self, batch: U256, res: &Result<(), DriverError>) {
        let label = &["submission"];
        self.processing_times
            .with_label_values(label)
            .observe(time_elapsed_since_batch_start(batch));
        match res {
            Ok(_) => self.successes.with_label_values(label).inc(),
            Err(_) => self.failures.with_label_values(label).inc(),
        }
    }

    pub fn auction_skipped(&self, batch: U256) {
        let label = &["skipped"];
        self.processing_times
            .with_label_values(label)
            .observe(time_elapsed_since_batch_start(batch));
        self.successes.with_label_values(label).inc();
    }
}

fn time_elapsed_since_batch_start(batch: U256) -> f64 {
    let now = Utc::now().timestamp() as u64;
    // A new batch is created every 5 minutes and becomes solvable one batch later
    let batch_start = (batch.low_u64() + 1) * 300;
    (now - batch_start) as f64
}

fn tokens_from_orders(orders: &Vec<Order>) -> i64 {
    orders
        .iter()
        .flat_map(|order| vec![order.buy_token, order.sell_token].into_iter())
        .collect::<HashSet<_>>()
        .len()
        .try_into()
        .unwrap_or(std::i64::MAX)
}

fn users_from_orders(orders: &Vec<Order>) -> i64 {
    orders
        .iter()
        .map(|order| order.account_id)
        .collect::<HashSet<_>>()
        .len()
        .try_into()
        .unwrap_or(std::i64::MAX)
}

#[cfg(test)]
impl Default for StableXMetrics {
    fn default() -> Self {
        Self::new(Arc::new(Registry::new()))
    }
}
