use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::Shell;
use prompt::run::{Format, TokenCountOptions};

#[derive(Parser)]
#[command(
    version,
    subcommand_required = false,
    subcommand_precedence_over_arg = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    default_generate: GenerateOptions,
}

impl Cli {
    pub(crate) fn try_into_command(self) -> Result<Command, clap::Error> {
        match self.command {
            Some(_) if self.default_generate.is_present() => Err(Self::command().error(
                ErrorKind::ArgumentConflict,
                "top-level options cannot be used with an explicit subcommand; put each option after its subcommand",
            )),
            Some(command) => Ok(command),
            None => Ok(Command::Generate(GenerateCommand::for_default_invocation(
                self.default_generate,
            ))),
        }
    }
}

#[derive(Debug, Args)]
pub struct FileOptions {
    #[arg(
        short,
        long,
        num_args = 1..,
        value_name = "PATH",
        help = "Paths to the files/directories for reading into a prompt [default: .]",
    )]
    pub(crate) paths: Vec<PathBuf>,
    #[arg(
        short,
        long,
        num_args = 1..,
        value_name = "PATTERN",
        help = "Glob patterns to exclude from the prompt; provide each pattern as a separate argument",
    )]
    pub(crate) exclude: Vec<String>,
    #[arg(
        long,
        help = "Include files even if they would normally be excluded by .gitignore"
    )]
    pub(crate) no_gitignore: bool,
}

impl FileOptions {
    const fn is_present(&self) -> bool {
        !self.paths.is_empty() || !self.exclude.is_empty() || self.no_gitignore
    }

    pub(crate) fn into_parts(self) -> (PathBuf, Vec<PathBuf>, Vec<String>, bool) {
        let mut paths = self.paths.into_iter();
        let first_path = paths.next().unwrap_or_else(|| PathBuf::from("."));

        (first_path, paths.collect(), self.exclude, self.no_gitignore)
    }
}

#[derive(Debug, Args)]
pub struct GenerateOptions {
    #[command(flatten)]
    pub(crate) files: FileOptions,
    #[arg(short, long, value_enum, help = "Output format [default: plaintext]")]
    pub(crate) format: Option<Format>,
    #[arg(
        long,
        value_name = "OPTION",
        value_enum,
        default_missing_value = "each",
        num_args = 0..=1,
        help = "What to token count: nothing, the final output, or also each individual file [default: final]"
    )]
    pub(crate) token_count: Option<TokenCountOptions>,
}

impl GenerateOptions {
    const fn is_present(&self) -> bool {
        self.files.is_present() || self.format.is_some() || self.token_count.is_some()
    }
}

#[derive(Debug, Args)]
pub struct GenerateCommand {
    #[command(flatten)]
    pub(crate) options: GenerateOptions,
    #[arg(
        long,
        help = "Print prompt to stdout with no summary instead of copying to clipboard"
    )]
    pub(crate) stdout: bool,
}

impl GenerateCommand {
    const fn for_default_invocation(options: GenerateOptions) -> Self {
        Self {
            options,
            stdout: false,
        }
    }
}

#[derive(Debug, Args)]
pub struct CountCommand {
    #[command(flatten)]
    pub(crate) files: FileOptions,
    #[arg(
        long,
        value_name = "COUNT",
        help = "List top files by token count",
        default_missing_value = "10",
        num_args = 0..=1
    )]
    pub(crate) top: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// (default) Generate a prompt that includes matching files (copies to clipboard by default)
    Generate(GenerateCommand),
    /// Generate shell completions
    ShellCompletions {
        #[arg()]
        shell: Shell,
    },
    /// Count tokens from matching files
    Count(CountCommand),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_separate_exclude_arguments_for_default_generate() {
        let command = Cli::try_parse_from(["prompt", "--exclude", "main.rs", "Cargo.toml"])
            .expect("arguments should parse")
            .try_into_command()
            .expect("default command should be valid");

        let Command::Generate(generate) = command else {
            panic!("default command should generate");
        };
        assert_eq!(generate.options.files.exclude, ["main.rs", "Cargo.toml"]);
    }

    #[test]
    fn parses_repeated_exclude_options_for_generate() {
        let command = Cli::try_parse_from([
            "prompt",
            "generate",
            "--exclude",
            "main.rs",
            "--exclude",
            "Cargo.toml",
        ])
        .expect("arguments should parse")
        .try_into_command()
        .expect("generate command should be valid");

        let Command::Generate(generate) = command else {
            panic!("command should generate");
        };
        assert_eq!(generate.options.files.exclude, ["main.rs", "Cargo.toml"]);
    }

    #[test]
    fn preserves_comma_in_count_exclude_pattern() {
        let command = Cli::try_parse_from(["prompt", "count", "--exclude", "main.rs,Cargo.toml"])
            .expect("arguments should parse")
            .try_into_command()
            .expect("count command should be valid");

        let Command::Count(count) = command else {
            panic!("command should count");
        };
        assert_eq!(count.files.exclude, ["main.rs,Cargo.toml"]);
    }

    #[test]
    fn exclude_help_describes_separate_arguments() {
        let help = Cli::command().render_help().to_string();

        assert!(help.contains("provide each pattern as a separate argument"));
        assert!(!help.contains("separated by commas"));
    }

    #[test]
    fn parses_stdout_for_generate() {
        let command = Cli::try_parse_from(["prompt", "generate", "--stdout"])
            .expect("generate should accept --stdout")
            .try_into_command()
            .expect("generate command should be valid");

        assert!(matches!(
            command,
            Command::Generate(GenerateCommand { stdout: true, .. })
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
    fn count_rejects_generation_options() {
        for arguments in [
            &["--format", "json"][..],
            &["--token-count", "each"][..],
            &["--stdout"][..],
        ] {
            let error = Cli::try_parse_from(
                ["prompt", "count"]
                    .into_iter()
                    .chain(arguments.iter().copied()),
            )
            .err()
            .expect("count should reject generation options");

            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn shell_completions_rejects_unrelated_options() {
        for arguments in [
            &["--paths", "src"][..],
            &["--exclude", "target"][..],
            &["--format", "json"][..],
            &["--no-gitignore"][..],
            &["--token-count", "each"][..],
            &["--stdout"][..],
            &["--top", "10"][..],
        ] {
            let error = Cli::try_parse_from(
                ["prompt", "shell-completions", "bash"]
                    .into_iter()
                    .chain(arguments.iter().copied()),
            )
            .err()
            .expect("shell-completions should reject unrelated options");

            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[test]
    fn shell_completions_rejects_default_options_before_subcommand() {
        for arguments in [
            &["--paths", "src"][..],
            &["--exclude", "target"][..],
            &["--format", "json"][..],
            &["--no-gitignore"][..],
            &["--token-count", "each"][..],
        ] {
            let error = Cli::try_parse_from(
                std::iter::once("prompt")
                    .chain(arguments.iter().copied())
                    .chain(["shell-completions", "bash"]),
            )
            .and_then(Cli::try_into_command)
            .expect_err("default options should conflict with an explicit subcommand");

            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn shell_completions_help_omits_unrelated_options() {
        let help = Cli::command()
            .find_subcommand_mut("shell-completions")
            .expect("shell-completions should exist")
            .render_help()
            .to_string();

        for option in [
            "--paths",
            "--exclude",
            "--format",
            "--no-gitignore",
            "--token-count",
            "--stdout",
            "--top",
        ] {
            assert!(!help.contains(option), "help should omit {option}");
        }
    }
}
