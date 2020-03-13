#[test]
fn test_rinkeby() {
    std::thread::sleep(std::time::Duration::from_secs(10));
    // Make sure there was no error
    let output = std::process::Command::new("docker")
        .arg("logs")
        .arg("dex-services_stablex_1")
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    // Errors go to stderr while other messages go to stdout.
    let logs = String::from_utf8(output.stderr).expect("failed to read logs");
    assert!(logs.is_empty());
}
