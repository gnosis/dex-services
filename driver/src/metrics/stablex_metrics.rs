use crate::models::{AccountState, Order, Solution};
use crate::solution_submission::SolutionSubmissionError;
use anyhow::Result;
use chrono::Utc;
use ethcontract::U256;
use prometheus::{IntCounterVec, IntGaugeVec, Opts, Registry};
use std::collections::HashSet;
use std::convert::TryInto;
use std::sync::Arc;

pub struct StableXMetrics {
    processing_times: IntGaugeVec,
    failures: IntCounterVec,
    successes: IntCounterVec,
    orders: IntGaugeVec,
    tokens: IntGaugeVec,
    users: IntGaugeVec,
}

impl StableXMetrics {
    pub fn new(registry: Arc<Registry>) -> Self {
        let processing_time_opts = Opts::new(
            "dfusion_service_processing_times",
            "timings between different processing stages",
        );
        let processing_times =
            IntGaugeVec::new(processing_time_opts, &[ProcessingStage::LABEL]).unwrap();
        registry
            .register(Box::new(processing_times.clone()))
            .unwrap();

        let failure_opts = Opts::new("dfusion_service_failures", "number of auctions failed");
        let failures = IntCounterVec::new(failure_opts, &[ProcessingStage::LABEL]).unwrap();
        ProcessingStage::initialize_counters(&failures);
        registry.register(Box::new(failures.clone())).unwrap();

        let success_opts = Opts::new(
            "dfusion_service_success",
            "number of auctions successfully processed",
        );
        let successes = IntCounterVec::new(success_opts, &[ProcessingStage::LABEL]).unwrap();
        ProcessingStage::initialize_counters(&successes);
        registry.register(Box::new(successes.clone())).unwrap();

        let order_opts = Opts::new("dfusion_service_orders", "number of orders in a batch");
        let orders = IntGaugeVec::new(order_opts, &[BookType::LABEL]).unwrap();
        BookType::initialize_gauges(&orders);
        registry.register(Box::new(orders.clone())).unwrap();

        let token_opts = Opts::new(
            "dfusion_service_tokens",
            "number of distinct tokens in a batch",
        );
        let tokens = IntGaugeVec::new(token_opts, &[BookType::LABEL]).unwrap();
        BookType::initialize_gauges(&tokens);
        registry.register(Box::new(tokens.clone())).unwrap();

        let users_opts = Opts::new(
            "dfusion_service_users",
            "number of distinct users in a batch",
        );
        let users = IntGaugeVec::new(users_opts, &[BookType::LABEL]).unwrap();
        BookType::initialize_gauges(&users);
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

    pub fn auction_processing_started(&self, res: &Result<U256>) {
        let stage_label = &[ProcessingStage::Started.as_ref()];
        match res {
            Ok(batch) => {
                self.processing_times
                    .with_label_values(stage_label)
                    .set(time_elapsed_since_batch_start(*batch));
            }
            Err(_) => self.failures.with_label_values(stage_label).inc(),
        };
    }

    pub fn auction_orders_fetched(&self, batch: U256, res: &Result<(AccountState, Vec<Order>)>) {
        let stage_label = &[ProcessingStage::OrdersFetched.as_ref()];
        let book_label = &[BookType::Orderbook.as_ref()];
        self.processing_times
            .with_label_values(stage_label)
            .set(time_elapsed_since_batch_start(batch));
        match res {
            Ok((_, orders)) => {
                self.orders
                    .with_label_values(book_label)
                    .set(orders.len().try_into().unwrap_or(std::i64::MAX));
                self.tokens
                    .with_label_values(book_label)
                    .set(tokens_from_orders(&orders));
                self.users
                    .with_label_values(book_label)
                    .set(users_from_orders(&orders));
            }
            Err(_) => self.failures.with_label_values(stage_label).inc(),
        }
    }

    pub fn auction_solution_computed(&self, batch: U256, res: &Result<Solution>) {
        let stage_label = &[ProcessingStage::Solved.as_ref()];
        let book_label = &[BookType::Solution.as_ref()];
        self.processing_times
            .with_label_values(stage_label)
            .set(time_elapsed_since_batch_start(batch));
        match res {
            Ok(solution) => {
                self.orders.with_label_values(book_label).set(
                    solution
                        .executed_orders
                        .len()
                        .try_into()
                        .unwrap_or(std::i64::MAX),
                );
                self.tokens
                    .with_label_values(book_label)
                    .set(tokens_from_solution(solution));
                self.users
                    .with_label_values(book_label)
                    .set(users_from_solution(solution));
            }
            Err(_) => self.failures.with_label_values(stage_label).inc(),
        }
    }

    pub fn auction_solution_verified(
        &self,
        batch: U256,
        res: &Result<U256, SolutionSubmissionError>,
    ) {
        let stage_label = &[ProcessingStage::Solved.as_ref()];
        self.processing_times
            .with_label_values(stage_label)
            .set(time_elapsed_since_batch_start(batch));
        match res {
            Ok(_) => (),
            Err(err) => match err {
                SolutionSubmissionError::Benign(_) => (),
                SolutionSubmissionError::Unexpected(_) => {
                    self.failures.with_label_values(stage_label).inc()
                }
            },
        }
    }

    pub fn auction_solution_submitted(
        &self,
        batch: U256,
        res: &Result<(), SolutionSubmissionError>,
    ) {
        let stage_label = &[ProcessingStage::Submitted.as_ref()];
        self.processing_times
            .with_label_values(stage_label)
            .set(time_elapsed_since_batch_start(batch));
        match res {
            Ok(_) => self.successes.with_label_values(stage_label).inc(),
            Err(err) => match err {
                SolutionSubmissionError::Benign(_) => (),
                SolutionSubmissionError::Unexpected(_) => {
                    self.failures.with_label_values(stage_label).inc()
                }
            },
        }
    }

    pub fn auction_skipped(&self, batch: U256) {
        let stage_label = &[ProcessingStage::Skipped.as_ref()];
        self.processing_times
            .with_label_values(stage_label)
            .set(time_elapsed_since_batch_start(batch));
        self.successes.with_label_values(stage_label).inc();
    }
}

fn time_elapsed_since_batch_start(batch: U256) -> i64 {
    let now = Utc::now().timestamp();
    // A new batch is created every 5 minutes and becomes solvable one batch later
    let batch_start = (batch.low_u64() as i64 + 1) * 300;
    now - batch_start
}

fn tokens_from_orders(orders: &[Order]) -> i64 {
    orders
        .iter()
        .flat_map(|order| vec![order.buy_token, order.sell_token].into_iter())
        .collect::<HashSet<_>>()
        .len()
        .try_into()
        .unwrap_or(std::i64::MAX)
}

fn tokens_from_solution(solution: &Solution) -> i64 {
    if solution.is_non_trivial() {
        solution
            .prices
            .iter()
            .filter(|(_token_id, price)| **price > 0)
            .count()
            .try_into()
            .unwrap_or(std::i64::MAX)
    } else {
        0
    }
}

fn users_from_orders(orders: &[Order]) -> i64 {
    orders
        .iter()
        .map(|order| order.account_id)
        .collect::<HashSet<_>>()
        .len()
        .try_into()
        .unwrap_or(std::i64::MAX)
}

fn users_from_solution(solution: &Solution) -> i64 {
    solution
        .executed_orders
        .iter()
        .map(|order| order.account_id)
        .collect::<HashSet<_>>()
        .len()
        .try_into()
        .unwrap_or(std::i64::MAX)
}

trait InitializeableMetric: 'static + Sized + AsRef<str> {
    const LABEL: &'static str;
    const ALL_STAGES: &'static [Self];

