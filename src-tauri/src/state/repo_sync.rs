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
            let configured_origin = Command::new("git")
                .args([
                    "-C",
                    repo_dir.to_string_lossy().as_ref(),
                    "remote",
                    "get-url",
                    "origin",
                ])
                .output()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            if !configured_origin.status.success() {
                return Err(crate::error::TupiError::CatalogRead(
                    "failed to verify trusted catalog repository origin".into(),
                ));
            }
            if String::from_utf8_lossy(&configured_origin.stdout).trim() != repository {
                let status = Command::new("git")
                    .args([
                        "-C",
                        repo_dir.to_string_lossy().as_ref(),
                        "remote",
                        "set-url",
                        "origin",
                        repository,
                    ])
                    .status()
                    .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
                if !status.success() {
                    return Err(crate::error::TupiError::CatalogRead(
                        "failed to configure trusted catalog repository origin".into(),
                    ));
                }
            }
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

        let catalog_contents = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "show",
                &format!("origin/{source_branch}:catalog/trusted-assets.yaml"),
            ])
            .output()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        if !catalog_contents.status.success() {
            return Err(crate::error::TupiError::CatalogRead(format!(
                "failed to read trusted catalog from origin/{source_branch}"
            )));
        }
        String::from_utf8(catalog_contents.stdout).map_err(|err| {
            crate::error::TupiError::CatalogRead(format!(
                "trusted catalog is not valid UTF-8 ({err})"
            ))
        })
    }

    pub(crate) fn sync_repository_workspace(
        &self,
        namespace: &str,
        cache_key: &str,
        repository: &str,
        branch: &str,
        revision: &str,
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

        let branch_ref = format!("origin/{branch}");
        let status = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "merge-base",
                "--is-ancestor",
                revision,
                &branch_ref,
            ])
            .status()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        if !status.success() {
            return Err(crate::error::TupiError::CatalogRead(format!(
                "pinned revision {revision} is not on {branch} for {label}"
            )));
        }

        let status = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "checkout",
                "--detach",
                revision,
            ])
            .status()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        if !status.success() {
            return Err(crate::error::TupiError::CatalogRead(format!(
                "failed to check out pinned revision for {label}"
            )));
        }

        let verified = Command::new("git")
            .args([
                "-C",
                repo_dir.to_string_lossy().as_ref(),
                "rev-parse",
                "HEAD",
            ])
            .output()
            .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
        let head = String::from_utf8_lossy(&verified.stdout).trim().to_string();
        if !verified.status.success() || head != revision {
            return Err(crate::error::TupiError::CatalogRead(format!(
                "failed to verify pinned revision for {label}"
            )));
        }

        Ok(repo_dir)
    }
}
