use anyhow::{Context, Result};
use askama::Template;
use serde::Deserialize;
use std::io::Write;
use std::{fs, path::PathBuf, process::Command};
use tiny_keccak::{Hasher, Keccak};

// ============================================================================
// Askama Templates
// ============================================================================

#[derive(Template)]
#[template(path = "scaffold/contract_alloc.rs.txt")]
struct ContractAllocTemplate<'a> {
    sol_file_name: &'a str,
    functions: Vec<AllocFunctionInfo>,
}

#[derive(Template)]
#[template(path = "scaffold/contract_no_alloc.rs.txt")]
struct ContractNoAllocTemplate<'a> {
    contract_name_upper: &'a str,
    selectors: Vec<SelectorConst>,
    events: Vec<EventConst>,
    errors: Vec<ErrorConst>,
    functions: Vec<NoAllocFunctionInfo>,
}

const BUILDER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Template)]
#[template(path = "scaffold/cargo_toml.txt")]
struct CargoTomlTemplate<'a> {
    contract_name: &'a str,
    use_alloc: bool,
    builder_version: &'a str,
    builder_path: Option<String>,
}

#[derive(Template)]
#[template(path = "scaffold/contract_blank.rs.txt")]
struct ContractBlankTemplate;

#[derive(Template)]
#[template(path = "scaffold/build.rs.txt")]
struct BuildRsTemplate;

// ============================================================================
// Template Data Structures
// ============================================================================

struct AllocFunctionInfo {
    name: String,
    name_snake: String,
    call_type: String,
}

struct SelectorConst {
    const_name: String,
    bytes_hex: String,
    signature: String,
}

struct EventConst {
    const_name: String,
    bytes_hex: String,
    signature: String,
}

struct ErrorConst {
    const_name: String,
    bytes_hex: String,
    signature: String,
}

struct NoAllocFunctionInfo {
    name: String,
    selector_const: String,
    min_call_data_len: usize,
    params: Vec<ParamDecode>,
}

struct ParamDecode {
    decode_line: String,
}

// ============================================================================
// Solc Output Parsing Structures
// ============================================================================

#[derive(Debug, Deserialize)]
struct SolcOutput {
    contracts: std::collections::HashMap<String, std::collections::HashMap<String, ContractInfo>>,
}

#[derive(Debug, Deserialize)]
struct ContractInfo {
    metadata: String,
}

#[derive(Debug, Deserialize)]
struct ContractMetadata {
    output: MetadataOutput,
}

#[derive(Debug, Deserialize)]
struct MetadataOutput {
    abi: Vec<AbiItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "type")]
enum AbiItem {
    #[serde(rename = "function")]
    Function {
        name: String,
        inputs: Vec<AbiInput>,
        #[allow(dead_code)]
        outputs: Vec<AbiOutput>,
        #[serde(rename = "stateMutability")]
        #[allow(dead_code)]
        state_mutability: String,
    },
    #[serde(rename = "event")]
    Event { name: String, inputs: Vec<AbiInput> },
    #[serde(rename = "error")]
    Error { name: String, inputs: Vec<AbiInput> },
    #[serde(rename = "constructor")]
    Constructor {
        #[allow(dead_code)]
        inputs: Vec<AbiInput>,
    },
}

#[derive(Debug, Deserialize, Clone)]
struct AbiInput {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
    #[allow(dead_code)]
    indexed: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
struct AbiOutput {
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Compute the keccak256 hash of a string
fn keccak256(input: &str) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    let mut output = [0u8; 32];
    hasher.update(input.as_bytes());
    hasher.finalize(&mut output);
    output
}

/// Compute the 4-byte function selector from a function signature
fn compute_selector(signature: &str) -> [u8; 4] {
    let hash = keccak256(signature);
    [hash[0], hash[1], hash[2], hash[3]]
}

/// Build a function signature from name and input types
fn build_function_signature(name: &str, inputs: &[AbiInput]) -> String {
    let types: Vec<&str> = inputs.iter().map(|i| i.type_name.as_str()).collect();
    format!("{}({})", name, types.join(","))
}

/// Format a byte array as Rust hex literal
fn format_bytes_as_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("0x{:02x}", b))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Format a 32-byte array with line breaks for readability
fn format_bytes32_multiline(bytes: &[u8; 32]) -> String {
    bytes
        .chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .map(|b| format!("0x{:02x}", b))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect::<Vec<_>>()
        .join(",\n    ")
}

