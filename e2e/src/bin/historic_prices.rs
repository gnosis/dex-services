use anyhow::{anyhow, Result};
use core::{
    history::{ExchangeHistory, Settlement},
    models::BatchId,
};
use pbr::ProgressBar;
use pricegraph::{Element, Pricegraph, TokenPair};
use std::{
    fs::File,
    io::{self, Write},
    path::PathBuf,
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "historic_prices",
    about = "Utility for computing historic exchange prices.",
    rename_all = "kebab"
)]
struct Options {
    /// The events registry file store containing past exchange events.
    #[structopt(long, env = "ORDERBOOK_FILE", parse(from_os_str))]
    orderbook_file: PathBuf,

    /// The output directory for the computed results.
    #[structopt(long, env = "OUTPUT_DIR", default_value = "target", parse(from_os_str))]
    output_dir: PathBuf,
}

fn main() -> Result<()> {
    let options = Options::from_args();
    let history = ExchangeHistory::from_filestore(&options.orderbook_file)?;

    let mut spreads = File::create(&options.output_dir.join("spreads.csv"))?;
    writeln!(&mut spreads, "market,best_bid,price,best_ask")?;

    let first_batch = history
        .first_batch()
        .ok_or_else(|| anyhow!("exchange has no events"))?;
    let (count, batches) = batches_until_now_from(first_batch);

    let mut progress = ProgressBar::on(io::stderr(), count);
    for batch in batches {
        progress.inc();

        let settlement = if let Some(value) = history.settlement_for_batch(batch) {
            value
        } else {
            continue;
        };
        let auction_elements = history.auction_elements_for_batch(batch)?;

        check_batch_spread(&auction_elements, &settlement, &mut spreads)?;
    }

    Ok(())
}

fn check_batch_spread(
    auction_elements: &[Element],
    settlement: &Settlement,
    mut output: impl Write,
) -> Result<()> {
    let token_ids = &settlement.solution.token_ids_for_price;
    let prices = &settlement.solution.prices;
    debug_assert_eq!(
        token_ids.len(),
        prices.len(),
        "invalid solution price mapping",
    );

    let pricegraph = Pricegraph::new(auction_elements.iter().cloned());
    for (i, j) in unique_pairs(token_ids.len()) {
        let (buy_token, sell_token) = (token_ids[i], token_ids[j]);

        let best_bid = pricegraph
            .estimate_exchange_rate(
                TokenPair {
                    buy: sell_token,
                    sell: buy_token,
                },
                0.0,
            )
            .unwrap_or(0.0);
        let price = prices[i] as f64 / prices[j] as f64;
        let best_ask = pricegraph
            .estimate_exchange_rate(
                TokenPair {
                    buy: buy_token,
                    sell: sell_token,
                },
                0.0,
            )
            .map(|xrate| 1.0 / xrate)
            .unwrap_or(f64::INFINITY);

        writeln!(
            &mut output,
            "{}-{},{},{},{}",
            buy_token, sell_token, best_bid, price, best_ask
        )?;
    }

    Ok(())
}

fn batches_until_now_from(starting_batch: BatchId) -> (u64, impl Iterator<Item = BatchId>) {
    let current_batch = BatchId::now();
    (
        current_batch.0.saturating_sub(starting_batch.0),
        (starting_batch.0..current_batch.0).map(BatchId),
    )
}

fn unique_pairs(len: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..len - 1).flat_map(move |i| (i + 1..len).map(move |j| (i, j)))
}
