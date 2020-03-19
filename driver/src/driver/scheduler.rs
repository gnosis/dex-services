mod evm;
mod system;

use self::evm::EvmScheduler;
use self::system::SystemScheduler;
use crate::contracts::stablex_contract::StableXContract;
use crate::driver::stablex_driver::StableXDriver;
use anyhow::{anyhow, Error, Result};
use std::str::FromStr;
use std::time::Duration;

/// The total time in a batch.
const BATCH_DURATION: Duration = Duration::from_secs(300);

/// The time in a batch where a solution may be submitted.
const SOLVING_WINDOW: Duration = Duration::from_secs(240);

/// A scheduler that can be started in order to run the driver for each batch.
pub trait Scheduler {
    /// Start the scheduler. This method never returns.
    fn start(&mut self) -> !;
}

#[derive(Debug)]
pub struct AuctionTimingConfiguration {
    /// The offset from the start of a batch at which point we should start
    /// solving.
    target_start_solve_time: Duration,

    /// The offset from the start of the batch to cap the solver's execution
    /// time.
    solver_time_limit: Duration,
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
    ///
    /// Where `SOLVING_WINDOW` represents the amount of time within a batch in
    /// which a solution is accepted. There is an amount of time at the end of a
    /// batch where solutions are no longer accepted, this is done to allow
    /// traders time to make decisions after the previous batch has already
    /// finalized.
    pub fn new(target_start_solve_time: Duration, solver_time_limit: Duration) -> Self {
        assert!(
            solver_time_limit < SOLVING_WINDOW,
            "The solver time limit must be within the solving window",
        );
        assert!(
            target_start_solve_time < solver_time_limit,
            "the target solve start time must be earlier than the solver time limit",
        );

        AuctionTimingConfiguration {
            target_start_solve_time,
            solver_time_limit,
        }
    }
}

impl Default for AuctionTimingConfiguration {
    fn default() -> Self {
        AuctionTimingConfiguration::new(Duration::from_secs(30), Duration::from_secs(180))
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
    pub fn create<'a>(
        &self,
        exchange: &'a dyn StableXContract,
        driver: &'a (dyn StableXDriver + Sync),
        config: AuctionTimingConfiguration,
    ) -> Box<dyn Scheduler + 'a> {
        match self {
            SchedulerKind::System => Box::new(SystemScheduler::new(driver, config)),
            SchedulerKind::Evm => Box::new(EvmScheduler::new(exchange, driver, config)),
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
