use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{command, Parser};
use dashmap::DashMap;
use ignore::{WalkBuilder, WalkState};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version)]
struct Cli {
    path: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .expect("should be able to initialize the logger");
    let cli = Cli::parse();
    let path = cli.path.unwrap_or_else(|| PathBuf::from("."));
    let all_files = DashMap::new();

    WalkBuilder::new(&path)
        .add_custom_ignore_filename(".promptignore")
        .build_parallel()
        .run(|| {
            Box::new(|result| {
                match result {
                    Ok(dir_entry) => {
                        if dir_entry.path().is_dir() {
                            return WalkState::Continue;
                        }
                        all_files.insert(
                            dir_entry.path().to_path_buf(),
                            read_file_sync(dir_entry.path()).unwrap_or_else(|_| vec![]),
                        );
                    }
                    Err(err) => {
                        panic!("Error reading file: {}", err);
                    }
                }
                WalkState::Continue
            })
        });

    // tracing::info!("Read {} files into memory.", all_files.len());
    print_files(all_files);
    Ok(())
}

fn read_file_sync(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}

fn print_files(all_files: DashMap<PathBuf, Vec<u8>>) {
    let mut keys = all_files
        .iter()
        .map(|r| r.key().clone())
        .collect::<Vec<_>>();
    keys.sort();
    for path in keys {
        println!("{}:", path.display());
        println!("");
        println!(
            "{}",
            String::from_utf8_lossy(&all_files.get(&path).unwrap())
        );
        println!("---");
    }
}
