mod cli;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use cli::{Cli, Command};
use prompt::run;
use tracing_subscriber::EnvFilter;

const BINARY_NAME: &str = "prompt";

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
                cli.no_gitignore,
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
        Command::Count { top } => {
            run::count(first_path, rest_paths, cli.exclude, cli.no_gitignore, top).await
        }
    }
}
