mod evm;
mod system;

use self::{evm::EvmScheduler, system::SystemScheduler};
use crate::{
    contracts::stablex_contract::StableXContract, driver::stablex_driver::StableXDriver,
    health::HealthReporting, models::batch_id::SOLVING_WINDOW,
};
use anyhow::{anyhow, Error, Result};
use std::{str::FromStr, sync::Arc, time::Duration};

/// A scheduler that can be started in order to run the driver for each batch.
pub trait Scheduler {
    /// Start the scheduler. This method never returns.
    fn start(&mut self) -> !;
}

#[derive(Clone, Copy, Debug)]
pub struct AuctionTimingConfiguration {
    /// The offset from the start of a batch at which point we should start
    /// solving.
    target_start_solve_time: Duration,

    /// The offset from the start of the batch to cap the solver's execution
    /// time.
    latest_solution_submit_time: Duration,

    /// The earliest offset from the start of a batch in seconds at which point we should submit the
    /// solution.
    earliest_solution_submit_time: Duration,
}

impl AuctionTimingConfiguration {
    /// Creates a new timing configuration for a scheduler.
    ///
    /// # Panics
    ///
    /// Panics if the configuration is invalid. Specifically the following
    /// invariants must hold:
    /// - `target_start_solve_time < solver_time_limit`
    /// - `solver_time_limit < SOLVING_WINDOW`
    /// - `min_solution_submit_time < SOLVING_WINDOW`
    ///
    /// Where `SOLVING_WINDOW` represents the amount of time within a batch in
    /// which a solution is accepted. There is an amount of time at the end of a
    /// batch where solutions are no longer accepted, this is done to allow
    /// traders time to make decisions after the previous batch has already
    /// finalized.
    pub fn new(
        target_start_solve_time: Duration,
        solver_time_limit: Duration,
        min_solution_submit_time: Duration,
    ) -> Self {
        assert!(
            solver_time_limit < SOLVING_WINDOW,
            "The solver time limit must be within the solving window",
        );
        assert!(
            target_start_solve_time < solver_time_limit,
            "the target solve start time must be earlier than the solver time limit",
        );
        assert!(
            min_solution_submit_time < SOLVING_WINDOW,
            "The min solution submit time must be within the solving window",
        );

        AuctionTimingConfiguration {
            target_start_solve_time,
            latest_solution_submit_time: solver_time_limit,
            earliest_solution_submit_time: min_solution_submit_time,
        }
    }
}

impl Default for AuctionTimingConfiguration {
    fn default() -> Self {
        AuctionTimingConfiguration::new(
            Duration::from_secs(30),
            Duration::from_secs(180),
            Duration::from_secs(0),
        )
    }
}

/// The different kinds of schedulers.
#[derive(Debug)]
pub enum SchedulerKind {
    /// A system based scheduler that uses system time to run the driver.
    System,
    /// An EVM based scheduler that queries block-chain state to run the driver.
    Evm,
}

impl SchedulerKind {
    /// Creates a new scheduler based on the parameters.
    pub fn create(
        &self,
        exchange: Arc<dyn StableXContract>,
        driver: Arc<dyn StableXDriver>,
        config: AuctionTimingConfiguration,
        health: Arc<dyn HealthReporting>,
    ) -> Box<dyn Scheduler> {
        match self {
            SchedulerKind::System => {
                Box::new(SystemScheduler::new(exchange, driver, health, config))
            }
            SchedulerKind::Evm => Box::new(EvmScheduler::new(exchange, driver, health, config)),
        }
    }
}

impl FromStr for SchedulerKind {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "system" => Ok(SchedulerKind::System),
            "evm" => Ok(SchedulerKind::Evm),
            _ => Err(anyhow!("unknown scheduler kind '{}'", value)),
        }
    }
}
