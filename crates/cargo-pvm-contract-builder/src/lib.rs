//! # PolkaVM Contract Builder
//!
//! A utility for building Rust projects as PolkaVM bytecode.
//!
//! ## Usage in `build.rs`
//!
//! ```no_run
//! fn main() {
//!     cargo_pvm_contract_builder::PvmBuilder::new().build();
//! }
//! ```

use anyhow::{Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Internal environment variable to prevent recursive builds.
const INTERNAL_BUILD_ENV: &str = "CARGO_PVM_CONTRACT_INTERNAL";

/// The builder for building a PolkaVM binary.
pub struct PvmBuilder {
    /// The path to the `Cargo.toml` of the project that should be built.
    project_cargo_toml: PathBuf,
    /// Specific binary to build (None = all binaries).
    bin_name: Option<String>,
}

impl PvmBuilder {
    /// Create a new builder for the current project.
    pub fn new() -> Self {
        Self {
            project_cargo_toml: get_manifest_dir().join("Cargo.toml"),
            bin_name: None,
        }
    }

    /// Build only the specified binary.
    pub fn with_bin(mut self, name: impl Into<String>) -> Self {
        self.bin_name = Some(name.into());
        self
    }

    /// Build the PolkaVM binary.
    pub fn build(self) {
        // Check if we're in a recursive build
        if env::var(INTERNAL_BUILD_ENV).is_ok() {
            return;
        }

        if let Err(e) = build_project(&self.project_cargo_toml, self.bin_name) {
            eprintln!("PolkaVM build failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Returns the manifest dir from the `CARGO_MANIFEST_DIR` env.
fn get_manifest_dir() -> PathBuf {
    env::var("CARGO_MANIFEST_DIR")
        .expect("`CARGO_MANIFEST_DIR` is always set for `build.rs` files")
        .into()
}

/// Detect the build profile from the environment.
#[derive(Clone, Debug)]
enum Profile {
    Debug,
    Release,
}

impl Profile {
    fn detect() -> Self {
        match env::var("PROFILE").as_deref() {
            Ok("release") => Profile::Release,
            _ => Profile::Debug,
        }
    }

    fn cargo_arg(&self) -> &'static str {
        match self {
            Profile::Debug => "dev",
            Profile::Release => "release",
        }
    }

    fn directory(&self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

/// Get the build output directory.
fn get_build_dir() -> PathBuf {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set"));

    for ancestor in out_dir.ancestors() {
        if ancestor.file_name().map(|n| n == "target").unwrap_or(false) {
            return ancestor.join("pvmbuild");
        }
    }

    out_dir.join("pvmbuild")
}

/// Get the list of binary targets from Cargo.toml.
fn get_bin_targets(cargo_toml: &Path) -> Result<Vec<String>> {
    let content = fs::read_to_string(cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;

    let doc: toml_edit::DocumentMut = content.parse().context("Failed to parse Cargo.toml")?;

    let mut bins = Vec::new();

    if let Some(bin_array) = doc.get("bin").and_then(|b| b.as_array_of_tables()) {
        for bin in bin_array {
            if let Some(name) = bin.get("name").and_then(|n| n.as_str()) {
                bins.push(name.to_string());
            }
        }
    }

    if bins.is_empty() {
        if let Some(name) = doc
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
        {
            bins.push(name.to_string());
        }
    }

    Ok(bins)
}

/// Get the crate name from Cargo.toml
fn get_crate_name(cargo_toml: &Path) -> Result<String> {
    let content = fs::read_to_string(cargo_toml)
        .with_context(|| format!("Failed to read {}", cargo_toml.display()))?;

    let doc: toml_edit::DocumentMut = content.parse().context("Failed to parse Cargo.toml")?;

    doc.get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .context("No package name found in Cargo.toml")
}

/// Build the project.
fn build_project(project_cargo_toml: &Path, bin_name: Option<String>) -> Result<()> {
    let profile = Profile::detect();
    let build_dir = get_build_dir();
    let crate_name = get_crate_name(project_cargo_toml)?;

    let project_dir = build_dir.join(&crate_name);
    fs::create_dir_all(&project_dir)?;

    let bins_to_build = match bin_name {
        Some(name) => vec![name],
        None => get_bin_targets(project_cargo_toml)?,
    };

    if bins_to_build.is_empty() {
        anyhow::bail!("No binary targets found in Cargo.toml");
    }

    let target_dir = project_dir.join("target");
    build_elf(project_cargo_toml, &target_dir, &profile, &bins_to_build)?;

    // Link each ELF to PolkaVM
    let elf_dir = target_dir
        .join("riscv64emac-unknown-none-polkavm")
        .join(profile.directory());

    for bin in &bins_to_build {
        let elf_path = elf_dir.join(bin);
        if !elf_path.exists() {
            anyhow::bail!("ELF binary not found at: {}", elf_path.display());
        }

        let output_path = project_dir.join(format!("{}.polkavm", bin));
        link_to_polkavm(&elf_path, &output_path)?;
    }

    Ok(())
}

/// Build the ELF binary using cargo.
fn build_elf(
    manifest_path: &Path,
    target_dir: &Path,
    profile: &Profile,
    bins: &[String],
) -> Result<()> {
    let immediate_abort = check_immediate_abort_support()?;

    let rustflags = if immediate_abort {
        "-Zunstable-options -Cpanic=immediate-abort"
    } else {
        ""
    };

    let mut args = polkavm_linker::TargetJsonArgs::default();
    args.is_64_bit = true;
    let target_json = polkavm_linker::target_json_path(args)
        .map_err(|e| anyhow::anyhow!("Failed to get target JSON: {e}"))?;

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let work_dir = manifest_path.parent().context("Invalid manifest path")?;

    let mut cmd = Command::new(&cargo);
    cmd.current_dir(work_dir)
        .env_remove("CARGO_ENCODED_RUSTFLAGS") // We set RUSTFLAGS, but cargo prefers this one
        .env_remove("RUSTC") // Prevent host toolchain override from build.rs
        .env("RUSTFLAGS", rustflags)
        .env("CARGO_TARGET_DIR", target_dir)
        // Disable strip during ELF build - it conflicts with --emit-relocs required by PolkaVM.
        // Stripping is done later by polkavm_linker after processing relocations.
        .env("CARGO_PROFILE_RELEASE_STRIP", "false")
        .env("RUSTC_BOOTSTRAP", "1")
        .env(INTERNAL_BUILD_ENV, "1")
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("--profile")
        .arg(profile.cargo_arg())
        .arg("--target")
        .arg(&target_json)
        .arg("-Zbuild-std=core,alloc");

    if immediate_abort {
        cmd.arg("-Zbuild-std-features=panic_immediate_abort");
    }

    for bin in bins {
        cmd.arg("--bin").arg(bin);
    }

    eprintln!("Building PolkaVM binary with profile: {:?}", profile);

    let output = cmd.output().context("Failed to execute cargo build")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Cargo build failed:\n{}", stderr);
    }

    Ok(())
}

/// Check if rustc supports immediate abort (>= 1.92).
fn check_immediate_abort_support() -> Result<bool> {
    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .context("Failed to run rustc --version")?;

    let version_str = String::from_utf8(output.stdout).context("Invalid rustc version output")?;

    let version = version_str
        .split_whitespace()
        .nth(1)
        .context("Unexpected rustc version format")?;

    let mut parts = version.split('.');
    let major: u32 = parts
        .next()
        .context("Missing major version")?
        .parse()
        .context("Invalid major version")?;
    let minor: u32 = parts
        .next()
        .context("Missing minor version")?
        .parse()
        .context("Invalid minor version")?;

    Ok(major > 1 || (major == 1 && minor >= 92))
}

/// Link an ELF binary to PolkaVM bytecode.
fn link_to_polkavm(elf_path: &Path, output_path: &Path) -> Result<()> {
    let elf_bytes = fs::read(elf_path)
        .with_context(|| format!("Failed to read ELF from {}", elf_path.display()))?;

    let mut config = polkavm_linker::Config::default();
    config.set_strip(true);
    config.set_optimize(true);

    let linked = polkavm_linker::program_from_elf(
        config,
        polkavm_linker::TargetInstructionSet::ReviveV1,
        &elf_bytes,
    )
    .map_err(|e| anyhow::anyhow!("Failed to link PolkaVM program: {e}"))?;

    fs::write(output_path, &linked)
        .with_context(|| format!("Failed to write PolkaVM bytecode to {}", output_path.display()))?;

    eprintln!(
        "Created PolkaVM binary: {} ({} bytes)",
        output_path.display(),
        linked.len()
    );

    Ok(())
}
