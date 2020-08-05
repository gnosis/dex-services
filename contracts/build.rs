use ethcontract_generate::Builder;
use std::{env, fs, path::Path};

#[path = "src/paths.rs"]
mod paths;

fn main() {
    generate_contract("BatchExchange");
    generate_contract("BatchExchangeViewer");
    generate_contract("IdToAddressBiMap");
    generate_contract("IterableAppendOnlySet");
    generate_contract("TokenOWL");
    generate_contract("TokenOWLProxy");
}

fn generate_contract(name: &str) {
    let artifact = format!("../dex-contracts/build/contracts/{}.json", name);
    let dest = env::var("OUT_DIR").unwrap();

    let mut builder = Builder::new(artifact)
        .with_visibility_modifier(Some("pub"))
        .add_event_derive("serde::Deserialize")
        .add_event_derive("serde::Serialize");

    if let Some(address) = contract_address(name) {
        builder = builder.add_deployment_str(5777, address.trim());
    }

    builder
        .generate()
        .unwrap()
        .write_to_file(Path::new(&dest).join(format!("{}.rs", name)))
        .unwrap();
}

fn contract_address(name: &str) -> Option<String> {
    let path = paths::contract_address_file(name);
    println!("cargo:rerun-if-changed={}", path.display());
    Some(fs::read_to_string(path).ok()?)
}
