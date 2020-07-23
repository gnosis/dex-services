use anyhow::Result;
use core::{history::Settlement, models::BatchId};
use e2e::cmd;
use pricegraph::{Element, Pricegraph};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

/// Common options for analyzing historic batch data.
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

    let mut spreads = File::create(&options.output_dir.join("spreads.csv"))?;
    writeln!(&mut spreads, "batch,token,price,estimate")?;

    cmd::for_each_batch(&options.orderbook_file, |history, batch| {
        let settlement = match history.settlement_for_batch(batch) {
            Some(value) => value,
            None => return Ok(()),
        };

        let auction_elements = history.auction_elements_for_batch(batch)?;
        check_batch_prices(batch, &auction_elements, &settlement, &mut spreads)?;

        Ok(())
    })
}

fn check_batch_prices(
    batch: BatchId,
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
    for (token_index, token) in token_ids
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, token)| *token != 0)
    {
        let price = prices[token_index];
        let estimate = pricegraph.estimate_token_price(token).unwrap_or(0.0) as u128;

        writeln!(&mut output, "{},{},{},{}", batch, token, price, estimate)?;
    }

    Ok(())
}
