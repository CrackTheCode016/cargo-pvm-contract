use anyhow::{Context, Result};
use log::debug;
use std::{env, fs, path::Path, path::PathBuf, process::Command};

pub const INTERNAL_BUILD_ENV: &str = "CARGO_PVM_CONTRACT_INTERNAL";

pub fn build_contract(
    manifest_path: impl AsRef<Path>,
    bin_name: Option<&str>,
    output_dir: impl AsRef<Path>,
) -> Result<PathBuf> {
    let manifest_path = manifest_path.as_ref();
    let bin_name = match bin_name {
        Some(name) => name.to_string(),
        None => resolve_bin_name(manifest_path)?,
    };

    let output_dir = output_dir.as_ref();
    fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {output_dir:?}"))?;

    let target_dir = output_dir
        .parent()
        .context("Output directory must be within the target directory")?;

    let elf_path = build_elf(manifest_path, target_dir, &bin_name)?;
    let output_path = output_dir.join(format!("{bin_name}.polkavm"));
    link_to_polkavm(&elf_path, &output_path)?;

    Ok(output_path)
}

pub fn default_target_dir() -> Result<PathBuf> {
    if let Ok(target_dir) = env::var("CARGO_TARGET_DIR") {
        return Ok(PathBuf::from(target_dir));
    }

    let out_dir =
        PathBuf::from(env::var("OUT_DIR").context("OUT_DIR environment variable not set")?);
    for ancestor in out_dir.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some("target") {
            return Ok(ancestor.to_path_buf());
        }
    }

    anyhow::bail!("Failed to determine target directory from OUT_DIR: {out_dir:?}");
}

pub fn default_output_dir() -> Result<PathBuf> {
    Ok(default_target_dir()?.join("pvm"))
}

fn resolve_bin_name(manifest_path: &Path) -> Result<String> {
    let cargo_toml_content = fs::read_to_string(manifest_path)
        .with_context(|| format!("Failed to read Cargo.toml at {manifest_path:?}"))?;

    let doc = cargo_toml_content
        .parse::<toml_edit::DocumentMut>()
        .context("Failed to parse Cargo.toml")?;

    let first_bin_name = doc
        .get("bin")
        .and_then(|b| b.as_array_of_tables())
        .and_then(|arr| arr.get(0))
        .and_then(|bin| bin.get("name"))
        .and_then(|name| name.as_str())
        .context("No [[bin]] section found in Cargo.toml. Please specify a binary name.")?;

    debug!("Using first binary from Cargo.toml: {first_bin_name}");
    Ok(first_bin_name.to_string())
}

fn build_elf(manifest_path: &Path, build_dir: &Path, bin_name: &str) -> Result<PathBuf> {
    debug!("Building RISC-V ELF binary for binary: {bin_name}");

    // Detect if immediate_abort is supported (rustc >= 1.92)
    let immediate_abort = {
        let out = Command::new("rustc")
            .arg("--version")
            .output()
            .context("rustc --version failed")?;
        let ver = String::from_utf8(out.stdout).context("utf8 from rustc --version failed")?;
        let ver_num = ver
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("unexpected rustc --version output: {ver}"))?;
        let mut parts = ver_num.split('.');
        let major: u32 = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing major version"))?
            .parse()
            .context("invalid major version")?;
        let minor: u32 = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing minor version"))?
            .parse()
            .context("invalid minor version")?;
        major > 1 || (major == 1 && minor >= 92)
    };

    let encoded_rustflags = if immediate_abort {
        ["-Zunstable-options", "-Cpanic=immediate-abort"].join("\x1f")
    } else {
        String::new()
    };

    let mut args = polkavm_linker::TargetJsonArgs::default();
    args.is_64_bit = true;

    let target_json = polkavm_linker::target_json_path(args).map_err(|e| anyhow::anyhow!(e))?;
    let work_dir = manifest_path
        .parent()
        .context("Failed to get manifest directory")?;

    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut build_command = Command::new(cargo);
    build_command
        .current_dir(work_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", encoded_rustflags)
        .env("RUSTC_BOOTSTRAP", "1")
        .env(INTERNAL_BUILD_ENV, "1")
        .args(["build", "--release", "--manifest-path"])
        .arg(manifest_path)
        .arg("-Zbuild-std=core,alloc");

    if immediate_abort {
        build_command.arg("-Zbuild-std-features=panic_immediate_abort");
    }

    build_command.args([
        "--bin",
        bin_name,
        "--target",
        &target_json.to_string_lossy(),
    ]);

    debug!("Running: {build_command:?}");
    let mut child = build_command
        .spawn()
        .context("Failed to execute cargo build")?;

    let status = child.wait().context("Failed to wait for cargo build")?;

    if !status.success() {
        anyhow::bail!("Failed to build binary {bin_name}");
    }

    let elf_path = build_dir
        .join("riscv64emac-unknown-none-polkavm/release")
        .join(bin_name);

    if !elf_path.exists() {
        anyhow::bail!("ELF binary was not generated at: {elf_path:?}");
    }

    Ok(elf_path)
}

fn link_to_polkavm(elf_path: &Path, output_path: &Path) -> Result<()> {
    debug!("Linking to PolkaVM bytecode...");

    let mut config = polkavm_linker::Config::default();
    config.set_strip(true);
    config.set_optimize(true);

    let elf_bytes =
        fs::read(elf_path).with_context(|| format!("Failed to read ELF from {elf_path:?}"))?;

    let linked = polkavm_linker::program_from_elf(
        config,
        polkavm_linker::TargetInstructionSet::ReviveV1,
        &elf_bytes,
    )
    .map_err(|err| anyhow::anyhow!("Failed to link PolkaVM program: {err:?}"))?;

    fs::write(output_path, &linked)
        .with_context(|| format!("Failed to write PolkaVM bytecode to {output_path:?}"))?;

    debug!("Wrote {} bytes to {output_path:?}", linked.len());
    Ok(())
}
