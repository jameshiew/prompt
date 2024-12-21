use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use config::Config;
use prompt::config::{find_config_path, PromptConfig};
use prompt::run::{self, CountTokenOptions};
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

    #[command(flatten)]
    output: OutputOptions,
}

#[derive(Debug, Args)]
struct OutputOptions {
    #[arg(long, help = "Print prompt to stdout instead of copying to clipboard")]
    stdout: bool,
    #[arg(long, help = "Don't print summary to stdout")]
    no_summary: bool,
    #[arg(
        long,
        value_name = "OPTION",
        value_enum,
        default_value_t = CountTokenOptions::default(),
        help = "Token count nothing, the final output or also all individual files"
    )]
    count_tokens: CountTokenOptions,
}

#[derive(Debug, Default, Subcommand, Clone)]
enum Command {
    /// (default) Output a prompt that includes matching files (copies to clipboard by default)
    #[default]
    Output,
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
        unreachable!("should have at least one path by default");
    };
    let first_path = first_path.to_owned();
    let rest_paths = rest_paths.to_vec();

    let command = cli.command.unwrap_or_default();
    match command {
        Command::Output => {
            run::output(
                first_path,
                rest_paths,
                cli.exclude,
                cli.output.stdout,
                cli.output.no_summary,
                cli.output.count_tokens,
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
