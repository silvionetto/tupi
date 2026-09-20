use crate::catalog::{load_catalog_from_str, summarize, CatalogSummary, Marketplace, TrustedCatalog};
use crate::error::Result;
use crate::profile::{Profile, ProjectProfileDefaults};
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalAgentRecord {
    pub name: String,
    pub file_location: String,
    pub description: Option<String>,
    pub trust_status: TrustStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalAgentsState {
    pub agents: Vec<GlobalAgentRecord>,
    pub scan_root: String,
    pub refreshed_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone)]
struct DiscoveredGlobalAgent {
    name: String,
    file_location: PathBuf,
    description: Option<String>,
    trust_status: TrustStatus,
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

            CREATE TABLE IF NOT EXISTS global_agents (
                file_location TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                description TEXT,
                trust_status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS global_agent_scan_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                scan_root TEXT NOT NULL,
                refreshed_at TEXT,
                error_message TEXT
            );
            "#,
        )?;
        self.ensure_cache_columns()?;
        self.ensure_global_agent_columns()?;
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

    fn ensure_global_agent_columns(&self) -> Result<()> {
        let connection = self.open()?;

        let agent_columns = {
            let mut stmt = connection.prepare("PRAGMA table_info(global_agents)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut columns = Vec::new();
            for row in rows {
                columns.push(row?);
            }
            columns
        };

        if !agent_columns.iter().any(|name| name == "description") {
            connection.execute(
                "ALTER TABLE global_agents ADD COLUMN description TEXT",
                [],
            )?;
        }
        if !agent_columns.iter().any(|name| name == "trust_status") {
            connection.execute(
                "ALTER TABLE global_agents ADD COLUMN trust_status TEXT NOT NULL DEFAULT 'Untrusted'",
                [],
            )?;
        }

        let scan_columns = {
            let mut stmt = connection.prepare("PRAGMA table_info(global_agent_scan_state)")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
            let mut columns = Vec::new();
            for row in rows {
                columns.push(row?);
            }
            columns
        };

        if !scan_columns.iter().any(|name| name == "refreshed_at") {
            connection.execute(
                "ALTER TABLE global_agent_scan_state ADD COLUMN refreshed_at TEXT",
                [],
            )?;
        }
        if !scan_columns.iter().any(|name| name == "error_message") {
            connection.execute(
                "ALTER TABLE global_agent_scan_state ADD COLUMN error_message TEXT",
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

    pub fn refresh_global_agents(&self) -> Result<GlobalAgentsState> {
        let scan_root = match self.resolve_global_agents_root() {
            Ok(scan_root) => scan_root,
            Err(err) => {
                let default_scan_root = self.default_global_agents_root();
                self.persist_global_agent_scan_error(&default_scan_root, &err.to_string())?;
                return Err(err);
            }
        };

        match self.collect_global_agents(&scan_root) {
            Ok(agents) => {
                let refreshed_at = Utc::now().to_rfc3339();
                self.persist_global_agents(&agents, &scan_root, &refreshed_at)?;
                Ok(GlobalAgentsState {
                    agents,
                    scan_root: scan_root.display().to_string(),
                    refreshed_at: Some(refreshed_at),
                    error_message: None,
                })
            }
            Err(err) => {
                self.persist_global_agent_scan_error(&scan_root, &err.to_string())?;
                Err(err)
            }
        }
    }

    pub fn load_global_agents_state(&self) -> Result<GlobalAgentsState> {
        let connection = self.open()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT agent_name, file_location, description, trust_status
            FROM global_agents
            ORDER BY lower(agent_name) ASC, lower(file_location) ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GlobalAgentRecord {
                name: row.get(0)?,
                file_location: row.get(1)?,
                description: row.get(2)?,
                trust_status: Self::trust_status_from_str(&row.get::<_, String>(3)?),
            })
        })?;

        let mut agents = Vec::new();
        for row in rows {
            agents.push(row?);
        }

        let scan_state = connection
            .query_row(
                r#"
                SELECT scan_root, refreshed_at, error_message
                FROM global_agent_scan_state
                WHERE id = 1
                "#,
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;

        let default_scan_root = self.default_global_agents_root();
        let (scan_root, refreshed_at, error_message) = scan_state.unwrap_or((
            default_scan_root.display().to_string(),
            None,
            None,
        ));

        Ok(GlobalAgentsState {
            agents,
            scan_root,
            refreshed_at,
            error_message,
        })
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

    fn resolve_global_agents_root(&self) -> Result<PathBuf> {
        Ok(Self::resolve_user_home_dir()?.join(".copilot").join("agents"))
    }

    fn default_global_agents_root(&self) -> PathBuf {
        Self::resolve_user_home_dir()
            .map(|home| home.join(".copilot").join("agents"))
            .unwrap_or_else(|_| PathBuf::from(".copilot").join("agents"))
    }

    fn resolve_user_home_dir() -> Result<PathBuf> {
        if let Some(home) = std::env::var_os("USERPROFILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return Ok(home);
        }

        if let Some(home) = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return Ok(home);
        }

        let home_drive = std::env::var_os("HOMEDRIVE");
        let home_path = std::env::var_os("HOMEPATH");
        if let (Some(home_drive), Some(home_path)) = (home_drive, home_path) {
            let mut combined = PathBuf::from(home_drive);
            combined.push(PathBuf::from(home_path));
            if !combined.as_os_str().is_empty() {
                return Ok(combined);
            }
        }

        Err(crate::error::TupiError::AgentRead(
            "could not resolve the user home directory".into(),
        ))
    }

    fn normalize_profile(&self, profile: Profile) -> Result<Profile> {
        let id = profile.id.trim().to_string();
        let name = profile.name.trim().to_string();
        let project_location = Self::normalize_optional_text(profile.project_location);
        if name.is_empty() {
            return Err(crate::error::TupiError::ProfileValidation(
                "project display name is required".into(),
            ));
        }
        if project_location.is_none() {
            return Err(crate::error::TupiError::ProfileValidation(
                "project location is required".into(),
            ));
        }

        Ok(Profile {
            id: if id.is_empty() {
                self.generate_profile_id()?
            } else {
                id
            },
            name,
            project_location,
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

    fn trust_status_label(value: &TrustStatus) -> &'static str {
        match value {
            TrustStatus::Trusted => "Trusted",
            TrustStatus::Untrusted => "Untrusted",
            TrustStatus::Stale => "Stale",
            TrustStatus::Invalid => "Invalid",
            TrustStatus::Missing => "Missing",
        }
    }

    fn trust_status_from_str(value: &str) -> TrustStatus {
        match value {
            "Trusted" => TrustStatus::Trusted,
            "Stale" => TrustStatus::Stale,
            "Invalid" => TrustStatus::Invalid,
            "Missing" => TrustStatus::Missing,
            _ => TrustStatus::Untrusted,
        }
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
                trust_status: Self::trust_status_from_str(&trust_status),
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

    fn collect_global_agents(&self, agents_root: &Path) -> Result<Vec<GlobalAgentRecord>> {
        let trusted_agent_bytes = self.load_trusted_agent_bytes().unwrap_or_default();

        if !agents_root.exists() {
            return Ok(Vec::new());
        }
        if !agents_root.is_dir() {
            return Err(crate::error::TupiError::AgentRead(format!(
                "{} is not a directory",
                agents_root.display()
            )));
        }

        let mut discovered = Vec::new();
        Self::collect_global_agents_from_directory(
            agents_root,
            &trusted_agent_bytes,
            &mut discovered,
        )?;
        discovered.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| {
                    left.file_location
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .cmp(&right.file_location.to_string_lossy().to_ascii_lowercase())
                })
        });

        Ok(discovered
            .into_iter()
            .map(|agent| GlobalAgentRecord {
                name: agent.name,
                file_location: agent.file_location.display().to_string(),
                description: agent.description,
                trust_status: agent.trust_status,
            })
            .collect())
    }

    fn collect_global_agents_from_directory(
        current_dir: &Path,
        trusted_agent_bytes: &HashSet<Vec<u8>>,
        discovered: &mut Vec<DiscoveredGlobalAgent>,
    ) -> Result<()> {
        let entries = fs::read_dir(current_dir).map_err(|err| {
            crate::error::TupiError::AgentRead(format!("{} ({})", current_dir.display(), err))
        })?;

        for entry in entries {
            let entry = entry.map_err(|err| crate::error::TupiError::AgentRead(err.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| crate::error::TupiError::AgentRead(err.to_string()))?;

            if file_type.is_dir() {
                Self::collect_global_agents_from_directory(&path, trusted_agent_bytes, discovered)?;
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    crate::error::TupiError::AgentValidation(format!(
                        "agent file {} must have a valid filename",
                        path.display()
                    ))
                })?;

            if !file_name.to_ascii_lowercase().ends_with(".agent.md") {
                continue;
            }

            let name = file_name
                .strip_suffix(".agent.md")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    crate::error::TupiError::AgentValidation(format!(
                        "agent file {} must end with a non-empty .agent.md name",
                        path.display()
                    ))
                })?;

            let file_bytes = fs::read(&path).map_err(|err| {
                crate::error::TupiError::AgentRead(format!("{} ({})", path.display(), err))
            })?;
            let contents = String::from_utf8(file_bytes.clone()).map_err(|err| {
                crate::error::TupiError::AgentRead(format!(
                    "{} is not valid UTF-8 ({})",
                    path.display(),
                    err
                ))
            })?;

            discovered.push(DiscoveredGlobalAgent {
                name,
                file_location: path,
                description: Self::extract_frontmatter_description(&contents)?,
                trust_status: if trusted_agent_bytes.contains(&file_bytes) {
                    TrustStatus::Trusted
                } else {
                    TrustStatus::Untrusted
                },
            });
        }

        Ok(())
    }

    fn extract_frontmatter_description(contents: &str) -> Result<Option<String>> {
        let mut lines = contents.trim_start_matches('\u{feff}').lines();
        let Some(first_line) = lines.next() else {
            return Ok(None);
        };

        if first_line.trim() != "---" {
            return Ok(None);
        }

        let mut frontmatter_lines = Vec::new();
        let mut found_closing_delimiter = false;
        for line in lines {
            if matches!(line.trim(), "---" | "...") {
                found_closing_delimiter = true;
                break;
            }
            frontmatter_lines.push(line);
        }

        if !found_closing_delimiter {
            return Err(crate::error::TupiError::AgentValidation(
                "agent frontmatter is missing a closing delimiter".into(),
            ));
        }

        if frontmatter_lines.is_empty() {
            return Ok(None);
        }

        let frontmatter = frontmatter_lines.join("\n");
        let parsed: serde_yaml::Value = serde_yaml::from_str(&frontmatter).map_err(|err| {
            crate::error::TupiError::AgentValidation(format!(
                "agent frontmatter could not be parsed: {}",
                err
            ))
        })?;

        let Some(description) = parsed.get("description") else {
            return Ok(None);
        };

        let description = description.as_str().ok_or_else(|| {
            crate::error::TupiError::AgentValidation(
                "agent frontmatter description must be a string".into(),
            )
        })?;

        Ok(Self::normalize_optional_text(Some(description.to_string())))
    }

    fn load_trusted_agent_bytes(&self) -> Result<HashSet<Vec<u8>>> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let mut trusted_agent_bytes = HashSet::new();

        for agent in &catalog.agents {
            let Some(workspace) = self.cached_marketplace_workspace(&agent.marketplace) else {
                continue;
            };
            let Some(agent_path) = Self::resolve_catalog_agent_file_path(&workspace, &agent.path) else {
                continue;
            };

            let Ok(bytes) = fs::read(&agent_path) else {
                continue;
            };
            trusted_agent_bytes.insert(bytes);
        }

        Ok(trusted_agent_bytes)
    }

    fn cached_marketplace_workspace(&self, marketplace_id: &str) -> Option<PathBuf> {
        let workspace = self
            .cache_dir
            .join("marketplaces")
            .join(Self::sanitize_cache_key(marketplace_id));

        if workspace.is_dir() {
            Some(workspace)
        } else {
            None
        }
    }

    fn resolve_catalog_agent_file_path(workspace: &Path, catalog_path: &str) -> Option<PathBuf> {
        let direct_path = workspace.join(catalog_path);
        let mut candidates = vec![
            direct_path.clone(),
            workspace.join(format!("{catalog_path}.md")),
            workspace.join(format!("{catalog_path}.agent.md")),
        ];

        if direct_path.is_dir() {
            candidates.push(direct_path.join("README.md"));
            candidates.push(direct_path.join("README.agent.md"));
        }

        candidates.into_iter().find(|candidate| candidate.is_file())
    }

    fn persist_global_agents(
        &self,
        agents: &[GlobalAgentRecord],
        scan_root: &Path,
        refreshed_at: &str,
    ) -> Result<()> {
        let mut connection = self.open()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM global_agents", [])?;

        for agent in agents {
            tx.execute(
                r#"
                INSERT INTO global_agents (file_location, agent_name, description, trust_status)
                VALUES (?1, ?2, ?3, ?4)
                "#,
                params![
                    &agent.file_location,
                    &agent.name,
                    &agent.description,
                    Self::trust_status_label(&agent.trust_status)
                ],
            )?;
        }

        tx.execute(
            r#"
            INSERT INTO global_agent_scan_state (id, scan_root, refreshed_at, error_message)
            VALUES (1, ?1, ?2, NULL)
            ON CONFLICT(id) DO UPDATE SET
                scan_root = excluded.scan_root,
                refreshed_at = excluded.refreshed_at,
                error_message = NULL
            "#,
            params![scan_root.display().to_string(), refreshed_at],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn persist_global_agent_scan_error(&self, scan_root: &Path, error_message: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO global_agent_scan_state (id, scan_root, refreshed_at, error_message)
            VALUES (
                1,
                ?1,
                (SELECT refreshed_at FROM global_agent_scan_state WHERE id = 1),
                ?2
            )
            ON CONFLICT(id) DO UPDATE SET
                scan_root = excluded.scan_root,
                error_message = excluded.error_message
            "#,
            params![scan_root.display().to_string(), error_message],
        )?;
        Ok(())
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
    use crate::trust::TrustStatus;
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
                project_location: Some(String::from("  D:\\workspace\\myProject  ")),
                description: Some(String::from("  Example repo  ")),
                enabled: true,
                version: Some(String::from("1")),
                catalog_revision: Some(String::from("catalog-1")),
                selected_assets: vec![String::from(" reviewer "), String::new()],
            })
            .unwrap();

        assert!(!saved.id.is_empty());
        assert_eq!(saved.name, "myProject");
        assert_eq!(
            saved.project_location.as_deref(),
            Some("D:\\workspace\\myProject")
        );
        assert_eq!(saved.description.as_deref(), Some("Example repo"));
        assert_eq!(saved.selected_assets, vec![String::from("reviewer")]);

        let profiles = state.read_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, saved.id);
        assert_eq!(
            profiles[0].project_location.as_deref(),
            Some("D:\\workspace\\myProject")
        );
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
        assert_eq!(profiles[0].project_location, None);
        assert_eq!(profiles[0].description, None);

        drop(connection);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_profile_without_project_location() {
        let root = unique_temp_dir("missing-project-location");
        let state = make_test_state(&root);

        let error = state
            .upsert_profile(crate::profile::Profile {
                id: String::new(),
                name: String::from("myProject"),
                project_location: None,
                description: None,
                enabled: true,
                version: Some(String::from("1")),
                catalog_revision: Some(String::from("catalog-1")),
                selected_assets: Vec::new(),
            })
            .unwrap_err();

        assert!(error.to_string().contains("project location is required"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discovers_global_agents_with_frontmatter_description_and_trust_status() {
        let root = unique_temp_dir("global-agents-trusted");
        let state = make_test_state_with_catalog(
            &root,
            r#"
version: 1
catalogRevision: trusted-test
marketplaces:
  - id: official
    name: Official
    repository: https://example.com/official.git
    branch: main
    revision: abc123
agents:
  - id: planner
    marketplace: official
    path: agents/planner
    version: 1.0.0
    revision: abc123
"#,
        );

        let trusted_workspace = root
            .join(".tupi")
            .join("catalog-cache")
            .join("marketplaces")
            .join("official")
            .join("agents");
        fs::create_dir_all(&trusted_workspace).unwrap();
        let trusted_contents = "---\ndescription: Trusted planner\n---\n# Planner\n";
        fs::write(trusted_workspace.join("planner.agent.md"), trusted_contents).unwrap();

        let global_agents_root = root.join("user-home").join(".copilot").join("agents");
        fs::create_dir_all(&global_agents_root).unwrap();
        fs::write(global_agents_root.join("planner.agent.md"), trusted_contents).unwrap();

        let agents = state.collect_global_agents(&global_agents_root).unwrap();

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "planner");
        assert_eq!(agents[0].description.as_deref(), Some("Trusted planner"));
        assert_eq!(agents[0].trust_status, TrustStatus::Trusted);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn marks_global_agents_untrusted_when_bytes_do_not_match_trusted_source() {
        let root = unique_temp_dir("global-agents-untrusted");
        let state = make_test_state_with_catalog(
            &root,
            r#"
version: 1
catalogRevision: trusted-test
marketplaces:
  - id: official
    name: Official
    repository: https://example.com/official.git
    branch: main
    revision: abc123
agents:
  - id: reviewer
    marketplace: official
    path: agents/reviewer
    version: 1.0.0
    revision: abc123
"#,
        );

        let trusted_workspace = root
            .join(".tupi")
            .join("catalog-cache")
            .join("marketplaces")
            .join("official")
            .join("agents");
        fs::create_dir_all(&trusted_workspace).unwrap();
        fs::write(
            trusted_workspace.join("reviewer.agent.md"),
            "---\ndescription: Trusted reviewer\n---\n# Reviewer\n",
        )
        .unwrap();

        let global_agents_root = root.join("user-home").join(".copilot").join("agents");
        fs::create_dir_all(&global_agents_root).unwrap();
        fs::write(
            global_agents_root.join("reviewer.agent.md"),
            "---\ndescription: Local reviewer\n---\n# Reviewer\nchanged\n",
        )
        .unwrap();

        let agents = state.collect_global_agents(&global_agents_root).unwrap();

        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "reviewer");
        assert_eq!(agents[0].description.as_deref(), Some("Local reviewer"));
        assert_eq!(agents[0].trust_status, TrustStatus::Untrusted);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_scan_state_and_global_agents() {
        let root = unique_temp_dir("global-agents-persist");
        let state = make_test_state(&root);
        let global_agents_root = root.join("user-home").join(".copilot").join("agents");
        fs::create_dir_all(&global_agents_root).unwrap();
        fs::write(
            global_agents_root.join("copilot.agent.md"),
            "---\ndescription: Local Copilot\n---\n# Copilot\n",
        )
        .unwrap();

        let agents = state.collect_global_agents(&global_agents_root).unwrap();
        state
            .persist_global_agents(&agents, &global_agents_root, "2026-01-01T00:00:00Z")
            .unwrap();

        let stored = state.load_global_agents_state().unwrap();

        assert_eq!(stored.scan_root, global_agents_root.display().to_string());
        assert_eq!(stored.refreshed_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(stored.error_message, None);
        assert_eq!(stored.agents.len(), 1);
        assert_eq!(stored.agents[0].name, "copilot");
        assert_eq!(stored.agents[0].description.as_deref(), Some("Local Copilot"));
        assert_eq!(stored.agents[0].trust_status, TrustStatus::Untrusted);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stores_null_description_when_frontmatter_description_is_missing() {
        let contents = "---\nname: Planner\n---\n# Planner\n";

        let description = AppState::extract_frontmatter_description(contents).unwrap();

        assert_eq!(description, None);
    }

    fn make_test_state(root: &Path) -> AppState {
        make_test_state_with_catalog(
            root,
            "version: 1\ncatalogRevision: test\nmarketplaces: []\nagents: []\n",
        )
    }

    fn make_test_state_with_catalog(root: &Path, catalog_contents: &str) -> AppState {
        let state_dir = root.join(".tupi");
        let cache_dir = state_dir.join("catalog-cache");
        let catalog_dir = root.join("catalog");
        fs::create_dir_all(&cache_dir).unwrap();
        fs::create_dir_all(&catalog_dir).unwrap();
        fs::write(catalog_dir.join("trusted-assets.yaml"), catalog_contents).unwrap();

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
