use anyhow::Result;
use e2e::cmd::{self, Reporting, SampleChannel};
use pricegraph::{Element, Pricegraph, TokenId};
use services_core::{history::Settlement, models::BatchId};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

/// Threshold logarithmic distance from the actual price at which an estimate is
/// considered "bad"; currently `0.1 * price < estimate < 10 * price`.
const BAD_ESTIMATE_THRESHOLD_LOG_DISTANCE: f64 = 1.0;

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
        move |samples, history, batch| {
            let settlement = match history.settlement_for_batch(batch) {
                Some(value) => value,
                None => return Ok(()),
            };

            let auction_elements = history.auction_elements_for_batch(batch)?;
            check_batch_prices(&samples, batch, &auction_elements, &settlement)?;

            Ok(())
        },
    )?;

    Ok(())
}

fn check_batch_prices(
    samples: &SampleChannel<Row>,
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
        let estimate = pricegraph
            .estimate_token_price(token, None)?
            .map(|p| p as u128);

        samples.record_sample(Row {
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
    estimate: Option<u128>,
}

struct Report<T> {
    output: T,
    samples: usize,
    total_error: f64,
    bad_estimates: usize,
    missed_estimates: usize,
}

impl<T> Report<T>
where
    T: Write,
{
    fn new(output: T) -> Self {
        Report {
            output,
            samples: 0,
            total_error: 0.0,
            bad_estimates: 0,
            missed_estimates: 0,
        }
    }

    fn header(&mut self) -> Result<()> {
        writeln!(
            &mut self.output,
            "batch,token,price,estimate,error,distance",
        )?;
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
        let price_estimate = estimate.unwrap_or_default();
        let error = (price_estimate as f64 - price as f64).abs() / price as f64;
        let distance = (price_estimate as f64 / price as f64).log10();
        writeln!(
            &mut self.output,
            "{},{},{},{},{},{}",
            batch, token, price, price_estimate, error, distance,
        )?;

        self.samples += 1;
        if estimate.is_some() {
            self.total_error += error;
            self.bad_estimates += (distance.abs() >= BAD_ESTIMATE_THRESHOLD_LOG_DISTANCE) as usize;
        } else {
            self.missed_estimates += 1;
        }

        Ok(())
    }

    fn finalize(self) -> Result<()> {
        println!(
            "Processed {} token prices: average error {:.2}%, bad {:.2}%, missed {:.2}%.",
            self.samples,
            100.0 * self.total_error / self.samples as f64,
            100.0 * self.bad_estimates as f64 / self.samples as f64,
            100.0 * self.missed_estimates as f64 / self.samples as f64,
        );
        Ok(())
    }
}
