use anyhow::{anyhow, Context, Result};
use std::process::Command;

fn find_container_id(container_name: &str) -> Result<String> {
    let output = Command::new("docker")
        .arg("ps")
        .arg("--quiet")
        .arg("--filter")
        .arg(format!("name={}", container_name))
        .output()
        .context("failed to execute `docker ps`")?;
    if !output.status.success() {
        return Err(anyhow!("status code is not success"));
    }
    let output = std::str::from_utf8(&output.stdout).context("output is not utf8")?;
    output
        .split('\n')
        .next()
        .map(|string| string.to_string())
        .ok_or_else(|| anyhow!("did not find container in output: {}", output))
}

pub fn assert_no_errors_logged(container_name: &str) {
    let container_id = find_container_id(container_name)
        .context("failed to find stablex container name")
        .unwrap();
    let output = Command::new("docker")
        .arg("logs")
        .arg(container_id)
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    // Errors go to stderr while other messages go to stdout.
    assert!(output.stderr.is_empty());
}
