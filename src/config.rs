use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PromptConfig {}

pub fn find_config_path(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start.to_path_buf());
    while let Some(dir) = current {
        let candidate = dir.join(".prompt/config.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        current = dir.parent().map(|p| p.to_path_buf());
    }
    None
}