fn to_pascal_case(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap());
    }
    result
}

fn to_screaming_snake_case(s: &str) -> String {
    to_snake_case(s).to_uppercase()
}

// ============================================================================
// Public API
// ============================================================================

pub fn init_blank_contract(contract_name: &str) -> Result<()> {
    let target_dir = std::env::current_dir()?.join(contract_name);
    if target_dir.exists() {
        anyhow::bail!("Directory already exists: {target_dir:?}");
    }

    fs::create_dir(&target_dir)
        .with_context(|| format!("Failed to create directory: {target_dir:?}"))?;

    let cargo_config_dir = target_dir.join(".cargo");
    fs::create_dir(&cargo_config_dir)?;
    fs::write(
        cargo_config_dir.join("config.toml"),
        r#"[build]
target = "riscv64imac-unknown-none-elf"
"#,
    )?;

    fs::write(target_dir.join(".gitignore"), "/target\n*.polkavm\n")?;

    fs::create_dir(target_dir.join("src"))?;
    let lib_rs_content = generate_blank_contract()?;
    fs::write(target_dir.join("src/lib.rs"), lib_rs_content)?;

    let build_rs_content = generate_build_rs()?;
    fs::write(target_dir.join("build.rs"), build_rs_content)?;

    let cargo_toml_content = generate_cargo_toml(contract_name, false)?;
    fs::write(target_dir.join("Cargo.toml"), cargo_toml_content)?;

    println!("Successfully initialized blank contract project: {target_dir:?}");
    println!("\nNext steps:");
    println!("  cd {contract_name}");
    println!("  cargo build");
    Ok(())
}

pub fn init_from_solidity_file(sol_file: &str, contract_name: &str, use_alloc: bool) -> Result<()> {
    let sol_path = PathBuf::from(sol_file);
    if !sol_path.exists() {
        anyhow::bail!("Solidity file not found: {sol_file}");
    }

    // Get absolute path to .sol file
    let sol_abs_path = sol_path
        .canonicalize()
        .with_context(|| format!("Failed to get absolute path for {sol_file}"))?;

    let sol_file_name = sol_path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;

    log::debug!("Extracting metadata from {sol_file}");
    let (metadata, actual_contract_name) = extract_solc_metadata(&sol_abs_path, sol_file_name)?;

    // Create project directory
    let target_dir = std::env::current_dir()?.join(contract_name);
    if target_dir.exists() {
        anyhow::bail!("Directory already exists: {target_dir:?}");
    }
    fs::create_dir(&target_dir)
        .with_context(|| format!("Failed to create directory: {target_dir:?}"))?;

    // Copy .sol file to project
    let target_sol_path = target_dir.join(sol_file_name);
    fs::copy(&sol_abs_path, &target_sol_path)
        .with_context(|| format!("Failed to copy {sol_file} to {target_sol_path:?}"))?;

    // Create .cargo directory and config
    let cargo_config_dir = target_dir.join(".cargo");
    fs::create_dir(&cargo_config_dir)?;
    fs::write(
        cargo_config_dir.join("config.toml"),
        r#"[build]
target = "riscv64imac-unknown-none-elf"
"#,
    )?;

    // Create .gitignore
    fs::write(target_dir.join(".gitignore"), "/target\n*.polkavm\n")?;

    // Generate src/lib.rs
    fs::create_dir(target_dir.join("src"))?;

    let lib_rs_content = if use_alloc {
        generate_rust_code_alloc(sol_file_name, &metadata, &actual_contract_name)?
    } else {
        generate_rust_code_no_alloc(&metadata, &actual_contract_name)?
    };
    fs::write(target_dir.join("src/lib.rs"), lib_rs_content)?;

    let build_rs_content = generate_build_rs()?;
    fs::write(target_dir.join("build.rs"), build_rs_content)?;

    // Create Cargo.toml
    let cargo_toml_content = generate_cargo_toml(contract_name, use_alloc)?;
    fs::write(target_dir.join("Cargo.toml"), cargo_toml_content)?;

    println!("Successfully initialized contract project from {sol_file}: {target_dir:?}");
    println!("\nNext steps:");
    println!("  cd {contract_name}");
    println!("  cargo build");
    Ok(())
}

