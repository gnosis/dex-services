use anyhow::Result;
use core::{history::Settlement, models::BatchId};
use e2e::cmd;
use pricegraph::{Element, Pricegraph, TokenPair};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "historic_prices",
    about = "Utility for computing historic exchange token prices.",
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
    writeln!(&mut spreads, "batch,market,best_bid,price,best_ask")?;

    cmd::for_each_batch(&options.orderbook_file, |history, batch| {
        let settlement = match history.settlement_for_batch(batch) {
            Some(value) => value,
            None => return Ok(()),
        };

        let auction_elements = history.auction_elements_for_batch(batch)?;
        check_batch_spread(batch, &auction_elements, &settlement, &mut spreads)?;

        Ok(())
    })
}

fn check_batch_spread(
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
    for (i, j) in unique_pairs(token_ids.len()) {
        let (buy_token, sell_token) = (token_ids[i], token_ids[j]);

        let best_bid = pricegraph
            .estimate_limit_price(
                TokenPair {
                    buy: sell_token,
                    sell: buy_token,
                },
                0.0,
            )
            .unwrap_or(0.0);
        let price = prices[i] as f64 / prices[j] as f64;
        let best_ask = pricegraph
            .estimate_limit_price(
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
            "{},{}-{},{},{},{}",
            batch, buy_token, sell_token, best_bid, price, best_ask
        )?;
    }

    Ok(())
}

fn unique_pairs(len: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..len - 1).flat_map(move |i| (i + 1..len).map(move |j| (i, j)))
}
