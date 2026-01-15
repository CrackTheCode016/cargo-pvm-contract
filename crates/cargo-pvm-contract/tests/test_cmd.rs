use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn cargo_path() -> PathBuf {
    std::env::var("CARGO")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("cargo"))
}

fn scaffold_example(temp_dir: &TempDir, name: &str, memory_model: &str) -> PathBuf {
    let builder_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cargo-pvm-contract-builder");
    let project_dir = temp_dir.path().join(name);
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("cargo-pvm-contract"));
    cmd.current_dir(temp_dir.path())
        .env("CARGO_PVM_CONTRACT_BUILDER_PATH", builder_path)
        .arg("pvm-contract")
        .arg("--init-type")
        .arg("example")
        .arg("--example")
        .arg("MyToken")
        .arg("--memory-model")
        .arg(memory_model)
        .arg("--name")
        .arg(name)
        .assert()
        .success();

    project_dir
}

fn build_scaffolded_project(project_dir: &Path) {
    let status = std::process::Command::new(cargo_path())
        .current_dir(project_dir)
        .arg("build")
        .status()
        .expect("run cargo build");

    assert!(status.success(), "cargo build failed");
}

#[test]
fn scaffold_mytoken_alloc() {
    let temp_dir = TempDir::new().expect("temp dir");
    let project_dir = scaffold_example(&temp_dir, "mytoken-alloc", "alloc-with-alloy");

    let cargo_toml =
        std::fs::read_to_string(project_dir.join("Cargo.toml")).expect("Cargo.toml exists");

    assert!(cargo_toml.contains("alloy-core"));
    assert!(cargo_toml.contains("picoalloc"));
    assert!(cargo_toml.contains("pallet-revive-uapi"));

    build_scaffolded_project(&project_dir);
}

#[test]
fn scaffold_mytoken_no_alloc() {
    let temp_dir = TempDir::new().expect("temp dir");
    let project_dir = scaffold_example(&temp_dir, "mytoken-no-alloc", "no-alloc");

    let cargo_toml =
        std::fs::read_to_string(project_dir.join("Cargo.toml")).expect("Cargo.toml exists");

    assert!(!cargo_toml.contains("alloy-core"));
    assert!(!cargo_toml.contains("picoalloc"));
    assert!(cargo_toml.contains("pallet-revive-uapi"));

    build_scaffolded_project(&project_dir);
}
