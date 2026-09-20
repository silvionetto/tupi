use crate::catalog::{load_catalog_from_str, summarize, CatalogSummary};
use crate::error::Result;
use crate::profile::Profile;
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct AppState {
    db_path: PathBuf,
    catalog_path: PathBuf,
    repository_catalog_path: PathBuf,
    cache_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryCatalog {
    pub version: u32,
    #[serde(default)]
    pub repositories: Vec<TrustedRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedRepository {
    pub id: String,
    pub repository: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRecord {
    pub summary: CatalogSummary,
    pub trust_status: TrustStatus,
    pub source_repository: Option<String>,
    pub source_branch: String,
    pub refreshed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogState {
    pub summary: CatalogSummary,
    pub trust_status: TrustStatus,
    pub source_repository: Option<String>,
    pub source_branch: String,
    pub refreshed_at: Option<String>,
    pub stale: bool,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let state_dir = root.join(".tupi");
        fs::create_dir_all(&state_dir).ok();
        let cache_dir = state_dir.join("catalog-cache");
        fs::create_dir_all(&cache_dir).ok();
        let state = Self {
            db_path: state_dir.join("state.sqlite"),
            catalog_path: root.join("catalog").join("trusted-assets.yaml"),
            repository_catalog_path: root.join("catalog_repository.yml"),
            cache_dir,
        };
        state.initialize()?;
        Ok(state)
    }

    pub fn open(&self) -> Result<Connection> {
        Ok(Connection::open(&self.db_path)?)
    }

    fn initialize(&self) -> Result<()> {
        let connection = self.open()?;
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS catalog_cache (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                version INTEGER NOT NULL,
                catalog_revision TEXT NOT NULL,
                summary_json TEXT NOT NULL,
                catalog_yaml TEXT NOT NULL DEFAULT '',
                source_repository TEXT,
                source_branch TEXT NOT NULL DEFAULT 'main',
                trust_status TEXT NOT NULL,
                stale INTEGER NOT NULL DEFAULT 0,
                refreshed_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS profiles (
                id TEXT PRIMARY KEY,
                profile_json TEXT NOT NULL
            );
            "#,
        )?;
        self.ensure_cache_columns()?;
        Ok(())
    }

    fn ensure_cache_columns(&self) -> Result<()> {
        let connection = self.open()?;
        let columns = {
            let mut stmt = connection.prepare("PRAGMA table_info(catalog_cache)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut columns = Vec::new();
            for row in rows {
                columns.push(row?);
            }
            columns
        };

        if !columns.iter().any(|name| name == "catalog_yaml") {
            connection.execute(
                "ALTER TABLE catalog_cache ADD COLUMN catalog_yaml TEXT NOT NULL DEFAULT ''",
                [],
            )?;
        }
        if !columns.iter().any(|name| name == "source_repository") {
            connection.execute(
                "ALTER TABLE catalog_cache ADD COLUMN source_repository TEXT",
                [],
            )?;
        }
        if !columns.iter().any(|name| name == "source_branch") {
            connection.execute(
                "ALTER TABLE catalog_cache ADD COLUMN source_branch TEXT NOT NULL DEFAULT 'main'",
                [],
            )?;
        }
        if !columns.iter().any(|name| name == "stale") {
            connection.execute(
                "ALTER TABLE catalog_cache ADD COLUMN stale INTEGER NOT NULL DEFAULT 0",
                [],
            )?;
        }
        Ok(())
    }

    pub fn load_catalog_summary(&self) -> Result<CatalogSummary> {
        Ok(self.load_catalog_state()?.summary)
    }

    pub fn load_catalog_state(&self) -> Result<CatalogState> {
        if let Some(cached) = self.read_cached_state()? {
            return Ok(cached);
        }

        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let summary = summarize(&catalog);
        Ok(CatalogState {
            summary,
            trust_status: TrustStatus::Trusted,
            source_repository: self.source_repository(),
            source_branch: self.source_branch(),
            refreshed_at: None,
            stale: true,
        })
    }

    pub fn refresh_catalog(&self) -> Result<RefreshRecord> {
        let repository_catalog = self.load_repository_catalog()?;
        let source_repository = self.source_repository();
        let source_branch = self.source_branch();
        if source_branch != "main" {
            return Err(crate::error::TupiError::CatalogValidation(
                "catalog refresh must target main".into(),
            ));
        }
        if let Some(repository) = source_repository.as_deref() {
            self.verify_trusted_repository(&repository_catalog, repository, &source_branch)?;
        }
        let contents = self.fetch_catalog_contents(source_repository.as_deref(), &source_branch)?;
        let catalog = load_catalog_from_str(&contents)?;
        let summary = summarize(&catalog);
        let refreshed_at = Utc::now().to_rfc3339();

        let mut connection = self.open()?;
        let tx = connection.transaction()?;
        tx.execute(
            r#"
            INSERT INTO catalog_cache (
                id, version, catalog_revision, summary_json, catalog_yaml,
                source_repository, source_branch, trust_status, stale, refreshed_at
            )
            VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8)
            ON CONFLICT(id) DO UPDATE SET
                version = excluded.version,
                catalog_revision = excluded.catalog_revision,
                summary_json = excluded.summary_json,
                catalog_yaml = excluded.catalog_yaml,
                source_repository = excluded.source_repository,
                source_branch = excluded.source_branch,
                trust_status = excluded.trust_status,
                stale = excluded.stale,
                refreshed_at = excluded.refreshed_at
            "#,
            params![
                summary.version,
                summary.catalog_revision.clone(),
                serde_json::to_string(&summary)?,
                contents,
                source_repository.clone(),
                source_branch.clone(),
                "Trusted",
                refreshed_at.clone()
            ],
        )?;
        tx.commit()?;

        Ok(RefreshRecord {
            summary,
            trust_status: TrustStatus::Trusted,
            source_repository,
            source_branch,
            refreshed_at,
        })
    }

    pub fn read_profiles(&self) -> Result<Vec<Profile>> {
        let connection = self.open()?;
        let mut stmt = connection.prepare("SELECT profile_json FROM profiles ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            let json: String = row.get(0)?;
            serde_json::from_str(&json).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        })?;

        let mut profiles = Vec::new();
        for row in rows {
            profiles.push(row?);
        }
        Ok(profiles)
    }

    pub fn upsert_profile(&self, profile: Profile) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO profiles (id, profile_json)
            VALUES (?1, ?2)
            ON CONFLICT(id) DO UPDATE SET profile_json = excluded.profile_json
            "#,
            params![profile.id, serde_json::to_string(&profile)?],
        )?;
        Ok(())
    }

    pub fn delete_profile(&self, profile_id: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute("DELETE FROM profiles WHERE id = ?1", params![profile_id])?;
        Ok(())
    }

    pub fn _catalog_path(&self) -> &Path {
        &self.catalog_path
    }

    fn source_repository(&self) -> Option<String> {
        std::env::var("TUPI_CATALOG_REPOSITORY")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn source_branch(&self) -> String {
        std::env::var("TUPI_CATALOG_BRANCH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "main".to_string())
    }

    fn read_active_catalog_contents(&self) -> Result<String> {
        if let Some(cached) = self.read_cached_catalog_yaml()? {
            return Ok(cached);
        }
        fs::read_to_string(&self.catalog_path).map_err(|err| {
            crate::error::TupiError::CatalogRead(format!(
                "{} ({})",
                self.catalog_path.display(),
                err
            ))
        })
    }

    fn load_repository_catalog(&self) -> Result<RepositoryCatalog> {
        let contents = fs::read_to_string(&self.repository_catalog_path).map_err(|err| {
            crate::error::TupiError::CatalogRead(format!(
                "{} ({})",
                self.repository_catalog_path.display(),
                err
            ))
        })?;
        let catalog: RepositoryCatalog =
            serde_yaml::from_str(&contents).map_err(|err| crate::error::TupiError::CatalogParse(err.to_string()))?;
        self.validate_repository_catalog(&catalog)?;
        Ok(catalog)
    }

    fn validate_repository_catalog(&self, catalog: &RepositoryCatalog) -> Result<()> {
        if catalog.version == 0 {
            return Err(crate::error::TupiError::CatalogValidation(
                "repository catalog version must be positive".into(),
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for repository in &catalog.repositories {
            if repository.id.trim().is_empty() {
                return Err(crate::error::TupiError::CatalogValidation(
                    "repository id is required".into(),
                ));
            }
            if repository.repository.trim().is_empty() {
                return Err(crate::error::TupiError::CatalogValidation(
                    "repository url is required".into(),
                ));
            }
            if repository.branch != "main" {
                return Err(crate::error::TupiError::CatalogValidation(format!(
                    "repository {} must target main",
                    repository.id
                )));
            }
            if !seen.insert(repository.id.as_str()) {
                return Err(crate::error::TupiError::CatalogValidation(format!(
                    "duplicate repository id: {}",
                    repository.id
                )));
            }
            url::Url::parse(&repository.repository)?;
        }
        Ok(())
    }

    fn verify_trusted_repository(
        &self,
        catalog: &RepositoryCatalog,
        repository: &str,
        branch: &str,
    ) -> Result<()> {
        if let Some(entry) = catalog
            .repositories
            .iter()
            .find(|candidate| candidate.repository == repository)
        {
            if entry.branch != branch {
                return Err(crate::error::TupiError::CatalogValidation(format!(
                    "repository {} is only trusted on {}",
                    repository, entry.branch
                )));
            }
            return Ok(());
        }

        Err(crate::error::TupiError::CatalogValidation(format!(
            "repository {} is not listed in catalog_repository.yml",
            repository
        )))
    }

    fn read_cached_catalog_yaml(&self) -> Result<Option<String>> {
        let connection = self.open()?;
        let value = connection
            .query_row(
                "SELECT catalog_yaml FROM catalog_cache WHERE id = 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        Ok(value.filter(|contents| !contents.trim().is_empty()))
    }

    fn read_cached_state(&self) -> Result<Option<CatalogState>> {
        let connection = self.open()?;
        let row = connection
            .query_row(
                r#"
                SELECT summary_json, trust_status, source_repository, source_branch, refreshed_at, stale
                FROM catalog_cache
                WHERE id = 1
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;

        if let Some((summary_json, trust_status, source_repository, source_branch, refreshed_at, stale)) = row
        {
            let summary: CatalogSummary = serde_json::from_str(&summary_json)?;
            return Ok(Some(CatalogState {
                summary,
                trust_status: match trust_status.as_str() {
                    "Trusted" => TrustStatus::Trusted,
                    "Untrusted" => TrustStatus::Untrusted,
                    "Stale" => TrustStatus::Stale,
                    "Invalid" => TrustStatus::Invalid,
                    _ => TrustStatus::Missing,
                },
                source_repository,
                source_branch,
                refreshed_at: Some(refreshed_at),
                stale: stale != 0,
            }));
        }

        Ok(None)
    }

    fn fetch_catalog_contents(
        &self,
        source_repository: Option<&str>,
        source_branch: &str,
    ) -> Result<String> {
        if let Some(repository) = source_repository {
            self.fetch_from_git(repository, source_branch)
        } else {
            self.read_active_catalog_contents()
        }
    }

    fn fetch_from_git(&self, repository: &str, source_branch: &str) -> Result<String> {
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
}
