// Stable X contracts artifacts
ethcontract::contract!(pub "dex-contracts/build/contracts/BatchExchange.json");
ethcontract::contract!(pub "dex-contracts/build/contracts/IERC20.json");
ethcontract::contract!(pub "dex-contracts/build/contracts/IdToAddressBiMap.json");
ethcontract::contract!(pub "dex-contracts/build/contracts/IterableAppendOnlySet.json");
ethcontract::contract!(pub "dex-contracts/build/contracts/TokenOWL.json");
ethcontract::contract!(pub "dex-contracts/build/contracts/ERC20Mintable.json");

pub mod common;
pub mod docker_logs;
pub mod history;
pub mod stablex;
