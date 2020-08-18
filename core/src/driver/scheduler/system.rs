use super::{AuctionTimingConfiguration, Scheduler};
use crate::{
    driver::stablex_driver::{DriverError, StableXDriver},
    models::{BatchId, Solution},
    util::{self, AsyncSleep, AsyncSleeping, FutureWaitExt as _, Now},
};
use anyhow::{Context, Result};
use crossbeam_utils::thread::Scope;
use std::{
    thread,
    time::{Duration, Instant, SystemTime},
};

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
        let earliest_solution_submit_time = self
            .auction_timing_configuration
            .earliest_solution_submit_time;
        scope.spawn(move |_| {
            solve_and_submit(
                batch_id,
                solver_deadline,
                earliest_solution_submit_time,
                driver,
                &util::default_now(),
                &AsyncSleep {},
            )
            .wait();
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
            || elapsed_time
                >= self
                    .auction_timing_configuration
                    .latest_solution_submit_time
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
            let time_limit = self
                .auction_timing_configuration
                .latest_solution_submit_time
                - elapsed_time;
            Action::Solve(solving_batch, time_limit)
        };

        Ok(action)
    }
}

async fn solve_and_submit(
    batch_id: BatchId,
    solver_deadline: Instant,
    earliest_solution_submit_time: Duration,
    driver: &dyn StableXDriver,
    now: &dyn Now,
    sleep: &dyn AsyncSleeping,
) {
    while let Some(time_limit) = solver_deadline.checked_duration_since(now.instant_now()) {
        let driver_result = driver.solve_batch(batch_id, time_limit).await;
        log_solve_result(batch_id, &driver_result);
        match driver_result {
            Ok(solution) => {
                return submit(
                    batch_id,
                    earliest_solution_submit_time,
                    solution,
                    driver,
                    now,
                    sleep,
                )
                .await
            }
            Err(DriverError::Retry(_)) => sleep.sleep(RETRY_SLEEP_DURATION).await,
            Err(DriverError::Skip(_)) => break,
        }
    }
}

async fn submit(
    batch_id: BatchId,
    earliest_solution_submit_time: Duration,
    solution: Solution,
    driver: &dyn StableXDriver,
    now: &dyn Now,
    sleep: &dyn AsyncSleeping,
) {
    if let Ok(duration) = (batch_id.solve_start_time() + earliest_solution_submit_time)
        .duration_since(now.system_now())
    {
        log::info!(
            "Sleeping {} seconds to wait for earliest_solution_submit_time.",
            duration.as_secs()
        );
        sleep.sleep(duration).await;
    }
    let result = driver.submit_solution(batch_id, solution).wait();
    log_submit_result(batch_id, &result);
}

fn log_solve_result(batch_id: BatchId, driver_result: &Result<Solution, DriverError>) {
    match driver_result {
        Ok(_) => log::info!("Batch {} solved successfully.", batch_id),
        Err(DriverError::Retry(err)) => {
            log::error!("Batch {} failed with retryable error: {:?}", batch_id, err)
        }
        Err(DriverError::Skip(err)) => log::error!(
            "Batch {} failed with unretryable error: {:?}",
            batch_id,
            err
        ),
    }
}

fn log_submit_result(batch_id: BatchId, result: &Result<()>) {
    match result {
        Ok(_) => log::info!("Batch {} solution submitted successfully.", batch_id),
        Err(err) => log::error!("Batch {} solution submission failed: {:?}", batch_id, err),
    }
}