    fn initialize_counters(c: &IntCounterVec) {
        for stage in Self::ALL_STAGES {
            c.with_label_values(&[stage.as_ref()]).inc_by(0);
        }
    }

    fn initialize_gauges(g: &IntGaugeVec) {
        for stage in Self::ALL_STAGES {
            g.with_label_values(&[stage.as_ref()]).set(0);
        }
    }
}

enum ProcessingStage {
    Started,
    OrdersFetched,
    Solved,
    Verified,
    Submitted,
    Skipped,
}

impl AsRef<str> for ProcessingStage {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::OrdersFetched => "orders_fetched",
            Self::Solved => "solved",
            Self::Verified => "verified",
            Self::Submitted => "submitted",
            Self::Skipped => "skipped",
        }
    }
}

impl InitializeableMetric for ProcessingStage {
    const LABEL: &'static str = "stage";
    const ALL_STAGES: &'static [Self] = &[
        Self::Started,
        Self::OrdersFetched,
        Self::Solved,
        Self::Verified,
        Self::Submitted,
        Self::Skipped,
    ];
}

enum BookType {
    Orderbook,
    Solution,
}

impl InitializeableMetric for BookType {
    const LABEL: &'static str = "type";
    const ALL_STAGES: &'static [Self] = &[Self::Orderbook, Self::Solution];
}

impl AsRef<str> for BookType {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::Orderbook => "orderbook",
            Self::Solution => "solution",
        }
    }
}

#[cfg(test)]
impl Default for StableXMetrics {
    fn default() -> Self {
        Self::new(Arc::new(Registry::new()))
    }
}
