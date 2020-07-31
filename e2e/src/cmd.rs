//! Module containing command line helpers for e2e scripts.

use anyhow::Result;
use core::{history::ExchangeHistory, models::BatchId};
use pbr::ProgressBar;
use std::{io, path::Path};

/// Runs a closure for each batch for the specified event history.
pub fn for_each_batch(
    orderbook_file: impl AsRef<Path>,
    mut f: impl FnMut(&ExchangeHistory, BatchId) -> Result<()>,
) -> Result<()> {
    let history = ExchangeHistory::from_filestore(orderbook_file)?;

    let batches = history.batches_until_now();
    let mut progress = ProgressBar::on(io::stderr(), batches.batch_count());

    for batch in batches {
        progress.inc();
        f(&history, batch)?;
    }

    Ok(())
}
