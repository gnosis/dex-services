extern crate wasm_bindgen_test;

#[path = "../../data/mod.rs"]
mod data;

use dex_pricegraph::PriceEstimator;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
}

#[wasm_bindgen_test]
fn estimate_price() {
    let start = now();

    let estimator = PriceEstimator::new(&*data::DEFAULT_ORDERBOOK).unwrap();
    let price = estimator
        .estimate_price(7, 1, 100.0 * 10.0f64.powi(18))
        .unwrap();

    let elapsed = now() - start;

    console_log!(
        "DAI-WETH price for selling 100 WETH: 1 WETH = {} DAI ({}ms)",
        price,
        elapsed,
    );
}
