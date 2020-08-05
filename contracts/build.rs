use ethcontract_generate::Builder;
use std::{env, fs, path::Path};

#[path = "src/paths.rs"]
mod paths;

const DEX_CONTRACTS_VERSION: &str = "0.4.1";

fn main() {
    // NOTE: This is a workaround for `rerun-if-changed` directives for
    // non-existant files cause the crate's build unit to get flagged for a
    // rebuild if any files in the workspace change.
    //
    // See:
    // - https://github.com/rust-lang/cargo/issues/6003
    // - https://doc.rust-lang.org/cargo/reference/build-scripts.html#cargorerun-if-changedpath
    println!("cargo:rerun-if-changed=build.rs");

    generate_contract("BatchExchange");
    generate_contract("BatchExchangeViewer");
}

fn generate_contract(name: &str) {
    let artifact = format!(
        "npm:@gnosis.pm/dex-contracts@{}/build/contracts/{}.json",
        DEX_CONTRACTS_VERSION, name,
    );
    let address_file = paths::contract_address_file(name);
    let dest = env::var("OUT_DIR").unwrap();

    let mut builder = Builder::from_source_url(artifact)
        .unwrap()
        .with_visibility_modifier(Some("pub"))
        .add_event_derive("serde::Deserialize")
        .add_event_derive("serde::Serialize");

    if let Ok(address) = fs::read_to_string(&address_file) {
        println!("cargo:rerun-if-changed={}", address_file.display());
        builder = builder.add_deployment_str(5777, address.trim());
    }

    builder
        .generate()
        .unwrap()
        .write_to_file(Path::new(&dest).join(format!("{}.rs", name)))
        .unwrap();
}
