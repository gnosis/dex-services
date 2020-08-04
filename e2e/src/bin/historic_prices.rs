use anyhow::Result;
use core::{history::Settlement, models::BatchId};
use e2e::cmd::{self, Reporting, Sampler};
use pricegraph::{Element, Pricegraph, TokenId};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

/// Threshold at which a price estimate is considered "bad"; currently 10%.
const BAD_ESTIMATE_THRESHOLD: f64 = 0.1;

/// Common options for analyzing historic batch data.
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
    let mut report = Report::new(File::create(&options.output_dir.join("prices.csv"))?);

    report.header()?;
    cmd::for_each_batch(
        &options.orderbook_file,
        report,
        move |sampler, history, batch| {
            let settlement = match history.settlement_for_batch(batch) {
                Some(value) => value,
                None => return Ok(()),
            };

            let auction_elements = history.auction_elements_for_batch(batch)?;
            check_batch_prices(&sampler, batch, &auction_elements, &settlement)?;

            Ok(())
        },
    )?;

    Ok(())
}

fn check_batch_prices(
    sampler: &Sampler<Row>,
    batch: BatchId,
    auction_elements: &[Element],
    settlement: &Settlement,
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

        sampler.record_sample(Row {
            batch,
            token,
            price,
            estimate,
        })?;
    }

    Ok(())
}

struct Row {
    batch: BatchId,
    token: TokenId,
    price: u128,
    estimate: u128,
}

struct Report<T> {
    output: T,
    samples: usize,
    bad_estimates: usize,
    total_error: f64,
}

impl<T> Report<T>
where
    T: Write,
{
    fn new(output: T) -> Self {
        Report {
            output,
            samples: 0,
            bad_estimates: 0,
            total_error: 0.0,
        }
    }

    fn header(&mut self) -> Result<()> {
        writeln!(&mut self.output, "batch,token,price,estimate,error")?;
        Ok(())
    }
}

impl<T> Reporting for Report<T>
where
    T: Write,
{
    type Sample = Row;
    type Summary = ();

    fn record_sample(
        &mut self,
        Row {
            batch,
            token,
            price,
            estimate,
        }: Row,
    ) -> Result<()> {
        let error = (estimate as f64 - price as f64).abs() / price as f64;
        writeln!(
            &mut self.output,
            "{},{},{},{},{}",
            batch, token, price, estimate, error,
        )?;

        self.samples += 1;
        self.bad_estimates += (error > BAD_ESTIMATE_THRESHOLD) as usize;
        self.total_error += error;

        Ok(())
    }

    fn finalize(self) -> Result<()> {
        println!(
            "Processed {} token prices: bad estimates {:.2}%, average error {:.2}%.",
            self.samples,
            100.0 * self.bad_estimates as f64 / self.samples as f64,
            100.0 * self.total_error / self.samples as f64,
        );
        Ok(())
    }
}
