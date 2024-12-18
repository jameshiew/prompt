use std::path::PathBuf;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::{generate, Shell};
use prompt::run;
use prompt::settings::Settings;
use tracing_subscriber::EnvFilter;

const BINARY_NAME: &str = "prompt";

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[arg(long, value_name = "SHELL", help = "Generate shell completions")]
    completions: Option<Shell>,

    #[arg(help = "Path to the file or directory to read into a prompt")]
    path: Option<PathBuf>,
    #[arg(long, help = "Print prompt to stdout without any summary")]
    stdout: bool,
    #[arg(
        long,
        value_name = "COUNT",
        help = "List top files by token count",
        default_missing_value = "10",
        num_args = 0..=1
    )]
    top: Option<u32>,
    #[arg(
        short,
        long,
        value_name = "PATTERNS",
        value_delimiter = ',',
        help = "Glob patterns to exclude from the prompt, separated by commas"
    )]
    exclude: Vec<glob::Pattern>,
}

impl From<Cli> for Settings {
    fn from(value: Cli) -> Self {
        Self {
            path: value.path.unwrap_or_else(|| PathBuf::from(".")),
            stdout: value.stdout,
            top: value.top,
            exclude: value.exclude,
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");

    let cli = Cli::parse();

    if let Some(shell) = cli.completions {
        let mut cmd = Cli::command();
        generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
        return Ok(());
    }

    run::start(cli.into())
}
