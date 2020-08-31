//! Module containing iterator implementation for batch IDs.

use crate::models::BatchId;
use std::{iter::FusedIterator, ops::RangeInclusive};

pub struct Batches {
    range: RangeInclusive<u64>,
}

impl Batches {
    /// Creates a new batch iterator from a starting batch until the current
    /// batch determined from system time.
    pub fn from_batch(start: BatchId) -> Self {
        Batches {
            range: start.0..=BatchId::now().0,
        }
    }

    /// Creates an empty batch iterator.
    pub fn empty() -> Self {
        #[allow(clippy::reversed_empty_ranges)]
        Batches { range: 1..=0 }
    }

    /// Returns the number of batches in this range.
    pub fn batch_count(&self) -> u64 {
        self.range.end().saturating_sub(*self.range.start())
    }
}

impl Iterator for Batches {
    type Item = BatchId;

    fn next(&mut self) -> Option<Self::Item> {
        self.range.next().map(BatchId)
    }
}

impl DoubleEndedIterator for Batches {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.range.next_back().map(BatchId)
    }
}

impl ExactSizeIterator for Batches {}

impl FusedIterator for Batches {}
