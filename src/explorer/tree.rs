use std::path::{Path, PathBuf};

use crate::agent::errors::AgentError;
use crate::agent::workspace::Workspace;

#[derive(Clone, Debug)]
pub enum FileNode {
    File {
        name: String,
        path: PathBuf,
    },
    Directory {
        name: String,
        path: PathBuf,
        children: Option<Vec<FileNode>>,
        is_expanded: bool,
    },
}

pub struct ExplorerState {
    pub root: FileNode,
    pub selected_path: Option<PathBuf>,
}

impl ExplorerState {
    pub async fn build(workspace: &Workspace) -> Result<Self, AgentError> {
        let root_path = workspace.base_path().to_path_buf();
        let mut children = Vec::new();
        let walker = ignore::WalkBuilder::new(&root_path)
            .max_depth(Some(2))
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .hidden(false)
            .build();

        for entry in walker.filter_map(Result::ok).skip(1) {
            let path = entry.path().to_path_buf();
            if path.parent() != Some(root_path.as_path()) {
                continue;
            }
            children.push(node_from_path(path).await?);
        }

        children.sort_by(|a, b| node_name(a).cmp(node_name(b)));
        Ok(Self {
            root: FileNode::Directory {
                name: root_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("workspace")
                    .to_string(),
                path: root_path,
                children: Some(children),
                is_expanded: true,
            },
            selected_path: None,
        })
    }

    pub async fn toggle_directory(&mut self, path: &Path) {
        if let Some(node) = find_node_mut(&mut self.root, path) {
            if let FileNode::Directory {
                path,
                children,
                is_expanded,
                ..
            } = node
            {
                if *is_expanded {
                    *is_expanded = false;
                    return;
                }
                if children.is_none() {
                    *children = read_children(path).await.ok();
                }
                *is_expanded = true;
            }
        }
    }
}

async fn node_from_path(path: PathBuf) -> Result<FileNode, AgentError> {
    let metadata = tokio::fs::metadata(&path).await?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_string();
    if metadata.is_dir() {
        Ok(FileNode::Directory {
            name,
            path,
            children: None,
            is_expanded: false,
        })
    } else {
        Ok(FileNode::File { name, path })
    }
}

async fn read_children(path: &Path) -> Result<Vec<FileNode>, AgentError> {
    let mut entries = tokio::fs::read_dir(path).await?;
    let mut children = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        children.push(node_from_path(entry.path()).await?);
    }
    children.sort_by(|a, b| node_name(a).cmp(node_name(b)));
    Ok(children)
}

fn find_node_mut<'a>(node: &'a mut FileNode, path: &Path) -> Option<&'a mut FileNode> {
    match node {
        FileNode::File { path: node_path, .. } if node_path == path => Some(node),
        FileNode::Directory {
            path: node_path,
            children,
            ..
        } if node_path == path => Some(node),
        FileNode::Directory {
            children: Some(children),
            ..
        } => {
            for child in children {
                if let Some(found) = find_node_mut(child, path) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn node_name(node: &FileNode) -> &str {
    match node {
        FileNode::File { name, .. } | FileNode::Directory { name, .. } => name,
    }
}

#[cfg(test)]
mod tests {
    use super::{ExplorerState, FileNode};
    use crate::agent::workspace::Workspace;

    #[tokio::test]
    async fn build_respects_gitignore() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".gitignore"), "ignored.txt\n").expect("gitignore");
        std::fs::write(dir.path().join("ignored.txt"), "ignored").expect("ignored");
        std::fs::write(dir.path().join("kept.txt"), "kept").expect("kept");
        let workspace = Workspace::new(dir.path()).expect("workspace");
        let state = ExplorerState::build(&workspace).await.expect("build");
        let FileNode::Directory {
            children: Some(children),
            ..
        } = state.root
        else {
            panic!("root children");
        };
        let names = children
            .iter()
            .map(|node| match node {
                FileNode::File { name, .. } | FileNode::Directory { name, .. } => name.clone(),
            })
            .collect::<Vec<_>>();
        assert!(names.contains(&"kept.txt".to_string()));
        assert!(!names.contains(&"ignored.txt".to_string()));
    }
}
