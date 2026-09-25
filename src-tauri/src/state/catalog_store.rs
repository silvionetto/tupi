use super::{AppState, CatalogState, RefreshRecord};
use crate::catalog::{load_catalog_from_str, summarize, CatalogSummary};
use crate::error::Result;
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::fs;

impl AppState {
    pub(crate) fn initialize(&self) -> Result<()> {
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

            CREATE TABLE IF NOT EXISTS installed_marketplaces (
                directory_location TEXT PRIMARY KEY,
                marketplace_id TEXT NOT NULL,
                marketplace_name TEXT NOT NULL,
                repository TEXT,
                trust_status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS installed_marketplace_plugins (
                marketplace_directory_location TEXT NOT NULL,
                plugin_name TEXT NOT NULL,
                plugin_directory_location TEXT NOT NULL,
                PRIMARY KEY (marketplace_directory_location, plugin_directory_location)
            );

            CREATE TABLE IF NOT EXISTS installed_marketplace_plugin_skills (
                plugin_directory_location TEXT NOT NULL,
                skill_name TEXT NOT NULL,
                skill_directory_location TEXT NOT NULL,
                PRIMARY KEY (plugin_directory_location, skill_directory_location)
            );

            CREATE TABLE IF NOT EXISTS installed_marketplace_scan_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                scan_root TEXT NOT NULL,
                refreshed_at TEXT,
                error_message TEXT
            );

            CREATE TABLE IF NOT EXISTS marketplace_agents (
                marketplace_id TEXT NOT NULL,
                agent_name TEXT NOT NULL,
                description TEXT,
                trust_status TEXT NOT NULL,
                PRIMARY KEY (marketplace_id, agent_name)
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
            connection.execute("ALTER TABLE global_agents ADD COLUMN description TEXT", [])?;
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
        self.refresh_marketplace_agents()?;
        let _ = self.refresh_installed_marketplaces();
        let _ = self.refresh_global_agents();

        Ok(RefreshRecord {
            summary,
            trust_status: TrustStatus::Trusted,
            source_repository,
            source_branch,
            refreshed_at,
        })
    }

    pub(crate) fn read_active_catalog_contents(&self) -> Result<String> {
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

        if let Some((
            summary_json,
            trust_status,
            source_repository,
            source_branch,
            refreshed_at,
            stale,
        )) = row
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
}
