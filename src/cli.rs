use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
use prompt::run::{Format, TokenCountOptions};

#[derive(Parser)]
#[command(
    version,
    subcommand_required = false,
    subcommand_precedence_over_arg = true
)]
pub struct Cli {
    #[clap(subcommand)]
    pub(crate) command: Option<Command>,
    #[arg(
        short,
        long,
        global = true,
        num_args = 1..,
        value_name = "PATH",
        default_value = ".",
        help = "Paths to the files/directories for reading into a prompt",
    )]
    pub(crate) paths: Vec<PathBuf>,
    #[arg(
        short,
        long,
        global = true,
        num_args = 1..,
        value_name = "PATTERN",
        help = "Glob patterns to exclude from the prompt; provide each pattern as a separate argument",
    )]
    pub(crate) exclude: Vec<String>,
    #[arg(short, long, global = true, value_enum, default_value_t = Format::default(), help = "Output format")]
    pub(crate) format: Format,
    #[arg(
        long,
        global = true,
        help = "Include files even if they would normally be excluded by .gitignore"
    )]
    pub(crate) no_gitignore: bool,
    #[command(flatten)]
    pub(crate) output: OutputOptions,
}

#[derive(Debug, Args)]
pub struct OutputOptions {
    #[arg(
        long,
        value_name = "OPTION",
        value_enum,
        default_value_t = TokenCountOptions::default(),
        default_missing_value = "each",
        num_args = 0..=1,
        help = "What to token count: nothing, the final output, or also each individual file"
    )]
    pub(crate) token_count: TokenCountOptions,
}

#[derive(Debug, Subcommand, Clone)]
pub enum Command {
    /// (default) Generate a prompt that includes matching files (copies to clipboard by default)
    Generate {
        #[arg(
            long,
            help = "Print prompt to stdout with no summary instead of copying to clipboard"
        )]
        stdout: bool,
    },
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

impl Default for Command {
    fn default() -> Self {
        Self::Generate { stdout: false }
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn parses_separate_exclude_arguments() {
        let cli = Cli::try_parse_from(["prompt", "--exclude", "main.rs", "Cargo.toml"])
            .expect("arguments should parse");

        assert_eq!(cli.exclude, ["main.rs", "Cargo.toml"]);
    }

    #[test]
    fn parses_repeated_exclude_options() {
        let cli =
            Cli::try_parse_from(["prompt", "--exclude", "main.rs", "--exclude", "Cargo.toml"])
                .expect("arguments should parse");

        assert_eq!(cli.exclude, ["main.rs", "Cargo.toml"]);
    }

    #[test]
    fn preserves_comma_in_exclude_pattern() {
        let cli = Cli::try_parse_from(["prompt", "--exclude", "main.rs,Cargo.toml"])
            .expect("arguments should parse");

        assert_eq!(cli.exclude, ["main.rs,Cargo.toml"]);
    }

    #[test]
    fn exclude_help_describes_separate_arguments() {
        let help = Cli::command().render_help().to_string();

        assert!(help.contains("provide each pattern as a separate argument"));
        assert!(!help.contains("separated by commas"));
    }

    #[test]
    fn parses_stdout_for_generate() {
        let cli = Cli::try_parse_from(["prompt", "generate", "--stdout"])
            .expect("generate should accept --stdout");

        assert!(matches!(
            cli.command,
            Some(Command::Generate { stdout: true })
        ));
    }

    #[test]
    fn rejects_stdout_without_generate() {
        let error = Cli::try_parse_from(["prompt", "--stdout"])
            .err()
            .expect("the top-level command should reject --stdout");

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn rejects_stdout_for_count() {
        let error = Cli::try_parse_from(["prompt", "count", "--stdout"])
            .err()
            .expect("count should reject --stdout");

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}
