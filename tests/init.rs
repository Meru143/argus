use std::process::Command;

#[test]
fn init_creates_valid_toml() {
    let dir = tempfile::tempdir().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "argus init failed: {}", String::from_utf8_lossy(&output.stderr));

    let config_path = dir.path().join(".argus.toml");
    assert!(config_path.exists(), ".argus.toml should exist");

    let content = std::fs::read_to_string(&config_path).unwrap();
    assert!(content.contains("[review]"));
    assert!(content.contains("[embedding]"));

    // Verify it's valid TOML that argus-core can parse
    let _config: argus_core::ArgusConfig = toml::from_str(&content).unwrap();
}

#[test]
fn init_refuses_if_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".argus.toml"), "# existing").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_argus"))
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
}
