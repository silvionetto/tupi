use crate::catalog::{load_catalog_from_str, summarize, CatalogSummary, Marketplace, TrustedCatalog};
use crate::error::Result;
use crate::profile::{Profile, ProjectProfileDefaults};
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct AppState {
    db_path: PathBuf,
    catalog_path: PathBuf,
    cache_dir: PathBuf,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOption {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceOption {
    pub id: String,
    pub name: String,
    pub repository: String,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let root = Self::resolve_workspace_root();
        let state_dir = root.join(".tupi");
        fs::create_dir_all(&state_dir).ok();
        let cache_dir = state_dir.join("catalog-cache");
        fs::create_dir_all(&cache_dir).ok();
        let state = Self {
            db_path: state_dir.join("state.sqlite"),
            catalog_path: root.join("catalog").join("trusted-assets.yaml"),
            cache_dir,
        };
        state.initialize()?;
        Ok(state)
    }

    fn resolve_workspace_root() -> PathBuf {
        let mut candidates = Vec::new();

        if let Ok(current_dir) = std::env::current_dir() {
            candidates.push(current_dir);
        }

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                candidates.push(parent.to_path_buf());
            }
        }

        for candidate in candidates {
            if let Some(root) = Self::find_workspace_root_from(&candidate) {
                return root;
            }
        }

        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    fn find_workspace_root_from(start: &Path) -> Option<PathBuf> {
        for candidate in start.ancestors() {
            if candidate.join("catalog").join("trusted-assets.yaml").is_file() {
                return Some(candidate.to_path_buf());
            }
        }

        None
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
        let source_repository = self.source_repository();
        let source_branch = self.source_branch();
        if source_branch != "main" {
            return Err(crate::error::TupiError::CatalogValidation(
                "catalog refresh must target main".into(),
            ));
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

    pub fn project_profile_defaults(&self) -> ProjectProfileDefaults {
        ProjectProfileDefaults {
            display_name: self.workspace_display_name(),
        }
    }

    pub fn upsert_profile(&self, profile: Profile) -> Result<Profile> {
        let profile = self.normalize_profile(profile)?;
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO profiles (id, profile_json)
            VALUES (?1, ?2)
            ON CONFLICT(id) DO UPDATE SET profile_json = excluded.profile_json
            "#,
            params![profile.id.clone(), serde_json::to_string(&profile)?],
        )?;
        Ok(profile)
    }

    pub fn delete_profile(&self, profile_id: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute("DELETE FROM profiles WHERE id = ?1", params![profile_id])?;
        Ok(())
    }

    pub fn list_available_agents(&self) -> Result<Vec<AgentOption>> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let agent_roots = self.trusted_marketplace_agent_roots(&catalog)?;
        let agents = Self::discover_agents_from_roots(&agent_roots)?;

        if agents.is_empty() {
            return Err(crate::error::TupiError::CatalogRead(
                "no Markdown agents were found in trusted marketplace workspaces".into(),
            ));
        }

        Ok(agents)
    }

    pub fn list_marketplaces(&self) -> Result<Vec<MarketplaceOption>> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let mut marketplaces = catalog
            .marketplaces
            .iter()
            .map(Self::to_marketplace_option)
            .collect::<Vec<_>>();
        marketplaces.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        Ok(marketplaces)
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

    fn normalize_profile(&self, profile: Profile) -> Result<Profile> {
        let id = profile.id.trim().to_string();
        let name = profile.name.trim().to_string();
        if name.is_empty() {
            return Err(crate::error::TupiError::ProfileValidation(
                "project display name is required".into(),
            ));
        }

        Ok(Profile {
            id: if id.is_empty() {
                self.generate_profile_id()?
            } else {
                id
            },
            name,
            description: Self::normalize_optional_text(profile.description),
            enabled: profile.enabled,
            version: Self::normalize_optional_text(profile.version),
            catalog_revision: Self::normalize_optional_text(profile.catalog_revision),
            selected_assets: profile
                .selected_assets
                .into_iter()
                .map(|asset| asset.trim().to_string())
                .filter(|asset| !asset.is_empty())
                .collect(),
        })
    }

    fn generate_profile_id(&self) -> Result<String> {
        let connection = self.open()?;

        loop {
            let candidate = connection.query_row(
                "SELECT lower(hex(randomblob(8)))",
                [],
                |row| row.get::<_, String>(0),
            )?;
            let exists = connection
                .query_row(
                    "SELECT 1 FROM profiles WHERE id = ?1",
                    params![candidate.as_str()],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;

            if exists.is_none() {
                return Ok(candidate);
            }
        }
    }

    fn normalize_optional_text(value: Option<String>) -> Option<String> {
        value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    }

    fn workspace_display_name(&self) -> String {
        self.catalog_path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Project".to_string())
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

    fn trusted_marketplace_agent_roots(&self, catalog: &TrustedCatalog) -> Result<Vec<(String, PathBuf)>> {
        let mut roots = Vec::new();

        for marketplace in &catalog.marketplaces {
            let workspace = self.sync_repository_workspace(
                "marketplaces",
                &marketplace.id,
                &marketplace.repository,
                &marketplace.branch,
                "trusted marketplace repository",
            )?;
            let agents_dir = workspace.join("agents");
            if agents_dir.is_dir() {
                roots.push((marketplace.id.clone(), agents_dir));
            }
        }

        Ok(roots)
    }

    fn sync_repository_workspace(
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

    fn sanitize_cache_key(value: &str) -> String {
        value
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '-',
            })
            .collect()
    }

    fn to_marketplace_option(marketplace: &Marketplace) -> MarketplaceOption {
        MarketplaceOption {
            id: marketplace.id.clone(),
            name: marketplace.name.clone(),
            repository: marketplace.repository.clone(),
        }
    }

    fn discover_agents_from_roots(agent_roots: &[(String, PathBuf)]) -> Result<Vec<AgentOption>> {
        let mut discovered: BTreeMap<String, (String, String)> = BTreeMap::new();

        for (marketplace_id, agents_dir) in agent_roots {
            Self::collect_agent_names(agents_dir, marketplace_id, &mut discovered)?;
        }

        Ok(discovered
            .into_values()
            .map(|(name, _)| AgentOption { name })
            .collect())
    }

    fn collect_agent_names(
        current_dir: &Path,
        marketplace_id: &str,
        discovered: &mut BTreeMap<String, (String, String)>,
    ) -> Result<()> {
        let entries = fs::read_dir(current_dir).map_err(|err| {
            crate::error::TupiError::CatalogRead(format!("{} ({})", current_dir.display(), err))
        })?;

        for entry in entries {
            let entry =
                entry.map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| crate::error::TupiError::CatalogRead(err.to_string()))?;

            if file_type.is_dir() {
                Self::collect_agent_names(&path, marketplace_id, discovered)?;
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if extension.as_deref() != Some("md") {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    crate::error::TupiError::CatalogValidation(format!(
                        "agent file {} must have a valid name",
                        path.display()
                    ))
                })?;

            let duplicate_key = stem.to_ascii_lowercase();
            if let Some((_, existing_marketplace)) = discovered.get(&duplicate_key) {
                return Err(crate::error::TupiError::CatalogValidation(format!(
                    "duplicate agent name {} found in trusted marketplace {} and {}",
                    stem, existing_marketplace, marketplace_id
                )));
            }

            discovered.insert(duplicate_key, (stem, marketplace_id.to_string()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_sorted_markdown_agents_from_nested_directories() {
        let root = unique_temp_dir("agent-discovery");
        let agents_dir = root.join("agents");
        fs::create_dir_all(agents_dir.join("nested")).unwrap();
        fs::write(agents_dir.join("reviewer.md"), "# reviewer").unwrap();
        fs::write(agents_dir.join("nested").join("planner.md"), "# planner").unwrap();
        fs::write(agents_dir.join("nested").join("notes.txt"), "ignore").unwrap();

        let agents = AppState::discover_agents_from_roots(&[(String::from("official"), agents_dir)])
            .unwrap();

        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "planner");
        assert_eq!(agents[1].name, "reviewer");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_agent_names_across_marketplaces() {
        let root = unique_temp_dir("agent-duplicates");
        let first = root.join("first").join("agents");
        let second = root.join("second").join("agents");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(first.join("reviewer.md"), "# reviewer").unwrap();
        fs::write(second.join("Reviewer.md"), "# reviewer").unwrap();

        let error = AppState::discover_agents_from_roots(&[
            (String::from("official"), first),
            (String::from("awesome-copilot"), second),
        ])
        .unwrap_err();

        assert!(error.to_string().contains("duplicate agent name Reviewer"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finds_workspace_root_from_src_tauri_directory() {
        let root = unique_temp_dir("workspace-root");
        fs::create_dir_all(root.join("catalog")).unwrap();
        fs::create_dir_all(root.join("src-tauri").join("src")).unwrap();
        fs::write(root.join("catalog").join("trusted-assets.yaml"), "version: 1").unwrap();

        let resolved = AppState::find_workspace_root_from(&root.join("src-tauri").join("src"));

        assert_eq!(resolved.as_deref(), Some(root.as_path()));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upsert_profile_generates_database_backed_id_and_optional_description() {
        let root = unique_temp_dir("profile-upsert");
        let state = make_test_state(&root);

        let saved = state
            .upsert_profile(crate::profile::Profile {
                id: String::new(),
                name: String::from(" myProject "),
                description: Some(String::from("  Example repo  ")),
                enabled: true,
                version: Some(String::from("1")),
                catalog_revision: Some(String::from("catalog-1")),
                selected_assets: vec![String::from(" reviewer "), String::new()],
            })
            .unwrap();

        assert!(!saved.id.is_empty());
        assert_eq!(saved.name, "myProject");
        assert_eq!(saved.description.as_deref(), Some("Example repo"));
        assert_eq!(saved.selected_assets, vec![String::from("reviewer")]);

        let profiles = state.read_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, saved.id);
        assert_eq!(profiles[0].description.as_deref(), Some("Example repo"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_legacy_profiles_without_description_field() {
        let root = unique_temp_dir("legacy-profile");
        let state = make_test_state(&root);
        let connection = state.open().unwrap();
        connection
            .execute(
                "INSERT INTO profiles (id, profile_json) VALUES (?1, ?2)",
                rusqlite::params![
                    "legacy-profile",
                    r#"{"id":"legacy-profile","name":"Legacy Project","enabled":true,"version":"1","catalogRevision":"catalog-1","selected_assets":["reviewer"]}"#
                ],
            )
            .unwrap();

        let profiles = state.read_profiles().unwrap();

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "legacy-profile");
        assert_eq!(profiles[0].description, None);

        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    fn make_test_state(root: &Path) -> AppState {
        let state_dir = root.join(".tupi");
        let cache_dir = state_dir.join("catalog-cache");
        let catalog_dir = root.join("catalog");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&catalog_dir).unwrap();
        fs::write(catalog_dir.join("trusted-assets.yaml"), "version: 1\ncatalogRevision: test\nmarketplaces: []\n").unwrap();

        let state = AppState {
            db_path: state_dir.join("state.sqlite"),
            catalog_path: catalog_dir.join("trusted-assets.yaml"),
            cache_dir,
        };
        state.initialize().unwrap();
        state
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tupi-{name}-{suffix}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
