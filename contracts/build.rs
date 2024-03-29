use ethcontract::{common::DeploymentInformation, Address};
use ethcontract_generate::Builder;
use maplit::hashmap;
use std::str::FromStr;
use std::{collections::HashMap, env, fs, path::Path};

#[path = "src/paths.rs"]
mod paths;

fn main() {
    // NOTE: This is a workaround for `rerun-if-changed` directives for
    // non-existant files cause the crate's build unit to get flagged for a
    // rebuild if any files in the workspace change.
    //
    // See:
    // - https://github.com/rust-lang/cargo/issues/6003
    // - https://doc.rust-lang.org/cargo/reference/build-scripts.html#cargorerun-if-changedpath
    println!("cargo:rerun-if-changed=build.rs");

    generate_contract_deployed_at(
        "BatchExchange",
        hashmap! {
            1 => (Address::from_str("0x6F400810b62df8E13fded51bE75fF5393eaa841F").unwrap(), 9340147),
            4 => (Address::from_str("0xC576eA7bd102F7E476368a5E98FA455d1Ea34dE2").unwrap(), 5844678),
            100 => (Address::from_str("0x25B06305CC4ec6AfCF3E7c0b673da1EF8ae26313").unwrap(), 11948310),
        },
    );
    generate_contract("BatchExchangeViewer");
    generate_contract("ERC20Mintable");
    generate_contract("IERC20");
    generate_contract("IdToAddressBiMap");
    generate_contract("IterableAppendOnlySet");
    generate_contract("SolutionSubmitter");
    generate_contract("TokenOWL");
    generate_contract("TokenOWLProxy");
}

fn generate_contract(name: &str) {
    generate_contract_deployed_at(name, HashMap::new())
}

fn generate_contract_deployed_at(name: &str, deployment_info: HashMap<u32, (Address, u64)>) {
    let artifact = paths::contract_artifacts_dir().join(format!("{}.json", name));
    let address_file = paths::contract_address_file(name);
    let dest = env::var("OUT_DIR").unwrap();

    println!("cargo:rerun-if-changed={}", artifact.display());
    let mut builder = Builder::new(artifact)
        .with_visibility_modifier(Some("pub"))
        .add_event_derive("serde::Deserialize")
        .add_event_derive("serde::Serialize");

    if let Ok(address) = fs::read_to_string(&address_file) {
        println!("cargo:rerun-if-changed={}", address_file.display());
        builder = builder.add_deployment_str(5777, address.trim());
    }

    for (network_id, (address, deployment_block)) in deployment_info {
        builder = builder.add_deployment(
            network_id,
            address,
            Some(DeploymentInformation::BlockNumber(deployment_block)),
        );
    }

    builder
        .generate()
        .unwrap()
        .write_to_file(Path::new(&dest).join(format!("{}.rs", name)))
        .unwrap();
}
