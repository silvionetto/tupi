use super::AppState;
use crate::error::Result;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

impl AppState {
    pub(crate) fn fetch_from_git(&self, repository: &str, source_branch: &str) -> Result<String> {
        let repo_dir = self.cache_dir.join("source-repo");
        if repo_dir.exists() && !repo_dir.join(".git").exists() {
            fs::remove_dir_all(&repo_dir).ok();
        }
        if !repo_dir.join(".git").exists() {
            let status = Command::new("git")
                .args([
                    "clone",
                    "--no-checkout",
                    "--branch",
                    source_branch,
                    "--single-branch",
                    repository,
                    repo_dir.to_string_lossy().as_ref(),
                ])
                .status()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            if !status.success() {
                return Err(crate::error::TupiError::CatalogRead(
                    "failed to clone trusted catalog repository".into(),
                ));
            }
        } else {
            let status = Command::new("git")
                .args([
                    "-C",
                    repo_dir.to_string_lossy().as_ref(),
                    "fetch",
                    "origin",
                    source_branch,
                ])
                .status()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            if !status.success() {
                return Err(crate::error::TupiError::CatalogRead(
                    "failed to fetch trusted catalog repository".into(),
                ));
            }
        }

        let verify = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "rev-parse",
                &format!("origin/{source_branch}"),
            ])
            .output()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        if !verify.status.success() {
            return Err(crate::error::TupiError::CatalogRead(
                "failed to verify trusted catalog branch".into(),
            ));
        }

        let catalog_path = repo_dir.join("catalog").join("trusted-assets.yaml");
        fs::read_to_string(&catalog_path).map_err(|err| {
            crate::error::TupiError::CatalogRead(format!("{} ({})", catalog_path.display(), err))
        })
    }

    pub(crate) fn sync_repository_workspace(
        &self,
        namespace: &str,
        cache_key: &str,
        repository: &str,
        branch: &str,
        label: &str,
    ) -> Result<PathBuf> {
        let repo_dir = self
            .cache_dir
            .join(namespace)
            .join(Self::sanitize_cache_key(cache_key));

        if let Some(parent) = repo_dir.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        }

        if repo_dir.exists() && !repo_dir.join(".git").exists() {
            fs::remove_dir_all(&repo_dir).ok();
        }

        if !repo_dir.join(".git").exists() {
            let status = Command::new("git")
                .args([
                    "clone",
                    "--branch",
                    branch,
                    "--single-branch",
                    repository,
                    repo_dir.to_string_lossy().as_ref(),
                ])
                .status()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            if !status.success() {
                return Err(crate::error::TupiError::CatalogRead(format!(
                    "failed to clone {label}"
                )));
            }
        } else {
            let status = Command::new("git")
                .args([
                    "-C",
                    repo_dir.to_string_lossy().as_ref(),
                    "fetch",
                    "origin",
                    branch,
                ])
                .status()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            if !status.success() {
                return Err(crate::error::TupiError::CatalogRead(format!(
                    "failed to fetch {label}"
                )));
            }
        }

        let status = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "checkout",
                "-B",
                branch,
                &format!("origin/{branch}"),
            ])
            .status()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        if !status.success() {
            return Err(crate::error::TupiError::CatalogRead(format!(
                "failed to check out {label} branch"
            )));
        }

        Ok(repo_dir)
    }
}
