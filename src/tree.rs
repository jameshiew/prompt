use std::collections::BTreeMap;

use termtree::Tree;

use std::path::Path;

use crate::files::{FileMeta, Files, ReadStatus, strip_dot_prefix};
use crate::fmt::group_digits;

#[derive(Debug, Clone)]
pub struct FiletreeNode {
    name: String,
    meta: Option<FileMeta>,
    children: BTreeMap<String, Self>,
}

impl FiletreeNode {
    pub fn new(name: &str, meta: Option<FileMeta>) -> Self {
        Self {
            name: name.to_string(),
            children: BTreeMap::new(),
            meta,
        }
    }

    fn label(&self) -> String {
        self.meta.as_ref().map_or_else(
            || self.name.clone(),
            |meta| match meta.read_status {
                ReadStatus::ExcludedExplicitly => format!("{} (excluded)", self.name),
                ReadStatus::ExcludedBinaryDetected => {
                    format!("{} (auto-excluded, binary detected)", self.name)
                }
                ReadStatus::Read => self.name.clone(),
                ReadStatus::TokenCounted(token_count) => {
                    format!("{} ({} tokens)", self.name, group_digits(token_count))
                }
            },
        )
    }

    fn to_tree(&self) -> Tree<String> {
        let mut tree = Tree::new(self.label());
        for child in self.children.values() {
            tree.push(child.to_tree());
        }
        tree
    }

    pub fn tty_output(&self) -> String {
        self.to_tree().to_string()
    }

    pub fn from_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut root = Self::new(".", None);
        for path in paths {
            root.insert_full_path(path, None);
        }
        root
    }

    fn insert_full_path(&mut self, path: &Path, meta: Option<FileMeta>) {
        // Remove leading "./" since the root node is the "."
        let path = strip_dot_prefix(path);

        let components = path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>();

        self.insert_path(&components, meta);
    }

    pub fn insert_path(&mut self, components: &[&str], meta: Option<FileMeta>) {
        if components.is_empty() {
            return;
        }

        let name = components[0];
        let is_last = components.len() == 1;

        let entry = self.children.entry(name.to_string());
        let entry = if is_last {
            // file node
            let meta = meta.clone();
            entry.or_insert_with(|| Self::new(name, meta))
        } else {
            // directory node
            entry.or_insert_with(|| Self::new(name, None))
        };

        if !is_last {
            entry.insert_path(&components[1..], meta);
        }
    }
}

impl From<&Files> for FiletreeNode {
    fn from(files: &Files) -> Self {
        // Build a tree of files collected
        let mut root = Self::new(".", None);
        for entry in files.iter() {
            let meta = entry.value().meta.clone();
            root.insert_full_path(entry.key(), Some(meta));
        }
        root
    }
}
