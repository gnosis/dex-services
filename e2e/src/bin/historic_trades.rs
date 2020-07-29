use anyhow::Result;
use core::{
    contracts::stablex_contract::batch_exchange::event_data::Trade, history::Settlement,
    models::BatchId,
};
use e2e::cmd;
use pricegraph::{Element, Pricegraph, TokenPair};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

const FULLY_FILLED_THREASHOLD: f64 = 0.95;

/// Common options for analyzing historic batch data.
#[derive(Debug, StructOpt)]
#[structopt(
    name = "historic_trades",
    about = "Utility for comparing historic trades to price estimates.",
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
    let mut report = Report::new(File::create(options.output_dir.join("trades.csv"))?)?;

    cmd::for_each_batch(&options.orderbook_file, |history, batch| {
        let auction_elements = history.auction_elements_for_batch(batch)?;
        let settlement = history.settlement_for_batch(batch);

        let new_orders = auction_elements
            .iter()
            .filter(|order| batch == order.valid.from)
            .filter(|order| order.price.numerator != 0 && order.price.denominator != 0);
        for order in new_orders {
            let pricegraph = Pricegraph::new(
                auction_elements
                    .iter()
                    .filter(|element| (element.user, element.id) != (order.user, order.id))
                    .cloned(),
            );

            let limit_price = order.price.numerator as f64 / order.price.denominator as f64;
            let effective_amount =
                pricegraph::num::u256_to_f64(order.balance).min(order.remaining_sell_amount as _);
            let estimated_limit_price =
                pricegraph.estimate_limit_price(order.pair, effective_amount);

            if limit_price > estimated_limit_price.unwrap_or_default() {
                let settled_xrate = settlement
                    .as_ref()
                    .and_then(|settlement| find_settled_exchange_rate(settlement, order.pair));
                match settled_xrate {
                    Some(xrate) if xrate > limit_price => {
                        report.overly_pessimistic_estimate(batch, &order, xrate)?
                    }
                    _ => report.unreasonable_order(batch, &order)?,
                };

                continue;
            }

            let settlement = match settlement.as_ref() {
                Some(value) => value,
                None => {
                    report.no_solution(batch, &order)?;
                    continue;
                }
            };

            match find_trade(settlement, order) {
                Some(trade) => {
                    let trade_ratio = trade.executed_sell_amount as f64 / effective_amount;
                    if trade_ratio > FULLY_FILLED_THREASHOLD {
                        report.fully_matched(batch, &order, &trade)?;
                    } else {
                        report.fully_matched(batch, &order, &trade)?;
                    }
                }
                None => match find_settled_exchange_rate(settlement, order.pair) {
                    Some(xrate) if xrate > limit_price => {
                        report.disregarded_order(batch, &order)?
                    }
                    Some(xrate) => report.incorrect_estimate(batch, &order, xrate)?,
                    None => report.not_traded(batch, &order)?,
                },
            }
        }

        Ok(())
    })?;

    report.summary()
}

fn find_trade<'a>(settlement: &'a Settlement, order: &Element) -> Option<&'a Trade> {
    settlement
        .trades
        .iter()
        .find(|trade| (trade.owner, trade.order_id) == (order.user, order.id))
}

fn find_settled_exchange_rate(settlement: &Settlement, pair: TokenPair) -> Option<f64> {
    let buy_token_index = settlement
        .solution
        .token_ids_for_price
        .iter()
        .position(|&id| id == pair.buy)?;
    let sell_token_index = settlement
        .solution
        .token_ids_for_price
        .iter()
        .position(|&id| id == pair.sell)?;

    // NOTE: This is unintuitive, but the echange rate is computed as the sell
    // price over the buy price. This is best illustrated by an example:
    // - The price of WETH is ~200 OWL
    // - The price of DAI is ~1 OWL
    // - The limit price for an order buying WETH and selling DAI would be
    //   `buy_amount / sell_amount` which sould be around `1 / 200` since you
    //   would only be able to buy around 1 WETH for 200 DAI
    // So, by example, the `xrate = sell_price / buy_price`.
    Some(
        settlement.solution.prices[sell_token_index] as f64
            / settlement.solution.prices[buy_token_index] as f64,
    )
}

struct Report<T> {
    output: T,
    skipped: usize,
    success: usize,
    failed: usize,
}

impl<T> Report<T>
where
    T: Write,
{
    fn new(mut output: T) -> Result<Self> {
        write!(&mut output, "")?;
        Ok(Report {
            output,
            skipped: 0,
            success: 0,
            failed: 0,
        })
    }

    fn unreasonable_order(&mut self, batch: BatchId, order: &Element) -> Result<()> {
        self.skipped += 1;
        Ok(())
    }

    fn overly_pessimistic_estimate(
        &mut self,
        batch: BatchId,
        order: &Element,
        settled_price: f64,
    ) -> Result<()> {
        self.skipped += 1;
        Ok(())
    }

    fn no_solution(&mut self, batch: BatchId, order: &Element) -> Result<()> {
        self.skipped += 1;
        Ok(())
    }

    fn fully_matched(&mut self, batch: BatchId, order: &Element, trade: &Trade) -> Result<()> {
        self.success += 1;
        Ok(())
    }

    fn partially_matched(&mut self, batch: BatchId, order: &Element, trade: &Trade) -> Result<()> {
        self.failed += 1;
        Ok(())
    }

    fn disregarded_order(&mut self, batch: BatchId, order: &Element) -> Result<()> {
        self.skipped += 1;
        Ok(())
    }

    fn incorrect_estimate(
        &mut self,
        batch: BatchId,
        order: &Element,
        settled_price: f64,
    ) -> Result<()> {
        self.failed += 1;
        Ok(())
    }

    fn not_traded(&mut self, batch: BatchId, order: &Element) -> Result<()> {
        self.failed += 1;
        Ok(())
    }

    fn summary(&mut self) -> Result<()> {
        Ok(())
    }
}
