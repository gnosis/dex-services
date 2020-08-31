//! Module containing command line helpers for e2e scripts.

use anyhow::{Error, Result};
use crossbeam::thread;
use pbr::ProgressBar;
use rayon::prelude::*;
use services_core::{history::ExchangeHistory, models::BatchId};
use std::{io, path::Path, sync::mpsc};

/// Runs a closure for each batch for the specified event history.
pub fn for_each_batch<R, F>(
    orderbook_file: impl AsRef<Path>,
    mut report: R,
    handler: F,
) -> Result<R::Summary>
where
    R: Reporting,
    F: Fn(&SampleChannel<R::Sample>, &ExchangeHistory, BatchId) -> Result<()> + Send + Sync,
{
    let history = ExchangeHistory::from_filestore(orderbook_file)?;

    let batches = history.batches_until_now();
    let mut progress = ProgressBar::on(io::stderr(), batches.batch_count());

    let (samples_tx, samples_rx) = mpsc::channel();
    let samples = SampleChannel(samples_tx);

    thread::scope(|scope| {
        scope.spawn(move |_| {
            if let Err(err) = batches
                .collect::<Vec<_>>()
                .into_par_iter()
                .try_for_each_with(samples.clone(), move |samples, batch| {
                    handler(&samples, &history, batch)?;
                    samples.batch_complete()
                })
            {
                samples.error(err);
            }
        });

        while let Ok(result) = samples_rx.recv() {
            match result? {
                Some(sample) => report.record_sample(sample)?,
                None => {
                    progress.inc();
                }
            }
        }

        report.finalize()
    })
    .expect("inner thread panicked when processing batches")
}

/// A channel used for sending samples to the main processing thread in order to
/// be recorded by the `Reporting` instance.
#[derive(Debug)]
pub struct SampleChannel<T>(mpsc::Sender<Result<Option<T>>>);

// NOTE: Manually implement clone, the derive unecessarily adds a `T: Clone`
// type bound.
impl<T> Clone for SampleChannel<T> {
    fn clone(&self) -> Self {
        SampleChannel(self.0.clone())
    }
}

impl<T> SampleChannel<T> {
    /// Sends a sample to the recording instance.
    pub fn record_sample(&self, sample: T) -> Result<()> {
        self.send(Some(sample))
    }

    /// Reports a batch was completed to increment the progress indicator.
    fn batch_complete(&self) -> Result<()> {
        self.send(None)
    }

    fn send(&self, value: Option<T>) -> Result<()> {
        // NOTE: Get rid of the `T` value since we don't care about recovering
        // it and allows us to remove the `T: Send + Sync + 'static` bounds.
        self.0.send(Ok(value)).map_err(|_| mpsc::SendError(()))?;
        Ok(())
    }

    /// Reports an error to the progress collecting thread.
    fn error(&self, err: Error) {
        // NOTE: If the channel is already closed, then we don't care about
        // propagating the error.
        let _ = self.0.send(Err(err));
    }
}

/// A trait for reporting samples collected from each batch run.
pub trait Reporting {
    /// The sample type.
    type Sample: Send;
    /// The summary type.
    type Summary;

    /// Record a sample.
    fn record_sample(&mut self, sample: Self::Sample) -> Result<()>;

    /// Finilize the recording of samples.
    fn finalize(self) -> Result<Self::Summary>;
}