// ============================================================================
// Internal Functions
// ============================================================================

fn extract_solc_metadata(
    sol_abs_path: &PathBuf,
    sol_file_name: &str,
) -> Result<(ContractMetadata, String)> {
    // Read the Solidity source code
    let sol_content = fs::read_to_string(sol_abs_path)
        .with_context(|| format!("Failed to read Solidity file: {sol_abs_path:?}"))?;

    let solc_input = serde_json::json!({
        "language": "Solidity",
        "sources": {
            sol_file_name: {
                "content": sol_content
            }
        },
        "settings": {
            "outputSelection": {
                "*": {
                    "*": ["metadata"]
                }
            }
        }
    });

    let solc_input_str = serde_json::to_string(&solc_input)?;

    let mut child = Command::new("solc")
        .arg("--standard-json")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn solc. Make sure solc is installed and in PATH.")?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Failed to open stdin"))?
        .write_all(solc_input_str.as_bytes())?;

    let output_result = child
        .wait_with_output()
        .context("Failed to wait for solc")?;

    if !output_result.status.success() {
        let stderr = String::from_utf8_lossy(&output_result.stderr);
        anyhow::bail!("solc failed: {stderr}");
    }

    log::debug!(
        "solc stdout: {}",
        String::from_utf8_lossy(&output_result.stdout)
    );

    let solc_output: SolcOutput =
        serde_json::from_slice(&output_result.stdout).with_context(|| {
            format!(
                "Failed to parse solc output. Output was: {}",
                String::from_utf8_lossy(&output_result.stdout)
            )
        })?;

    // Extract metadata from the first contract
    let contracts_for_file = solc_output
        .contracts
        .get(sol_file_name)
        .ok_or_else(|| anyhow::anyhow!("No contract found in solc output"))?;

    let (contract_name, contract_info) = contracts_for_file
        .iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No contract found in solc output"))?;

    let metadata: ContractMetadata = serde_json::from_str(&contract_info.metadata)
        .context("Failed to parse contract metadata")?;

    Ok((metadata, contract_name.clone()))
}

fn generate_blank_contract() -> Result<String> {
    ContractBlankTemplate
        .render()
        .context("Failed to render blank contract template")
}

fn generate_build_rs() -> Result<String> {
    BuildRsTemplate
        .render()
        .context("Failed to render build.rs template")
}

fn generate_rust_code_alloc(
    sol_file_name: &str,
    metadata: &ContractMetadata,
    contract_name: &str,
) -> Result<String> {
    let contract_name_pascal = to_pascal_case(contract_name);

    let functions: Vec<AllocFunctionInfo> = metadata
        .output
        .abi
        .iter()
        .filter_map(|item| match item {
            AbiItem::Function { name, .. } => Some(AllocFunctionInfo {
                name: name.clone(),
                name_snake: to_snake_case(name),
                call_type: format!("{contract_name_pascal}::{name}Call"),
            }),
            _ => None,
        })
        .collect();

    let template = ContractAllocTemplate {
        sol_file_name,
        functions,
    };

    template.render().context("Failed to render alloc template")
}

