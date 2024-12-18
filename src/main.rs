use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use prompt::run::{self, walk_files};
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

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();

    let Some((first_path, rest_paths)) = cli.paths.split_first() else {
        bail!("No paths provided")
    };
    let files = walk_files(first_path.clone(), rest_paths.to_vec(), cli.exclude)?;

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