impl<'a> Scheduler for SystemScheduler<'a> {
    fn start(&mut self) -> ! {
        crossbeam_utils::thread::scope(|scope| -> ! {
            loop {
                match self.determine_action(SystemTime::now()) {
                    Ok(Action::Sleep(duration)) => {
                        log::info!("Sleeping {}s.", duration.as_secs());
                        thread::sleep(duration);
                    }
                    Ok(Action::Solve(batch_id, duration)) => {
                        log::info!("Starting to solve batch {}.", batch_id);
                        self.last_solved_batch = Some(batch_id);
                        self.start_solving_in_thread(batch_id, Instant::now() + duration, scope)
                    }
                    Err(err) => {
                        log::error!("Scheduler error: {:?}", err);
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
    use crate::{
        driver::stablex_driver::MockStableXDriver,
        util::{MockAsyncSleeping, MockNow},
    };
    use anyhow::anyhow;
    use futures::future::FutureExt as _;
    use mockall::{predicate::*, Sequence};

    #[test]
    fn determine_action_without_matching_last_solved_batch() {
        let driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            latest_solution_submit_time: Duration::from_secs(20),
            earliest_solution_submit_time: Duration::from_secs(0),
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
            latest_solution_submit_time: Duration::from_secs(20),
            earliest_solution_submit_time: Duration::from_secs(0),
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

    #[test]
    fn solve_checks_deadline() {
        lazy_static::lazy_static! {
            static ref EPOCH: Instant = Instant::now();
        };

        let mut driver = MockStableXDriver::new();
        driver
            .expect_solve_batch()
            .returning(|_, _| immediate!(Err(DriverError::Retry(anyhow!("")))));
        let mut sleep = MockAsyncSleeping::new();
        sleep.expect_sleep().returning(|_| immediate!(()));

        let mut now = MockNow::new();
        now.expect_instant_now().times(1).returning(|| *EPOCH);
        now.expect_instant_now()
            .times(1)
            .returning(|| *EPOCH + Duration::from_secs(2));
        now.expect_instant_now()
            .times(1)
            .returning(|| *EPOCH + Duration::from_secs(4));
        now.expect_instant_now()
            .times(1)
            .returning(|| *EPOCH + Duration::from_secs(6));

        assert!(solve_and_submit(
            BatchId(0),
            *EPOCH + Duration::from_secs(5),
            Duration::from_secs(0),
            &driver,
            &now,
            &sleep,
        )
        .now_or_never()
        .is_some());
    }

    #[test]
    fn submit_waits_for_earliest_time() {
        let mut sequence = Sequence::new();
        let mut driver = MockStableXDriver::new();
        let mut now = MockNow::new();
        let mut sleep = MockAsyncSleeping::new();

        now.expect_system_now()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|| std::time::UNIX_EPOCH + Duration::from_secs(300));
        sleep
            .expect_sleep()
            .times(1)
            .in_sequence(&mut sequence)
            .with(eq(Duration::from_secs(5)))
            .returning(|_| immediate!(()));
        driver
            .expect_submit_solution()
            .times(1)
            .in_sequence(&mut sequence)
            .returning(|_, _| immediate!(Ok(())));

        assert!(submit(
            BatchId(0),
            Duration::from_secs(5),
            Solution::trivial(),
            &driver,
            &now,
            &sleep,
        )
        .now_or_never()
        .is_some());
    }

    // Allows a human to observe real behavior by looking at the log output.
    // You should see the log messages from `impl Scheduler for SystemScheduler`
    // and from `log_driver_result`.
    // To test different cases change `expect_run`.
    #[test]
    #[ignore]
    fn test_real() {
        let (_, _guard) = crate::logging::init("info");

        let mut driver = MockStableXDriver::new();

        let mut counter = 0;
        driver
            .expect_solve_batch()
            .returning(move |batch, time_limit| {
                log::info!(
                    "driver solve batch called for the {}. time with batch {} time_limit {}",
                    counter,
                    batch,
                    time_limit.as_secs(),
                );
                counter += 1;
                immediate!(match counter % 3 {
                    0 => Ok(Solution::trivial()),
                    1 => Err(DriverError::Retry(anyhow!(""))),
                    2 => Err(DriverError::Skip(anyhow!(""))),
                    _ => unreachable!(),
                })
            });
        driver.expect_submit_solution().returning(|batch, _| {
            log::info!("driver submit solution called for batch {}", batch);
            immediate!(Ok(()))
        });

        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            latest_solution_submit_time: Duration::from_secs(20),
            earliest_solution_submit_time: Duration::from_secs(0),
        };
        let mut scheduler = SystemScheduler::new(&driver, auction_timing_configuration);

        scheduler.start();
    }
}
