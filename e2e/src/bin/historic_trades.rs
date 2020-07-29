use anyhow::Result;
use core::{
    contracts::stablex_contract::batch_exchange::event_data::Trade, history::Settlement,
    models::BatchId,
};
use e2e::cmd;
use pricegraph::{Element, Pricegraph, TokenPair, U256};
use std::{fs::File, io::Write, path::PathBuf};
use structopt::StructOpt;

const FULLY_FILLED_THREASHOLD: f64 = 0.95;
const MIN_AMOUNT: u128 = 10_000;

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

    let mut report = Report::new(File::create(options.output_dir.join("trades.csv"))?);
    report.header()?;

    cmd::for_each_batch(&options.orderbook_file, |history, batch| {
        let auction_elements = history.auction_elements_for_batch(batch)?;
        let settlement = history.settlement_for_batch(batch);

        let new_orders = auction_elements
            .iter()
            .filter(|order| batch == order.valid.from)
            .filter(|order| order.price.numerator != 0 && order.price.denominator > MIN_AMOUNT)
            .filter(|order| order.balance > U256::from(MIN_AMOUNT));
        for order in new_orders {
            let settlement = settlement.as_ref();
            let pricegraph = Pricegraph::new(
                auction_elements
                    .iter()
                    .filter(|element| (element.user, element.id) != (order.user, order.id))
                    .cloned(),
            );

            let meta = OrderMetadata::compute(settlement, order, &pricegraph);
            let result = if meta.is_resonable_order() {
                process_reasonable_order(settlement, order, &meta)
            } else {
                process_unreasonable_order(&meta)
            };

            report.record_result(batch, settlement, order, &meta, result)?;
        }

        Ok(())
    })?;

    report.summary()
}

struct OrderMetadata {
    effective_sell_amount: f64,
    limit_price: f64,
    estimated_limit_price: Option<f64>,
    settled_xrate: Option<f64>,
    trade: Option<Trade>,
}

impl OrderMetadata {
    fn compute(settlement: Option<&Settlement>, order: &Element, pricegraph: &Pricegraph) -> Self {
        let effective_sell_amount =
            pricegraph::num::u256_to_f64(order.balance).min(order.remaining_sell_amount as _);
        let limit_price = order.price.numerator as f64 / order.price.denominator as f64;
        let estimated_limit_price =
            pricegraph.estimate_limit_price(order.pair, effective_sell_amount);
        let settled_xrate =
            settlement.and_then(|settlement| find_settled_exchange_rate(settlement, order.pair));
        let trade = settlement.and_then(|settlement| find_trade(settlement, order));

        OrderMetadata {
            effective_sell_amount,
            limit_price,
            estimated_limit_price,
            settled_xrate,
            trade,
        }
    }

    fn is_resonable_order(&self) -> bool {
        match self.estimated_limit_price {
            Some(estimated_limit_price) => self.limit_price <= estimated_limit_price,
            None => false,
        }
    }

    fn fill_ratio(&self) -> Option<f64> {
        self.trade
            .as_ref()
            .map(|trade| trade.executed_sell_amount as f64 / self.effective_sell_amount)
    }
}

fn process_reasonable_order(
    settlement: Option<&Settlement>,
    order: &Element,
    meta: &OrderMetadata,
) -> TradeResult {
    let settlement = match settlement {
        Some(value) => value,
        None => return TradeResult::NoSolution,
    };

    match meta.fill_ratio() {
        Some(trade_ratio) if trade_ratio > FULLY_FILLED_THREASHOLD => TradeResult::FullyMatched,
        Some(_) => TradeResult::PartiallyMatched,
        None => match find_settled_exchange_rate(settlement, order.pair) {
            Some(xrate) if xrate > meta.limit_price => TradeResult::SkippedMatchableOrder,
            Some(_) => TradeResult::OverlyOptimistic,
            None => TradeResult::NotTraded,
        },
    }
}

fn process_unreasonable_order(meta: &OrderMetadata) -> TradeResult {
    match meta.settled_xrate {
        Some(xrate) if xrate > meta.limit_price => TradeResult::OverlyPessimistic,
        _ => TradeResult::UnreasonableOrderNotMatched,
    }
}

fn find_trade(settlement: &Settlement, order: &Element) -> Option<Trade> {
    settlement
        .trades
        .iter()
        .find(|trade| (trade.owner, trade.order_id) == (order.user, order.id))
        .cloned()
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

enum TradeResult {
    FullyMatched,
    UnreasonableOrderNotMatched,

    OverlyPessimistic,
    SkippedMatchableOrder,

    PartiallyMatched,
    OverlyOptimistic,
    NoSolution,
    NotTraded,
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
    fn new(output: T) -> Self {
        Report {
            output,
            skipped: 0,
            success: 0,
            failed: 0,
        }
    }

    fn header(&mut self) -> Result<()> {
        let rows = [
            "batch",
            "order",
            "solved",
            "sellAmount",
            "limitPrice",
            "estimatedXrate",
            "settledXrate",
            "fillAmount",
        ];
        writeln!(&mut self.output, "{}", rows.join(","))?;
        Ok(())
    }

    fn record_result(
        &mut self,
        batch: BatchId,
        settlement: Option<&Settlement>,
        order: &Element,
        meta: &OrderMetadata,
        result: TradeResult,
    ) -> Result<()> {
        let order_uid = format!("{}-{}", order.user, order.id);
        let solved = settlement.is_some();

        writeln!(
            &mut self.output,
            "{},{},{},{},{},{},{},{}",
            batch,
            order_uid,
            solved,
            meta.effective_sell_amount,
            meta.limit_price,
            meta.estimated_limit_price.unwrap_or_default(),
            meta.settled_xrate.unwrap_or_default(),
            meta.fill_ratio().unwrap_or_default(),
        )?;

        match result {
            TradeResult::FullyMatched | TradeResult::UnreasonableOrderNotMatched => {
                self.success += 1
            }
            TradeResult::OverlyPessimistic | TradeResult::SkippedMatchableOrder => {
                self.skipped += 1
            }
            TradeResult::PartiallyMatched
            | TradeResult::OverlyOptimistic
            | TradeResult::NoSolution
            | TradeResult::NotTraded => self.failed += 1,
        };

        Ok(())
    }

    fn summary(&mut self) -> Result<()> {
        let total = self.success + self.skipped + self.failed;
        let percent = |value: usize| 100.0 * value as f64 / total as f64;
        println!(
            "Processed {} orders: {:.2}% correct, {:.2}% failed, {:.2}% skipped.",
            total,
            percent(self.success),
            percent(self.skipped),
            percent(self.failed),
        );

        Ok(())
    }
}
