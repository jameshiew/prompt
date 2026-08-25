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

    let command = Cli::parse()
        .try_into_command()
        .unwrap_or_else(|error| error.exit());
    match command {
        Command::Generate(generate) => {
            let (first_path, rest_paths, exclude, no_gitignore) =
                generate.options.files.into_parts();
            run::generate(
                first_path,
                rest_paths,
                exclude,
                no_gitignore,
                generate.stdout,
                generate.options.token_count.unwrap_or_default(),
                generate.options.format.unwrap_or_default(),
            )
            .await
        }
        Command::ShellCompletions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, BINARY_NAME, &mut std::io::stdout());
            Ok(())
        }
        Command::Count(count) => {
            let (first_path, rest_paths, exclude, no_gitignore) = count.files.into_parts();
            run::count(first_path, rest_paths, exclude, no_gitignore, count.top).await
        }
    }
}
