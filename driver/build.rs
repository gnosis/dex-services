use ethcontract_generate::Builder;
use std::env;
use std::path::Path;

fn main() {
    generate_contract("BatchExchange", "batch_exchange.rs");
}

fn generate_contract(name: &str, out: &str) {
    let artifact = format!("../dex-contracts/build/contracts/{}.json", name);
    let dest = env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed={}", artifact);
    Builder::new(artifact)
        .generate()
        .unwrap()
        .write_to_file(Path::new(&dest).join(out))
        .unwrap();
}
