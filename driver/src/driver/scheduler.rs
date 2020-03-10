use super::stablex_driver::StableXDriver;
use crate::metrics::StableXMetrics;
use crate::orderbook::StableXOrderBookReading;
use anyhow::{anyhow, Context, Result};
use log::error;
use log::info;
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant, SystemTime, SystemTimeError};

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

    pub fn start_time(self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(self.0 * BATCH_DURATION.as_secs())
    }

    pub fn solve_start_time(self) -> SystemTime {
        self.start_time() + BATCH_DURATION
    }

    pub fn next(self) -> BatchId {
        self.0.checked_add(1).map(BatchId).unwrap()
    }

    pub fn prev(self) -> BatchId {
        self.0.checked_sub(1).map(BatchId).unwrap()
    }
}

pub struct Scheduler<'a> {
    driver: &'a mut dyn StableXDriver,
    orderbook_reader: &'a dyn StableXOrderBookReading,
    metrics: &'a StableXMetrics,
    batch_wait_time: Duration,
    max_batch_elapsed_time: Duration,
    past_auctions: HashSet<BatchId>,
}

#[derive(Debug, Eq, PartialEq)]
enum Action {
    AlreadyHandled,
    NotEnoughTimeLeft,
    Solve,
}

impl<'a> Scheduler<'a> {
    pub fn new(
        driver: &'a mut dyn StableXDriver,
        orderbook_reader: &'a dyn StableXOrderBookReading,
        metrics: &'a StableXMetrics,
        batch_wait_time: Duration,
        max_batch_elapsed_time: Duration,
    ) -> Self {
        Self {
            driver,
            orderbook_reader,
            metrics,
            batch_wait_time,
            max_batch_elapsed_time,
            past_auctions: HashSet::new(),
        }
    }

    pub fn run_forever(&mut self) {
        loop {
            let now = SystemTime::now();
            match self.determine_action(now) {
                Ok((batch_id, Action::AlreadyHandled)) => {
                    info!(
                        "Skipping batch {} because it has already been handled.",
                        batch_id.0
                    );
                    self.wait_for_next_batch_id(batch_id);
                }
                Ok((batch_id, Action::NotEnoughTimeLeft)) => {
                    info!(
                        "Skipping batch {} because there is not enough time left.",
                        batch_id.0
                    );
                    self.metrics.auction_skipped(batch_id.0.into());
                    self.wait_for_next_batch_id(batch_id);
                }
                Ok((batch_id, Action::Solve)) => {
                    let deadline = batch_id.start_time() + self.batch_wait_time;
                    if let Ok(duration) = deadline.duration_since(now) {
                        info!(
                            "Waiting {}s to handle batch {}.",
                            duration.as_secs(),
                            batch_id.0
                        );
                        self.wait_for_batch_id(batch_id, Instant::now() + duration);
                    }
                    info!("Running solver for batch {}.", batch_id.0);
                    if let Err(err) = self.driver.run(batch_id.0.into()) {
                        error!("StableXDriver error: {}", err);
                    }
                    self.past_auctions.insert(batch_id);
                    self.wait_for_next_batch_id(batch_id);
                }
                Err(err) => {
                    error!("Scheduler error: {}", err);
                    thread::sleep(Duration::from_secs(10));
                }
            }
        }
    }

    /// Return current batch id and what to do with it.
    fn determine_action(&self, now: SystemTime) -> Result<(BatchId, Action)> {
        let solving_batch = BatchId::currently_being_solved(now)
            .with_context(|| anyhow!("failed to get batch id currently being solved"))?;
        // unwrap here because this cannot fail because the `solving_batch`'s
        // start time is always before `now`.
        let elapsed_time = now.duration_since(solving_batch.start_time()).unwrap() - BATCH_DURATION;

        Ok((
            solving_batch,
            if self.past_auctions.contains(&solving_batch) {
                Action::AlreadyHandled
            } else if elapsed_time > self.max_batch_elapsed_time {
                Action::NotEnoughTimeLeft
            } else {
                Action::Solve
            },
        ))
    }

    /// Wait until the batch id matches the batch id according to the order book
    /// but at most until deadline.
    fn wait_for_batch_id(&self, batch_id: BatchId, deadline: Instant) {
        info!("Waiting for the next batch ({}) to begin.", batch_id.0);
        let sleep_interval = Duration::from_secs(5);
        loop {
            if let Ok(index) = self.orderbook_reader.get_auction_index() {
                if index.low_u64() == batch_id.0 {
                    break;
                }
            }
            let remaining = deadline - Instant::now();
            if remaining <= sleep_interval {
                thread::sleep(remaining);
                break;
            }
            thread::sleep(sleep_interval);
        }
    }

    fn wait_for_next_batch_id(&self, currently_being_solved: BatchId) {
        let mut duration = Duration::from_secs(1);
        if let Ok(duration_) = currently_being_solved
            .next()
            .solve_start_time()
            .duration_since(SystemTime::now())
        {
            duration = duration_;
        }
        thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::super::stablex_driver::MockStableXDriver;
    use super::*;
    use crate::orderbook::MockStableXOrderBookReading;

    #[test]
    fn batch_id_current() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 0);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.start_time(), start_time);

        let start_time = SystemTime::UNIX_EPOCH + Duration::from_secs(300);
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 1);
        assert_eq!(batch_id.start_time(), start_time);
    }

    #[test]
    fn determine_action() {
        let mut driver = MockStableXDriver::new();
        let orderbook_reader = MockStableXOrderBookReading::new();
        let metrics = StableXMetrics::default();
        let batch_wait_time = Duration::from_secs(10);
        let max_batch_elapsed_time = Duration::from_secs(100);
        let mut scheduler = Scheduler::new(
            &mut driver,
            &orderbook_reader,
            &metrics,
            batch_wait_time,
            max_batch_elapsed_time,
        );

        assert_eq!(
            scheduler
                .determine_action(SystemTime::UNIX_EPOCH + Duration::from_secs(305))
                .unwrap(),
            (BatchId(0), Action::Solve)
        );

        assert_eq!(
            scheduler
                .determine_action(SystemTime::UNIX_EPOCH + Duration::from_secs(605))
                .unwrap(),
            (BatchId(1), Action::Solve)
        );

        assert_eq!(
            scheduler
                .determine_action(SystemTime::UNIX_EPOCH + Duration::from_secs(550))
                .unwrap(),
            (BatchId(0), Action::NotEnoughTimeLeft)
        );

        scheduler.past_auctions.insert(BatchId(0));
        assert_eq!(
            scheduler
                .determine_action(SystemTime::UNIX_EPOCH + Duration::from_secs(305))
                .unwrap(),
            (BatchId(0), Action::AlreadyHandled)
        );
    }
}
