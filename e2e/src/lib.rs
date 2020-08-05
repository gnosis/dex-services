ethcontract::contract!(pub "npm:@gnosis.pm/dex-contracts@0.4.1/build/contracts/IERC20.json");
ethcontract::contract!(pub "npm:@gnosis.pm/owl-token@3.1.0/build/contracts/TokenOWL.json");
ethcontract::contract!(pub "npm:@openzeppelin/contracts@3.1.0/build/contracts/ERC20Mintable.json");

pub mod cmd;
pub mod common;
pub mod docker_logs;
pub mod stablex;
