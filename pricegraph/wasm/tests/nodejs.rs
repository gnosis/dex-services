extern crate wasm_bindgen_test;

use dex_pricegraph::PriceEstimator;
use pricegraph_data::DEFAULT_ORDERBOOK;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = Date)]
    fn now() -> f64;
}

fn time<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let start = now();
    let result = f();
    (result, now() - start)
}

#[wasm_bindgen_test]
fn estimate_price() {
    let (estimator, load_time) = time(|| PriceEstimator::new(&*DEFAULT_ORDERBOOK).unwrap());
    let (price, estimate_time) = time(|| estimator.estimate_price(7, 1, 100e18).unwrap().unwrap());

    console_log!(
        "DAI-WETH price for selling 100 WETH: 1 WETH = {} DAI (load {}ms, estimate {}ms)",
        price,
        load_time,
        estimate_time,
    );
}