fn generate_rust_code_no_alloc(metadata: &ContractMetadata, contract_name: &str) -> Result<String> {
    let contract_name_upper = contract_name.to_uppercase();

    // Collect function selectors
    let mut selectors = Vec::new();
    let mut functions = Vec::new();

    for item in &metadata.output.abi {
        if let AbiItem::Function { name, inputs, .. } = item {
            let signature = build_function_signature(name, inputs);
            let selector = compute_selector(&signature);
            let const_name = format!("{}_SELECTOR", to_screaming_snake_case(name));

            selectors.push(SelectorConst {
                const_name: const_name.clone(),
                bytes_hex: format_bytes_as_hex(&selector),
                signature: signature.clone(),
            });

            // Generate decode params
            let mut params = Vec::new();
            let mut offset = 4usize;

            for (idx, input) in inputs.iter().enumerate() {
                let param_name = if input.name.is_empty() {
                    format!("param_{}", idx)
                } else {
                    to_snake_case(&input.name)
                };

                let decode_line = match input.type_name.as_str() {
                    "address" => {
                        format!(
                            "let {param_name} = decode_address(&call_data[{offset}..{}]);",
                            offset + 32
                        )
                    }
                    "uint256" | "uint128" | "uint64" | "uint32" | "uint16" | "uint8" => {
                        format!(
                            "let {param_name} = decode_u128(&call_data[{offset}..{}]);",
                            offset + 32
                        )
                    }
                    "bool" => {
                        format!("let {param_name} = call_data[{}] != 0;", offset + 31)
                    }
                    "bytes32" => {
                        format!(
                            "let {param_name}: [u8; 32] = call_data[{offset}..{}].try_into().unwrap();",
                            offset + 32
                        )
                    }
                    _ => {
                        format!("// TODO: decode {param_name} of type {}", input.type_name)
                    }
                };

                params.push(ParamDecode { decode_line });
                offset += 32;
            }

            functions.push(NoAllocFunctionInfo {
                name: name.clone(),
                selector_const: const_name,
                min_call_data_len: 4 + inputs.len() * 32,
                params,
            });
        }
    }

    // Collect events
    let events: Vec<EventConst> = metadata
        .output
        .abi
        .iter()
        .filter_map(|item| {
            if let AbiItem::Event { name, inputs } = item {
                let signature = build_function_signature(name, inputs);
                let hash = keccak256(&signature);
                Some(EventConst {
                    const_name: format!("{}_EVENT_SIGNATURE", to_screaming_snake_case(name)),
                    bytes_hex: format_bytes32_multiline(&hash),
                    signature,
                })
            } else {
                None
            }
        })
        .collect();

    // Collect errors
    let errors: Vec<ErrorConst> = metadata
        .output
        .abi
        .iter()
        .filter_map(|item| {
            if let AbiItem::Error { name, inputs } = item {
                let signature = build_function_signature(name, inputs);
                let selector = compute_selector(&signature);
                Some(ErrorConst {
                    const_name: format!("{}_ERROR", to_screaming_snake_case(name)),
                    bytes_hex: format_bytes_as_hex(&selector),
                    signature,
                })
            } else {
                None
            }
        })
        .collect();

    let template = ContractNoAllocTemplate {
        contract_name_upper: &contract_name_upper,
        selectors,
        events,
        errors,
        functions,
    };

    template
        .render()
        .context("Failed to render no-alloc template")
}

fn generate_cargo_toml(contract_name: &str, use_alloc: bool) -> Result<String> {
    let builder_path = std::env::var("CARGO_PVM_CONTRACT_BUILDER_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty());

    if let Some(ref path) = builder_path {
        let path = std::path::Path::new(path);
        if !path.exists() {
            anyhow::bail!("Builder path does not exist: {}", path.display());
        }
    }

    let template = CargoTomlTemplate {
        contract_name,
        use_alloc,
        builder_version: BUILDER_VERSION,
        builder_path,
    };
    template
        .render()
        .context("Failed to render Cargo.toml template")
}
