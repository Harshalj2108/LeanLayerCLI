use std::path::{Path, PathBuf};

use crate::integration::errors::IntegrationError;

#[derive(Debug, Clone)]
pub struct GitWorkspace {
    pub repo_path: PathBuf,
}

impl GitWorkspace {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            repo_path: path.as_ref().to_path_buf(),
        }
    }

    pub async fn get_status(&self) -> Result<Vec<(String, String)>, IntegrationError> {
        let repo_path = self.repo_path.clone();
        tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open(repo_path)?;
            let statuses = repo.statuses(None)?;
            let mut files = Vec::new();

            for entry in statuses.iter() {
                let status = entry.status();
                let path = entry
                    .path()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "<unknown>".to_string());
                let label = if status.is_wt_new() {
                    "Untracked"
                } else if status.is_wt_modified() || status.is_index_modified() {
                    "Modified"
                } else if status.is_wt_deleted() || status.is_index_deleted() {
                    "Deleted"
                } else if status.is_conflicted() {
                    "Conflicted"
                } else if status.is_index_new() {
                    "Staged"
                } else {
                    "Changed"
                };
                files.push((path, label.to_string()));
            }

            Ok::<_, IntegrationError>(files)
        })
        .await?
    }

    pub async fn commit_all(&self, message: &str) -> Result<String, IntegrationError> {
        let repo_path = self.repo_path.clone();
        let message = message.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open(repo_path)?;
            let mut index = repo.index()?;
            index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
            index.write()?;

            let oid = index.write_tree()?;
            let tree = repo.find_tree(oid)?;
            let signature = repo
                .signature()
                .or_else(|_| git2::Signature::now("LeanLayerCLI", "leanlayer@example.local"))?;
            let parent = repo.head().ok().and_then(|head| head.peel_to_commit().ok());
            let parents = parent.iter().collect::<Vec<_>>();
            let commit_oid =
                repo.commit(Some("HEAD"), &signature, &signature, &message, &tree, &parents)?;
            Ok::<_, IntegrationError>(commit_oid.to_string())
        })
        .await?
    }

    pub async fn create_branch(&self, branch_name: &str) -> Result<(), IntegrationError> {
        let repo_path = self.repo_path.clone();
        let branch_name = branch_name.to_string();
        tokio::task::spawn_blocking(move || {
            let repo = git2::Repository::open(repo_path)?;
            let head_commit = repo.head()?.peel_to_commit()?;
            repo.branch(&branch_name, &head_commit, false)?;
            let ref_name = format!("refs/heads/{}", branch_name);
            repo.set_head(&ref_name)?;
            repo.checkout_head(Some(git2::build::CheckoutBuilder::new().safe()))?;
            Ok::<_, IntegrationError>(())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::GitWorkspace;

    #[tokio::test]
    async fn commit_all_returns_oid() {
        let dir = tempfile::tempdir().expect("tempdir");
        git2::Repository::init(dir.path()).expect("init repo");
        std::fs::write(dir.path().join("hello.txt"), "hello").expect("write");

        let workspace = GitWorkspace::new(dir.path());
        let oid = workspace.commit_all("initial").await.expect("commit");
        assert!(!oid.is_empty());
    }
}
