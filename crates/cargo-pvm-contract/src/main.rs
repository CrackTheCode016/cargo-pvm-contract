use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use include_dir::{Dir, include_dir};
use inquire::{Select, Text};
use log::debug;
use std::{fs, path::PathBuf};

mod scaffold;

// Embed the templates directory into the binary
static TEMPLATES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

#[derive(Parser, Debug)]
#[command(name = "cargo", bin_name = "cargo", author, version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize contract projects for PolkaVM
    PvmContract(PvmContractArgs),
}

#[derive(Parser, Debug, Default)]
struct PvmContractArgs {
    #[arg(long, value_enum, requires = "non_interactive")]
    init_type: Option<InitType>,
    #[arg(long, value_enum, requires = "non_interactive")]
    example: Option<ExampleChoice>,
    #[arg(long, value_enum, requires = "non_interactive")]
    memory_model: Option<MemoryModel>,
    #[arg(long, requires = "non_interactive")]
    name: Option<String>,
    #[arg(long, requires = "non_interactive")]
    sol_file: Option<PathBuf>,
    #[arg(long)]
    non_interactive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
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

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
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

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
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

    let Cli { command } = Cli::parse();
    match command {
        Commands::PvmContract(args) => init_command(args),
    }
}

fn init_command(args: PvmContractArgs) -> Result<()> {
    let builder_path = std::env::var("CARGO_PVM_CONTRACT_BUILDER_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);

    if let Some(path) = builder_path.as_deref()
        && !path.exists()
    {
        anyhow::bail!("Builder path does not exist: {}", path.display());
    }

    if args.non_interactive {
        init_command_non_interactive(args, builder_path.as_deref())
    } else {
        init_command_interactive(builder_path.as_deref())
    }
}

fn init_command_interactive(builder_path: Option<&std::path::Path>) -> Result<()> {
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
            scaffold::init_blank_contract(&contract_name, builder_path)
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

            init_from_example(example, &contract_name, memory_model, builder_path)
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
            scaffold::init_from_solidity_file(&sol_file, &contract_name, use_alloc, builder_path)
        }
    }
}

fn init_command_non_interactive(
    args: PvmContractArgs,
    builder_path: Option<&std::path::Path>,
) -> Result<()> {
    let init_type = args
        .init_type
        .ok_or_else(|| anyhow::anyhow!("--init-type is required with --non-interactive"))?;

    match init_type {
        InitType::Blank => {
            let contract_name = args
                .name
                .filter(|name| !name.is_empty())
                .ok_or_else(|| anyhow::anyhow!("--name is required for blank initialization"))?;

            check_dir_exists(&contract_name)?;
            debug!("Initializing blank contract: {contract_name}");
            scaffold::init_blank_contract(&contract_name, builder_path)
        }
        InitType::Example => {
            let example = args.example.ok_or_else(|| {
                anyhow::anyhow!("--example is required for example initialization")
            })?;
            let memory_model = args.memory_model.ok_or_else(|| {
                anyhow::anyhow!("--memory-model is required for example initialization")
            })?;
            let contract_name = args
                .name
                .unwrap_or_else(|| example.default_name().to_string());

            if contract_name.is_empty() {
                anyhow::bail!("Contract name cannot be empty");
            }

            check_dir_exists(&contract_name)?;
            debug!(
                "Initializing from example: {} with memory model: {:?}",
                example.sol_filename(),
                memory_model
            );

            init_from_example(example, &contract_name, memory_model, builder_path)
        }
        InitType::SolidityFile => {
            let sol_path = args.sol_file.ok_or_else(|| {
                anyhow::anyhow!("--sol-file is required for Solidity initialization")
            })?;

            if !sol_path.exists() {
                anyhow::bail!("Solidity file not found: {}", sol_path.display());
            }

            let default_name = sol_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("contract")
                .to_string();
            let contract_name = args.name.unwrap_or(default_name);

            if contract_name.is_empty() {
                anyhow::bail!("Contract name cannot be empty");
            }

            let memory_model = args.memory_model.ok_or_else(|| {
                anyhow::anyhow!("--memory-model is required for Solidity initialization")
            })?;

            check_dir_exists(&contract_name)?;
            debug!(
                "Initializing from Solidity file: {} with memory model: {:?}",
                sol_path.display(),
                memory_model
            );

            let sol_file = sol_path.to_str().ok_or_else(|| {
                anyhow::anyhow!("Solidity file path is not valid UTF-8: {:?}", sol_path)
            })?;
            let use_alloc = memory_model == MemoryModel::AllocWithAlloy;
            scaffold::init_from_solidity_file(sol_file, &contract_name, use_alloc, builder_path)
        }
    }
}

fn init_from_example(
    example: ExampleChoice,
    contract_name: &str,
    memory_model: MemoryModel,
    builder_path: Option<&std::path::Path>,
) -> Result<()> {
    // Get the embedded example .sol file
    let example_path = format!("examples/{}", example.sol_filename());
    let example_file = TEMPLATES_DIR
        .get_file(&example_path)
        .ok_or_else(|| anyhow::anyhow!("Example file not found: {}", example_path))?;

    // Write to a temporary file
    let temp_dir = std::env::temp_dir();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("Failed to read system time")?
        .as_nanos();
    let example_temp_dir = temp_dir.join(format!("cargo-pvm-contract-{timestamp}"));
    fs::create_dir_all(&example_temp_dir).with_context(|| {
        format!("Failed to create temporary directory for example: {example_temp_dir:?}",)
    })?;
    let temp_sol_path = example_temp_dir.join(example.sol_filename());
    fs::write(&temp_sol_path, example_file.contents())
        .with_context(|| format!("Failed to write temporary .sol file: {:?}", temp_sol_path))?;

    let use_alloc = memory_model == MemoryModel::AllocWithAlloy;

    // Use scaffold to initialize from the temp file
    let result = scaffold::init_from_solidity_file(
        temp_sol_path.to_str().unwrap(),
        contract_name,
        use_alloc,
        builder_path,
    );

    // Clean up temp file
    let _ = fs::remove_dir_all(&example_temp_dir);

    result
}

fn check_dir_exists(contract_name: &str) -> Result<()> {
    let target_dir = std::env::current_dir()?.join(contract_name);
    if target_dir.exists() {
        anyhow::bail!("Directory already exists: {target_dir:?}");
    }
    Ok(())
}
