use anyhow::Result;
use contracts::ethcontract::{Address, U256};
use env_logger::Env;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    ops::Deref,
    path::{Path, PathBuf},
};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "pricegraph-data-convert",
    about = "Converts a solver instance file into a hex-encoded orderbook for testing `pricegraph`."
)]
struct Options {
    /// The solver instance file to convert.
    #[structopt(name = "INSTANCE")]
    instance: PathBuf,

    /// The batch ID for the corresponding instance.
    #[structopt(name = "BATCH_ID")]
    batch_id: u32,
}

fn main() {
    env_logger::init_from_env(Env::default().default_filter_or("warn,convert=debug"));

    if let Err(err) = run(Options::from_args()) {
        log::error!("Error converting orderbook: {:?}", err);
        std::process::exit(-1);
    }
}

fn run(options: Options) -> Result<()> {
    let instance = serde_json::from_str::<Instance>(&fs::read_to_string(&options.instance)?)?;

    log::info!(
        "encoding {} orders from `{}`",
        instance.orders.len(),
        options.instance.display(),
    );

    let mut output = File::create(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../orderbook-{}.hex", options.batch_id)),
    )?;
    for order in &instance.orders {
        let balance = instance
            .accounts
            .get(&order.account_id)
            .and_then(|account| account.get(&order.sell_token))
            .copied()
            .unwrap_or_default();
        write_order(&mut output, order, balance)?;
    }

    Ok(())
}

fn write_order(mut output: impl Write, order: &Order, balance: Balance) -> Result<()> {
    macro_rules! num {
        ($x:expr, $size:expr) => {
            format!("{:0digits$x}", $x, digits = ($size * 2))
        };

        // NOTE: `U256` implementation does not support `LowerHex` format with
        // `0` padding.
        (u256: $x:expr) => {
            format!("{:0>64}", format!("{:x}", *$x))
        };
    }

    writeln!(
        output,
        "{}",
        vec![
            hex::encode(order.account_id.as_bytes()),
            num!(u256: balance),
            num!(*order.buy_token, 2),
            num!(*order.sell_token, 2),
            num!(u32::MIN, 4),
            num!(u32::MAX, 4),
            num!(order.buy_amount, 16),
            num!(order.sell_amount, 16),
            num!(order.sell_amount, 16),
            num!(order.order_id, 2),
        ]
        .join(" ")
    )?;

    Ok(())
}

// TODO(nlordell): This has some duplicated code with the `core` crate. It would
// be nice to split out the solver interface format into its own crate so that
// it can be re-used here and potentially in other solver implementations.

#[derive(Deserialize)]
struct Instance {
    accounts: HashMap<Address, HashMap<TokenRef, Balance>>,
    orders: Vec<Order>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Order {
    #[serde(rename = "accountID")]
    account_id: Address,
    sell_token: TokenRef,
    buy_token: TokenRef,
    #[serde(with = "serde_with::rust::display_fromstr")]
    sell_amount: u128,
    #[serde(with = "serde_with::rust::display_fromstr")]
    buy_amount: u128,
    #[serde(rename = "orderID")]
    order_id: u16,
}

#[derive(Deserialize, Eq, Hash, PartialEq)]
struct TokenRef(#[serde(with = "token_ref")] u16);

impl Deref for TokenRef {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

mod token_ref {
    use serde::de::{Deserialize, Deserializer, Error as _};
    use std::borrow::Cow;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u16, D::Error>
    where
        D: Deserializer<'de>,
    {
        let token_ref = Cow::<'de, str>::deserialize(deserializer)?;
        token_ref
            .strip_prefix("T")
            .and_then(|t| t.parse::<u16>().ok())
            .ok_or_else(|| D::Error::custom("token ID '{}' not in the format 'Txxxx'"))
    }
}

#[derive(Clone, Copy, Default, Deserialize)]
struct Balance(#[serde(with = "balance")] U256);

impl Deref for Balance {
    type Target = U256;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

mod balance {
    use contracts::ethcontract::U256;
    use serde::de::{Deserialize, Deserializer, Error as _};
    use std::borrow::Cow;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let dec = Cow::<'de, str>::deserialize(deserializer)?;
        U256::from_dec_str(&dec).map_err(|err| D::Error::custom(err.to_string()))
    }
}
