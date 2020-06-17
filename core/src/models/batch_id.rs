use std::fmt;
use std::time::{Duration, SystemTime, SystemTimeError};

/// The total time in a batch.
pub const BATCH_DURATION: Duration = Duration::from_secs(300);

/// Wraps a batch id as in the smart contract to add functionality related to
/// the current time.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BatchId(pub u64);

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

    fn prev(self) -> BatchId {
        self.0.checked_sub(1).map(BatchId).unwrap()
    }
}

impl Into<u32> for BatchId {
    fn into(self) -> u32 {
        self.0 as u32
    }
}

impl fmt::Display for BatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_batch_at_unix_epoch_is_zero() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.0, 0);
    }

    #[test]
    fn current_batch_at_unix_epoch_has_next_batch_one() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.next().0, 1);
    }

    #[test]
    #[should_panic]
    fn current_batch_at_unix_epoch_panics_on_previous() {
        let start_time = SystemTime::UNIX_EPOCH;
        BatchId::current(start_time).unwrap().prev();
    }

    #[test]
    fn current_batch_at_unix_epoch_plus_300_has_previous_batch_zero() {
        let batch_id = BatchId::current(SystemTime::UNIX_EPOCH + Duration::from_secs(300)).unwrap();
        assert_eq!(batch_id.prev().0, 0);
    }

    #[test]
    #[should_panic]
    #[allow(unused_must_use)]
    fn currently_being_solved_at_unix_epoch_panics() {
        let start_time = SystemTime::UNIX_EPOCH;
        BatchId::currently_being_solved(start_time);
    }

    #[test]
    fn currently_being_solved_batch_at_unix_epoch_plus_300_is_zero() {
        let batch_id =
            BatchId::currently_being_solved(SystemTime::UNIX_EPOCH + Duration::from_secs(300))
                .unwrap();
        assert_eq!(batch_id.0, 0);
    }

    #[test]
    fn batch_id_increases_every_300_seconds() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.0, 0);
        let batch_id = BatchId::current(start_time + Duration::from_secs(300)).unwrap();
        assert_eq!(batch_id.0, 1);

        let batch_id = BatchId::current(start_time + Duration::from_secs(599)).unwrap();
        assert_eq!(batch_id.0, 1);
    }

    #[test]
    fn order_collection_start_time_is_first_second_of_batch() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(batch_id.order_collection_start_time(), start_time);

        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(batch_id.order_collection_start_time(), start_time);
    }

    #[test]
    fn solve_start_time_is_order_collection_start_time() {
        let start_time = SystemTime::UNIX_EPOCH;
        let batch_id = BatchId::current(start_time).unwrap();
        assert_eq!(
            batch_id.solve_start_time(),
            batch_id.next().order_collection_start_time()
        );

        let batch_id = BatchId::current(start_time + Duration::from_secs(299)).unwrap();
        assert_eq!(
            batch_id.solve_start_time(),
            batch_id.next().order_collection_start_time()
        );
    }
}
