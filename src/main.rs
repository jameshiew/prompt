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
    #[arg(long, help = "Copy output straight to the clipboard")]
    copy: bool,
    #[arg(long, value_name = "COUNT", help = "List top files by token count")]
    top: Option<u32>,
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
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));

    run::start(Settings {
        path,
        copy: cli.copy,
        top: cli.top,
    })
}
