use std::collections::BTreeMap;

use anyhow::Result;
use termtree::Tree;

use crate::files::{FileMeta, Files, strip_dot_prefix};

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
        match &self.meta {
            Some(meta) => match meta.read_status {
                crate::files::ReadStatus::ExcludedExplicitly => {
                    format!("{} (excluded)", &self.name)
                }
                crate::files::ReadStatus::ExcludedBinaryDetected => {
                    format!("{} (auto-excluded, binary detected)", &self.name)
                }
                crate::files::ReadStatus::Read => self.name.clone(),
                crate::files::ReadStatus::TokenCounted(token_count) => {
                    format!("{} ({} tokens)", &self.name, token_count)
                }
            },
            None => self.name.clone(),
        }
    }

    fn to_tree(&self) -> Tree<String> {
        let mut tree = Tree::new(self.label());
        for child in self.children.values() {
            tree.push(child.to_tree());
        }
        tree
    }

    pub fn tty_output(&self) -> Result<String> {
        let tree = self.to_tree();
        let output = format!("{tree}");
        Ok(output)
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

impl TryFrom<&Files> for FiletreeNode {
    type Error = anyhow::Error;

    fn try_from(files: &Files) -> Result<Self> {
        let paths = files.iter().map(|r| r.key().clone());

        // Build a tree of files collected
        let mut root = Self::new(".", None);
        for path in paths {
            let meta = files
                .get(&path)
                .expect("should be able to get file contents from map")
                .value()
                .meta
                .clone();

            // Remove leading "./" since the root node is the "."
            let path = strip_dot_prefix(&path);

            let components = path
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect::<Vec<_>>();

            root.insert_path(&components, Some(meta));
        }
        Ok(root)
    }
}
