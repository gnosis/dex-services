use anyhow::{anyhow, Context, Result};

fn find_stablex_container_in_output(output: &str) -> Option<&str> {
    output
        .split('\n')
        .skip(2)
        .filter_map(|line| line.split(' ').next())
        .find(|container_name| container_name.starts_with("dex-services_stablex_1"))
}

fn find_stablex_container() -> Result<String> {
    let output = std::process::Command::new("docker-compose")
        .arg("ps")
        .output()
        .context("failed to execute `docker-compose ps`")?;
    if !output.status.success() {
        return Err(anyhow!("status code is not success"));
    }
    let output = String::from_utf8(output.stdout).context("output is not utf8")?;
    find_stablex_container_in_output(&output)
        .map(|name| name.to_string())
        .ok_or_else(|| anyhow!("failed to find stablex container"))
}

#[test]
fn test_find_container_name() {
    let output = r#"
               Name                         Command               State           Ports
--------------------------------------------------------------------------------------------
dex-services_ganache-cli_1   node /app/ganache-core.doc ...   Up      0.0.0.0:8545->8545/tcp
dex-services_stablex_1_f31cbd690fe3       /tini -- cargo run               Up      0.0.0.0:9586->9586/tcp
"#;
    assert_eq!(
        find_stablex_container_in_output(output).unwrap(),
        "dex-services_stablex_1_f31cbd690fe3"
    );
}

#[test]
fn test_rinkeby() {
    std::thread::sleep(std::time::Duration::from_secs(10));
    // Make sure there was no error
    let container_name = find_stablex_container()
        .context("failed to find stablex container name")
        .unwrap();
    let output = std::process::Command::new("docker")
        .arg("logs")
        .arg(&container_name)
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    // Errors go to stderr while other messages go to stdout.
    let logs = String::from_utf8(output.stderr).expect("failed to read logs");
    assert!(logs.is_empty());
}
