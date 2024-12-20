use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use config::Config;
use prompt::config::{find_config_path, PromptConfig};
use prompt::discovery::discover;
use prompt::files::Files;
use prompt::run::{self};
use serde::Deserialize;
use tracing_subscriber::EnvFilter;

const BINARY_NAME: &str = "prompt";

#[derive(Parser)]
#[command(version, subcommand_required = false)]
struct Cli {
    #[clap(subcommand)]
    command: Option<Command>,
    #[arg(
        short,
        long,
        global = true,
        num_args = 1..,
        value_name = "PATH",
        help = "Paths to the files/directories for reading into a prompt",
        default_value = "."
    )]
    paths: Vec<PathBuf>,
    #[arg(
        short,
        long,
        global = true,
        num_args = 1..,
        value_name = "PATTERN",
        help = "Glob patterns to exclude from the prompt, separated by commas"
    )]
    exclude: Vec<glob::Pattern>,
}

#[derive(Debug, Subcommand, Clone)]
enum Command {
    /// Generate shell completions
    ShellCompletions {
        #[arg()]
        shell: Shell,
    },
    /// Output a prompt that includes matching files (copies to clipboard by default)
    Output {
        #[arg(long, help = "Print prompt to stdout without any summary")]
        stdout: bool,
    },
    /// Count tokens from matching files
    Count {
        #[arg(
        long,
        value_name = "COUNT",
        help = "List top files by token count",
        default_missing_value = "10",
        num_args = 0..=1
    )]
        top: Option<u32>,
    },
}

impl Default for Command {
    fn default() -> Self {
        Command::Output { stdout: false }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cwd = std::env::current_dir()?;
    let _cfg = match find_config_path(&cwd) {
        Some(cfg_path) => {
            let cfg = Config::builder()
                .add_source(config::File::with_name(&cfg_path.to_string_lossy()))
                .build()?;
            let cfg = PromptConfig::deserialize(cfg)?;
            tracing::debug!(?cfg, "Loaded config");
            cfg
        }
        None => PromptConfig::default(),
    };

    let cli = Cli::parse();

    let Some((first_path, rest_paths)) = cli.paths.split_first() else {
        bail!("No paths provided")
    };
    let discovered = discover(first_path.clone(), rest_paths.to_vec(), cli.exclude)?;
    let files = Files::read_from(discovered).await?;

    let command = cli.command.unwrap_or_default();
    match command {
        Command::ShellCompletions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
            Ok(())
        }
        Command::Output { stdout } => run::output(files, stdout),
        Command::Count { top } => run::count(files, top),
    }
}
