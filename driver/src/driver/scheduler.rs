use super::stablex_driver::StableXDriver;
use anyhow::{anyhow, Context, Result};
use log::error;
use log::info;
use std::thread;
use std::time::{Duration, SystemTime, SystemTimeError};

const BATCH_DURATION: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct BatchId(pub u64);

impl BatchId {
    pub fn current(now: SystemTime) -> std::result::Result<Self, SystemTimeError> {
        let time_since_epoch = now.duration_since(SystemTime::UNIX_EPOCH)?;
        Ok(Self(time_since_epoch.as_secs() / BATCH_DURATION.as_secs()))
    }

    pub fn currently_being_solved(now: SystemTime) -> std::result::Result<Self, SystemTimeError> {
        Self::current(now).map(|batch_id| batch_id.prev())
    }

    pub fn order_collection_start_time(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0 * BATCH_DURATION.as_secs())
    }

    pub fn solve_start_time(self) -> SystemTime {
        self.order_collection_start_time() + BATCH_DURATION
    }

    pub fn next(self) -> BatchId {
        self.0.checked_add(1).map(BatchId).unwrap()
    }

    pub fn prev(self) -> BatchId {
        self.0.checked_sub(1).map(BatchId).unwrap()
    }
}

#[derive(Debug)]
pub struct AuctionTimingConfiguration {
    /// The offset from the start of a batch at which point we should start
    /// solving.
    pub target_start_solve_time: Duration,
    /// The offset from the start of the batch at which point there is not
    /// enough time left to attempt to solve.
    pub latest_solve_attempt_time: Duration,
}

pub struct Scheduler<'a> {
    driver: &'a mut dyn StableXDriver,
    auction_timing_configuration: AuctionTimingConfiguration,
}

#[derive(Debug, Eq, PartialEq)]
enum Action {
    NotEnoughTimeLeft,
    SolveImmediately,
    SolveAtTime(SystemTime),
}

impl<'a> Scheduler<'a> {
    pub fn new(
        driver: &'a mut dyn StableXDriver,
        auction_timing_configuration: AuctionTimingConfiguration,
    ) -> Self {
        assert!(auction_timing_configuration.latest_solve_attempt_time <= BATCH_DURATION);
        assert!(
            auction_timing_configuration.target_start_solve_time
                < auction_timing_configuration.latest_solve_attempt_time
        );
        Self {
            driver,
            auction_timing_configuration,
        }
    }

    pub fn run_forever(&mut self) {
        loop {
            let now = SystemTime::now();
            match self.determine_action(now) {
                Ok((batch_id, Action::NotEnoughTimeLeft)) => {
                    info!(
                        "Skipping batch {} because there is not enough time left.",
                        batch_id.0
                    );
                    self.wait_for_next_batch_id(batch_id);
                }
                Ok((batch_id, Action::SolveAtTime(target_time))) => {
                    // unwrap is ok because it would be a logic error in
                    // determine_action to return SolveAtTime with a time that
                    // is not actually in the future.
                    let duration = target_time.duration_since(now).unwrap();
                    info!(
                        "Waiting {}s to handle batch {}.",
                        duration.as_secs(),
                        batch_id.0
                    );
                    thread::sleep(duration);
                    self.run_solver(batch_id);
                    self.wait_for_next_batch_id(batch_id);
                }
                Ok((batch_id, Action::SolveImmediately)) => {
                    self.run_solver(batch_id);
                    self.wait_for_next_batch_id(batch_id);
                }
                Err(err) => {
                    error!("Scheduler error: {}", err);
                    thread::sleep(Duration::from_secs(10));
                }
            }
        }
    }

    fn run_solver(&mut self, batch_id: BatchId) {
        info!("Running solver for batch {}.", batch_id.0);
        if let Err(err) = self.driver.run(batch_id.0.into()) {
            error!("StableXDriver error: {}", err);
        }
    }

    /// Return current batch id and what to do with it.
    fn determine_action(&self, now: SystemTime) -> Result<(BatchId, Action)> {
        let solving_batch = BatchId::currently_being_solved(now)
            .with_context(|| anyhow!("failed to get batch id currently being solved"))?;
        let intended_solve_start_time = solving_batch.solve_start_time()
            + self.auction_timing_configuration.target_start_solve_time;
        // unwrap here because this cannot fail because the `solving_batch`'s
        // start time is always before `now`.
        let elapsed_time = now
            .duration_since(solving_batch.solve_start_time())
            .unwrap();

        Ok((
            solving_batch,
            if elapsed_time > self.auction_timing_configuration.latest_solve_attempt_time {
                Action::NotEnoughTimeLeft
            } else if now < intended_solve_start_time {
                Action::SolveAtTime(intended_solve_start_time)
            } else {
                Action::SolveImmediately
            },
        ))
    }

    fn wait_for_next_batch_id(&self, currently_being_solved: BatchId) {
        let next = currently_being_solved.next();
        let duration = (next.solve_start_time()
            + self.auction_timing_configuration.target_start_solve_time)
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::from_secs(1));
        info!(
            "Waiting {}s to handle batch {}.",
            duration.as_secs(),
            next.0
        );
        thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::super::stablex_driver::MockStableXDriver;
    use super::*;

    #[test]
    fn batch_id_current() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.order_collection_start_time(), start_time);
    }

    #[test]
    fn determine_action() {
        let mut driver = MockStableXDriver::new();
        let auction_timing_configuration = AuctionTimingConfiguration {
            target_start_solve_time: Duration::from_secs(10),
            latest_solve_attempt_time: Duration::from_secs(20),
        };
        let scheduler = Scheduler::new(&mut driver, auction_timing_configuration);

        let base_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);

        assert_eq!(
            scheduler.determine_action(base_time).unwrap(),
            (
                BatchId(0),
                Action::SolveAtTime(base_time + Duration::from_secs(10))
            )
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(9))
                .unwrap(),
            (
                BatchId(0),
                Action::SolveAtTime(base_time + Duration::from_secs(10))
            )
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(10))
                .unwrap(),
            (BatchId(0), Action::SolveImmediately)
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(19))
                .unwrap(),
            (BatchId(0), Action::SolveImmediately)
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(21))
                .unwrap(),
            (BatchId(0), Action::NotEnoughTimeLeft)
        );

        assert_eq!(
            scheduler
                .determine_action(base_time + Duration::from_secs(299))
                .unwrap(),
            (BatchId(0), Action::NotEnoughTimeLeft)
        );
    }
}
