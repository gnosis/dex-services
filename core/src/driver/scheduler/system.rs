use super::{AuctionTimingConfiguration, Scheduler};
use crate::driver::stablex_driver::{DriverResult, StableXDriver};
use crate::models::BatchId;
use crate::util::FutureWaitExt as _;
use anyhow::{Context, Result};
use crossbeam_utils::thread::Scope;
use log::error;
use log::info;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

const RETRY_SLEEP_DURATION: Duration = Duration::from_secs(1);

pub struct SystemScheduler<'a> {
    driver: &'a (dyn StableXDriver + Sync),
    auction_timing_configuration: AuctionTimingConfiguration,
    last_solved_batch: Option<BatchId>,
}

#[derive(Debug, Eq, PartialEq)]
enum Action {
    Solve(BatchId, Duration),
    Sleep(Duration),
}

impl<'a> SystemScheduler<'a> {
    pub fn new(
        driver: &'a (dyn StableXDriver + Sync),
        auction_timing_configuration: AuctionTimingConfiguration,
    ) -> Self {
        Self {
            driver,
            auction_timing_configuration,
            last_solved_batch: None,
        }
    }

    fn start_solving_in_thread<'b>(
        &self,
        batch_id: BatchId,
        solver_deadline: Instant,
        scope: &Scope<'b>,
    ) where
        'a: 'b,
    {
        let driver = self.driver;
        scope.spawn(move |_| {
            while let Some(time_limit) = solver_deadline.checked_duration_since(Instant::now()) {
                let driver_result = driver.run(batch_id.into(), time_limit).wait();
                log_driver_result(batch_id, &driver_result);
                match driver_result {
                    DriverResult::Retry(_) => thread::sleep(RETRY_SLEEP_DURATION),
                    DriverResult::Ok | DriverResult::Skip(_) => break,
                }
            }
        });
    }

    /// Return current batch id and what to do with it.
    fn determine_action(&self, now: SystemTime) -> Result<Action> {
        let solving_batch = BatchId::currently_being_solved(now)
            .context("failed to get batch id currently being solved")?;
        let intended_solve_start_time = solving_batch.solve_start_time()
            + self.auction_timing_configuration.target_start_solve_time;
        // unwrap here because this cannot fail because the `solving_batch`'s
        // start time is always before `now`.
        let elapsed_time = now
            .duration_since(solving_batch.solve_start_time())
            .unwrap();

        let action = if self.last_solved_batch == Some(solving_batch)
            || elapsed_time >= self.auction_timing_configuration.solver_time_limit
        {
            let next = solving_batch.next();
            let duration = (next.solve_start_time()
                + self.auction_timing_configuration.target_start_solve_time)
                .duration_since(now)
                .unwrap();
            Action::Sleep(duration)
        } else if now < intended_solve_start_time {
            let duration = intended_solve_start_time.duration_since(now).unwrap();
            Action::Sleep(duration)
        } else {
            let time_limit = self.auction_timing_configuration.solver_time_limit - elapsed_time;
            Action::Solve(solving_batch, time_limit)
        };

        Ok(action)
    }
}

fn log_driver_result(batch_id: BatchId, driver_result: &DriverResult) {
    match driver_result {
        DriverResult::Ok => info!("Batch {} solved successfully.", batch_id),
        DriverResult::Retry(err) => {
            error!("Batch {} failed with retryable error: {:?}", batch_id, err)
        }
        DriverResult::Skip(err) => error!(
            "Batch {} failed with unretryable error: {:?}",
            batch_id, err
        ),
    }
}

impl<'a> Scheduler for SystemScheduler<'a> {
    fn start(&mut self) -> ! {
        crossbeam_utils::thread::scope(|scope| -> ! {
            loop {
                match self.determine_action(SystemTime::now()) {
                    Ok(Action::Sleep(duration)) => {
                        info!("Sleeping {}s.", duration.as_secs());
                        thread::sleep(duration);
                    }
                    Ok(Action::Solve(batch_id, duration)) => {
                        info!("Starting to solve batch {}.", batch_id);
                        self.last_solved_batch = Some(batch_id);
                        self.start_solving_in_thread(batch_id, Instant::now() + duration, scope)
                    }
                    Err(err) => {
                        error!("Scheduler error: {:?}", err);
                        thread::sleep(RETRY_SLEEP_DURATION);
                    }
                };
            }
        })
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::stablex_driver::MockStableXDriver;
    use anyhow::anyhow;
    use futures::future::FutureExt as _;

    #[test]
    fn determine_action_without_matching_last_solved_batch() {
        let driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            scheduler.determine_action(base_time).unwrap(),
            Action::Sleep(Duration::from_secs(10))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(9))
                .unwrap(),
            Action::Sleep(Duration::from_secs(1))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(10))
                .unwrap(),
            Action::Solve(BatchId(0), Duration::from_secs(10))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(19))
                .unwrap(),
            Action::Solve(BatchId(0), Duration::from_secs(1))
        );

        // Sleep because we are behind latest_solve_attempt_time:

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(20))
                .unwrap(),
            Action::Sleep(Duration::from_secs(290))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(299))
                .unwrap(),
            Action::Sleep(Duration::from_secs(11))
        );
    }

    #[test]
    fn determine_action_with_matching_last_solved_batch() {
        let driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);
        scheduler.last_solved_batch = Some(BatchId(0));

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            scheduler.determine_action(base_time).unwrap(),
            Action::Sleep(Duration::from_secs(310))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(9))
                .unwrap(),
            Action::Sleep(Duration::from_secs(301))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(10))
                .unwrap(),
            Action::Sleep(Duration::from_secs(300))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(19))
                .unwrap(),
            Action::Sleep(Duration::from_secs(291))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(21))
                .unwrap(),
            Action::Sleep(Duration::from_secs(289))
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(299))
                .unwrap(),
            Action::Sleep(Duration::from_secs(11))
        );
    }

    // Allows a human to observe real behavior by looking at the log output.
    // You should see the log messages from `impl Scheduler for SystemScheduler`
    // and from `log_driver_result`.
    // To test different cases change `expect_run`.
    #[test]
    #[ignore]
    fn test_real() {
        use crate::driver::stablex_driver::DriverResult;
        let (_, _guard) = crate::logging::init("info");

        let mut driver = MockStableXDriver::new();

        let mut counter = 0;
        driver.expect_run().returning(move |batch, time_limit| {
            log::info!(
                "driver run called for the {}. time with batch {} time_limit {}",
                counter,
                batch,
                time_limit.as_secs(),
            );
            counter += 1;
            async move {
                match counter % 3 {
                    0 => DriverResult::Ok,
                    1 => DriverResult::Retry(anyhow!("")),
                    2 => DriverResult::Skip(anyhow!("")),
                    _ => unreachable!(),
                }
            }
            .boxed()
        });

        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            solver_time_limit: Duration::from_secs(20),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        scheduler.start();
    }
}
