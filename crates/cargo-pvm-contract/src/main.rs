use anyhow::{Context, Result};
use clap::Parser;
use include_dir::{include_dir, Dir};
use inquire::{Select, Text};
use log::debug;
use std::{fs, path::PathBuf};

mod scaffold;

// Embed the templates directory into the binary
static TEMPLATES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// Initialize contract projects for PolkaVM
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct PvmContractArgs;

#[derive(Debug, Clone, Copy, PartialEq)]
enum InitType {
    SolidityFile,
    Example,
    Blank,
}

impl std::fmt::Display for InitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitType::SolidityFile => write!(f, "From a Solidity interface file (.sol)"),
            InitType::Example => write!(f, "From an example contract"),
            InitType::Blank => write!(f, "Blank (empty contract)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MemoryModel {
    AllocWithAlloy,
    NoAlloc,
}

impl std::fmt::Display for MemoryModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryModel::AllocWithAlloy => {
                write!(f, "alloy-core + allocator (easier API, larger binary)")
            }
            MemoryModel::NoAlloc => write!(f, "No allocator (manual encoding, smaller binary)"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ExampleChoice {
    MyToken,
}

impl std::fmt::Display for ExampleChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExampleChoice::MyToken => write!(f, "MyToken (ERC20-like token)"),
        }
    }
}

impl ExampleChoice {
    fn sol_filename(&self) -> &'static str {
        match self {
            ExampleChoice::MyToken => "MyToken.sol",
        }
    }

    fn default_name(&self) -> &'static str {
        match self {
            ExampleChoice::MyToken => "MyToken",
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();

    PvmContractArgs::parse();
    init_command()
}

fn init_command() -> Result<()> {
    // First, prompt for initialization type
    let init_types = vec![InitType::SolidityFile, InitType::Example, InitType::Blank];
    let init_type = Select::new("How do you want to initialize the project?", init_types)
        .prompt()
        .context("Failed to get initialization type")?;

    match init_type {
        InitType::Blank => {
            // Ask for name without prefill
            let contract_name = Text::new("What is your contract name?")
                .with_help_message("This will be the name of the project directory")
                .prompt()
                .context("Failed to get contract name")?;

            if contract_name.is_empty() {
                anyhow::bail!("Contract name cannot be empty");
            }

            check_dir_exists(&contract_name)?;
            debug!("Initializing blank contract: {contract_name}");
            scaffold::init_blank_contract(&contract_name)
        }
        InitType::Example => {
            // Prompt for example choice
            let examples = vec![ExampleChoice::MyToken];
            let example = Select::new("Select an example:", examples)
                .prompt()
                .context("Failed to get example choice")?;

            // Prompt for memory model
            let memory_models = vec![MemoryModel::AllocWithAlloy, MemoryModel::NoAlloc];
            let memory_model = Select::new("Which memory model do you want to use?", memory_models)
                .prompt()
                .context("Failed to get memory model choice")?;

            // Ask for name with example name as default
            let contract_name = Text::new("What is your contract name?")
                .with_default(example.default_name())
                .with_help_message("This will be the name of the project directory")
                .prompt()
                .context("Failed to get contract name")?;

            if contract_name.is_empty() {
                anyhow::bail!("Contract name cannot be empty");
            }

            check_dir_exists(&contract_name)?;
            debug!(
                "Initializing from example: {} with memory model: {:?}",
                example.sol_filename(),
                memory_model
            );

            init_from_example(example, &contract_name, memory_model)
        }
        InitType::SolidityFile => {
            // Prompt for .sol file path
            let sol_file = Text::new("Enter path to your .sol file:")
                .with_help_message("Path to a Solidity interface file")
                .prompt()
                .context("Failed to get .sol file path")?;

            if sol_file.is_empty() {
                anyhow::bail!("Solidity file path cannot be empty");
            }

            // Verify file exists
            let sol_path = PathBuf::from(&sol_file);
            if !sol_path.exists() {
                anyhow::bail!("Solidity file not found: {sol_file}");
            }

            // Extract default name from .sol filename
            let default_name = sol_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("contract")
                .to_string();

            // Prompt for memory model
            let memory_models = vec![MemoryModel::AllocWithAlloy, MemoryModel::NoAlloc];
            let memory_model = Select::new("Which memory model do you want to use?", memory_models)
                .prompt()
                .context("Failed to get memory model choice")?;

            // Ask for name with .sol filename as default
            let contract_name = Text::new("What is your contract name?")
                .with_default(&default_name)
                .with_help_message("This will be the name of the project directory")
                .prompt()
                .context("Failed to get contract name")?;

            if contract_name.is_empty() {
                anyhow::bail!("Contract name cannot be empty");
            }

            check_dir_exists(&contract_name)?;
            debug!(
                "Initializing from Solidity file: {sol_file} with memory model: {:?}",
                memory_model
            );

            let use_alloc = memory_model == MemoryModel::AllocWithAlloy;
            scaffold::init_from_solidity_file(&sol_file, &contract_name, use_alloc)
        }
    }
}

fn init_from_example(
    example: ExampleChoice,
    contract_name: &str,
    memory_model: MemoryModel,
) -> Result<()> {
    // Get the embedded example .sol file
    let example_path = format!("examples/{}", example.sol_filename());
    let example_file = TEMPLATES_DIR
        .get_file(&example_path)
        .ok_or_else(|| anyhow::anyhow!("Example file not found: {}", example_path))?;

    // Write to a temporary file
    let temp_dir = std::env::temp_dir();
    let temp_sol_path = temp_dir.join(example.sol_filename());
    fs::write(&temp_sol_path, example_file.contents())
        .with_context(|| format!("Failed to write temporary .sol file: {:?}", temp_sol_path))?;

    let use_alloc = memory_model == MemoryModel::AllocWithAlloy;

    // Use scaffold to initialize from the temp file
    let result = scaffold::init_from_solidity_file(
        temp_sol_path.to_str().unwrap(),
        contract_name,
        use_alloc,
    );

    // Clean up temp file
    let _ = fs::remove_file(&temp_sol_path);

    result
}

fn check_dir_exists(contract_name: &str) -> Result<()> {
    let target_dir = std::env::current_dir()?.join(contract_name);
    if target_dir.exists() {
        anyhow::bail!("Directory already exists: {target_dir:?}");
    }
    Ok(())
}
