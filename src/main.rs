use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use prompt::run::{self, Format, TokenCountOptions};
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
        default_value = ".",
        help = "Paths to the files/directories for reading into a prompt",
    )]
    paths: Vec<PathBuf>,
    #[arg(
        short,
        long,
        global = true,
        num_args = 1..,
        value_name = "PATTERN",
        help = "Glob patterns to exclude from the prompt, separated by commas",
    )]
    exclude: Vec<glob::Pattern>,
    #[arg(short, long, global = true, value_enum, default_value_t = Format::default(), help = "Output format")]
    format: Format,
    #[command(flatten)]
    output: OutputOptions,
}

// default - prompt clip, summary stdout
// prompt stdout, summary NO
// prompt stdout, summary stdout
//
#[derive(Debug, Args)]
struct OutputOptions {
    #[arg(
        long,
        help = "Print prompt to stdout with no summary instead of copying to clipboard"
    )]
    stdout: bool,
    #[arg(
        long,
        value_name = "OPTION",
        value_enum,
        default_value_t = TokenCountOptions::default(),
        default_missing_value = "all",
        num_args = 0..=1,
        help = "Token count nothing, the final output or also all individual files"
    )]
    token_count: TokenCountOptions,
}

#[derive(Debug, Default, Subcommand, Clone)]
enum Command {
    /// (default) Generate a prompt that includes matching files (copies to clipboard by default)
    #[default]
    Generate,
    /// Generate shell completions
    ShellCompletions {
        #[arg()]
        shell: Shell,
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();

    let Some((first_path, rest_paths)) = cli.paths.split_first() else {
        unreachable!("should have at least one path by default");
    };
    let first_path = first_path.to_owned();
    let rest_paths = rest_paths.to_vec();

    let command = cli.command.unwrap_or_default();
    match command {
        Command::Generate => {
            run::generate(
                first_path,
                rest_paths,
                cli.exclude,
                cli.output.stdout,
                cli.output.token_count,
                cli.format,
            )
            .await
        }
        Command::ShellCompletions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
            Ok(())
        }
        Command::Count { top } => run::count(first_path, rest_paths, cli.exclude, top).await,
    }
}
