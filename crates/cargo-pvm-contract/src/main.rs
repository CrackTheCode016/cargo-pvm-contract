use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use include_dir::{Dir, include_dir};
use inquire::{Select, Text};
use log::debug;
use std::{
    fs,
    path::{Path, PathBuf},
};

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
    #[arg(long, value_enum)]
    init_type: Option<InitType>,
    #[arg(long)]
    example: Option<String>,
    #[arg(long, value_enum)]
    memory_model: Option<MemoryModel>,
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    sol_file: Option<PathBuf>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExampleContract {
    name: String,
    filename: String,
}

impl ExampleContract {
    fn from_path(path: &Path) -> Option<Self> {
        if path.extension().and_then(|ext| ext.to_str()) != Some("sol") {
            return None;
        }

        let filename = path.file_name()?.to_str()?.to_string();
        let name = path.file_stem()?.to_str()?.to_string();
        Some(Self { name, filename })
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        let name = self.name.to_ascii_lowercase();
        let filename = self.filename.to_ascii_lowercase();
        query == name || query == filename
    }
}

impl std::fmt::Display for ExampleContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

fn load_examples() -> Result<Vec<ExampleContract>> {
    let examples_dir = TEMPLATES_DIR
        .get_dir("examples")
        .ok_or_else(|| anyhow::anyhow!("Examples directory not found in templates"))?;
    let mut examples: Vec<ExampleContract> = examples_dir
        .files()
        .filter_map(|file| ExampleContract::from_path(file.path()))
        .collect();

    examples.sort_by(|left, right| left.name.cmp(&right.name));

    if examples.is_empty() {
        anyhow::bail!("No example contracts found in templates/examples");
    }

    Ok(examples)
}

fn find_example(examples: &[ExampleContract], query: &str) -> Result<ExampleContract> {
    examples
        .iter()
        .find(|example| example.matches(query))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Unknown example: {query}"))
}

fn main() -> Result<()> {
    env_logger::init();

    let Cli { command } = Cli::parse();
    match command {
        Commands::PvmContract(args) => init_command(args),
    }
}

fn init_command(args: PvmContractArgs) -> Result<()> {
    // Get init_type from args or prompt
    let init_type = match args.init_type {
        Some(t) => t,
        None => {
            let init_types = vec![InitType::SolidityFile, InitType::Example, InitType::Blank];
            Select::new("How do you want to initialize the project?", init_types)
                .prompt()
                .context("Failed to get initialization type")?
        }
    };

    match init_type {
        InitType::Blank => {
            let contract_name = prompt_name(args.name, None)?;
            check_dir_exists(&contract_name)?;
            debug!("Initializing blank contract: {contract_name}");
            scaffold::init_blank_contract(&contract_name)
        }
        InitType::Example => {
            let examples = load_examples()?;

            // Get example from args or prompt
            let example = match args.example {
                Some(example_name) => find_example(&examples, &example_name)?,
                None => Select::new("Select an example:", examples)
                    .prompt()
                    .context("Failed to get example choice")?,
            };

            let memory_model = prompt_memory_model(args.memory_model)?;
            let contract_name = prompt_name(args.name, Some(&example.name))?;

            check_dir_exists(&contract_name)?;
            debug!(
                "Initializing from example: {} with memory model: {:?}",
                example.filename, memory_model
            );

            init_from_example(&example, &contract_name, memory_model)
        }
        InitType::SolidityFile => {
            // Get sol_file from args or prompt
            let sol_path = match args.sol_file {
                Some(path) => path,
                None => {
                    let sol_file = Text::new("Enter path to your .sol file:")
                        .with_help_message("Path to a Solidity interface file")
                        .prompt()
                        .context("Failed to get .sol file path")?;

                    if sol_file.is_empty() {
                        anyhow::bail!("Solidity file path cannot be empty");
                    }
                    PathBuf::from(sol_file)
                }
            };

            if !sol_path.exists() {
                anyhow::bail!("Solidity file not found: {}", sol_path.display());
            }

            let default_name = sol_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("contract")
                .to_string();

            let memory_model = prompt_memory_model(args.memory_model)?;
            let contract_name = prompt_name(args.name, Some(&default_name))?;

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
            scaffold::init_from_solidity_file(sol_file, &contract_name, use_alloc)
        }
    }
}

fn prompt_memory_model(arg: Option<MemoryModel>) -> Result<MemoryModel> {
    match arg {
        Some(m) => Ok(m),
        None => {
            let memory_models = vec![MemoryModel::AllocWithAlloy, MemoryModel::NoAlloc];
            Select::new("Which memory model do you want to use?", memory_models)
                .prompt()
                .context("Failed to get memory model choice")
        }
    }
}

fn prompt_name(arg: Option<String>, default: Option<&str>) -> Result<String> {
    let contract_name = match arg {
        Some(name) => name,
        None => {
            let mut prompt = Text::new("What is your contract name?")
                .with_help_message("This will be the name of the project directory");
            if let Some(d) = default {
                prompt = prompt.with_default(d);
            }
            prompt.prompt().context("Failed to get contract name")?
        }
    };

    if contract_name.is_empty() {
        anyhow::bail!("Contract name cannot be empty");
    }

    Ok(contract_name)
}

fn init_from_example(
    example: &ExampleContract,
    contract_name: &str,
    memory_model: MemoryModel,
) -> Result<()> {
    // Get the embedded example .sol file
    let example_path = format!("examples/{}", example.filename);
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
        format!("Failed to create temporary directory for example: {example_temp_dir:?}")
    })?;
    let temp_sol_path = example_temp_dir.join(example.filename.as_str());
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
