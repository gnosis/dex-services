use crate::models;
use crate::price_finding::error::{PriceFindingError, ErrorKind};
use crate::price_finding::price_finder_interface::{PriceFinding, Solution};

use serde_json::json;
use web3::types::U256;
use std::collections::HashMap;
use chrono::{Utc};
use std::fs::File;
use std::process::Command;
use std::io::BufReader;

const RESULT_FOLDER: &str = "./results/tmp/";
type Prices = HashMap<String, String>;

pub struct LinearOptimisationPriceFinder {
    previous_prices: Prices
}

impl LinearOptimisationPriceFinder {
    pub fn new() -> Self {
        // All prices are 1 (10**18)
        return LinearOptimisationPriceFinder {
            previous_prices: (0..models::TOKENS)
                .map(|t| (token_id(t), "1000000000000000000".to_string()))
                .collect()
        }
    }
}

fn token_id(token: u8) -> String {
    format!("token{}", token)
}

fn account_id(account: u16) -> String {
    format!("account{}", account)
}

fn serialize_order(order: &models::Order, id: &str) -> serde_json::Value {
    json!({
        "accountID": account_id(order.account_id),
        "sellToken": token_id(order.sell_token),
        "buyToken": token_id(order.buy_token), 
        "sellAmount": order.sell_amount.to_string(),
        "buyAmount": order.buy_amount.to_string(),
        "ID": id //TODO this should not be needed
    })
}

fn serialize_balances(balances: &Vec<u128>) -> serde_json::Value {
    let mut accounts: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_account = 1;
    for balances_for_current_account in balances.chunks(models::TOKENS as usize) {
        accounts.insert(account_id(current_account), (0..models::TOKENS)
            .map(|t| token_id(t))
            .zip(balances_for_current_account.iter().map(|b| b.to_string()))
            .collect());
        current_account += 1;
    }
    json!(accounts)
}

fn deserialize_result(json: &serde_json::Value) -> Result<(Prices, Solution), PriceFindingError> {
    let price_map = json["prices"]
        .as_object()
        .ok_or_else(|| "No 'price' object in json")?
        .iter()
        .map(|(token, price)| price
            .as_str()
            .map(|p| (token.to_owned(), p.to_owned()))
            .ok_or_else(|| PriceFindingError::new(&format!("Couldnot convert price to string"), ErrorKind::JsonError))
        )
        .collect::<Result<Prices, PriceFindingError>>()?;
    let prices = (0..models::TOKENS)
        .map(|t| price_map.get(&token_id(t))
            .ok_or_else(|| PriceFindingError::new(&format!("Token {} not found in price map", t), ErrorKind::JsonError))
            .and_then(|price| price.parse::<u128>().map_err(|e| PriceFindingError::from(e)))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let orders = json["orders"].as_array().ok_or_else(|| "No 'orders' list in json")?;
    let surplus = orders
        .iter()
        .map(|o| o["execSurplus"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execSurplus' field on order",  ErrorKind::JsonError))
            .and_then(|surplus| U256::from_dec_str(surplus).map_err(
                |e| PriceFindingError::new(&format!("{:?}", e), ErrorKind::ParseIntError)
            ))
        )
        .collect::<Result<Vec<U256>, PriceFindingError>>()?
        .iter()
        .fold(U256::zero(), |acc, surplus| surplus.saturating_add(acc));
    let executed_sell_amounts = orders
        .iter()
        .map(|o| o["execSellAmount"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execSurplus' field on order",  ErrorKind::JsonError))
            .and_then(|amount| amount.parse::<u128>().map_err(|e| PriceFindingError::from(e)))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    let executed_buy_amounts = orders
        .iter()
        .map(|o| o["execBuyAmount"]
            .as_str()
            .ok_or_else(|| PriceFindingError::new("No 'execBuyAmount' field on order",  ErrorKind::JsonError))
            .and_then(|amount| amount.parse::<u128>().map_err(|e| PriceFindingError::from(e)))
        )
        .collect::<Result<Vec<u128>, PriceFindingError>>()?;
    Ok((price_map.to_owned(), Solution {
        surplus,
        prices,
        executed_sell_amounts,
        executed_buy_amounts,
    }))
}

impl PriceFinding for LinearOptimisationPriceFinder {
    fn find_prices(
        &mut self, 
        orders: &Vec<models::Order>, 
        state: &models::State
    ) -> Result<Solution, PriceFindingError> {
        let token_ids: Vec<String> = (0..models::TOKENS)
            .map(|t| token_id(t))
            .collect();
        let orders: Vec<serde_json::Value> = orders
            .iter()
            .enumerate()
            .map(|(index, order)| serialize_order(&order, &index.to_string()))
            .collect();
        let input = json!({
            "tokens": token_ids,
            "refToken": token_id(0),
            "pricesPrev": self.previous_prices,
            "accounts": serialize_balances(&state.balances),
            "orders": orders, 
        });
        let input_file = format!("instance_{}.json", Utc::now().to_rfc3339());
        write_input(&input_file, &input)?;
        run_solver(&input_file)?;
        let result = read_output()?;
        let (prices, solution) = deserialize_result(&result)?;
        self.previous_prices = prices;
        Ok(solution)
    }
}

fn write_input(input_file: &str, input: &serde_json::Value) -> std::io::Result<()> {
    let file = File::create(&input_file)?;
    serde_json::to_writer(file, input)?;
    Ok(())
}

fn run_solver(input_file: &str) -> Result<(), PriceFindingError> {
    let output = Command::new("python")
        .arg("../batchauctions/scripts/optimize_e2e.py")
        .arg(input_file)
        .args(&["--solverTimelimit", "120"])
        .args(&["--outputDir", RESULT_FOLDER])
        .output()?;

    if !output.status.success() {
        println!("Solver failed - stdout: {}, error: {}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        return Err(PriceFindingError::new("Solver execution failed", ErrorKind::ExecutionError))
    }
    Ok(())
}

fn read_output() -> std::io::Result<serde_json::Value> {
    let file = File::open(format!("{}{}", RESULT_FOLDER, "03_output_snark.json"))?;
    let reader = BufReader::new(file);
    let value = serde_json::from_reader(reader)?;
    Ok(value)
}
