use super::{
    AppState, InstalledMarketplaceRecord, InstalledMarketplacesState, InstalledPluginRecord,
};
use crate::catalog::{load_catalog_from_str, Marketplace};
use crate::error::Result;
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

impl AppState {
    pub fn refresh_installed_marketplaces(&self) -> Result<InstalledMarketplacesState> {
        let scan_root = match self.resolve_installed_plugins_root() {
            Ok(scan_root) => scan_root,
            Err(err) => {
                let default_scan_root = self.default_installed_plugins_root();
                self.persist_installed_marketplace_scan_error(
                    &default_scan_root,
                    &err.to_string(),
                )?;
                return Err(err);
            }
        };

        match self.collect_installed_marketplaces(&scan_root) {
            Ok(marketplaces) => {
                let refreshed_at = Utc::now().to_rfc3339();
                self.persist_installed_marketplaces(&marketplaces, &scan_root, &refreshed_at)?;
                Ok(InstalledMarketplacesState {
                    marketplaces,
                    scan_root: scan_root.display().to_string(),
                    refreshed_at: Some(refreshed_at),
                    error_message: None,
                })
            }
            Err(err) => {
                self.persist_installed_marketplace_scan_error(&scan_root, &err.to_string())?;
                Err(err)
            }
        }
    }

    pub fn load_installed_marketplaces_state(&self) -> Result<InstalledMarketplacesState> {
        let connection = self.open()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT marketplace_id, marketplace_name, directory_location, repository, trust_status
            FROM installed_marketplaces
            ORDER BY lower(marketplace_name) ASC, lower(marketplace_id) ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(InstalledMarketplaceRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                directory_location: row.get(2)?,
                repository: row.get(3)?,
                trust_status: Self::trust_status_from_str(&row.get::<_, String>(4)?),
                plugins: Vec::new(),
            })
        })?;

        let mut marketplaces = Vec::new();
        for row in rows {
            marketplaces.push(row?);
        }

        let mut plugin_stmt = connection.prepare(
            r#"
            SELECT marketplace_directory_location, plugin_name, plugin_directory_location
            FROM installed_marketplace_plugins
            ORDER BY lower(marketplace_directory_location) ASC, lower(plugin_name) ASC, lower(plugin_directory_location) ASC
            "#,
        )?;
        let plugin_rows = plugin_stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                InstalledPluginRecord {
                    name: row.get(1)?,
                    directory_location: row.get(2)?,
                },
            ))
        })?;

        let mut plugins_by_marketplace = BTreeMap::new();
        for row in plugin_rows {
            let (marketplace_directory_location, plugin) = row?;
            plugins_by_marketplace
                .entry(marketplace_directory_location)
                .or_insert_with(Vec::new)
                .push(plugin);
        }

        for marketplace in &mut marketplaces {
            marketplace.plugins = plugins_by_marketplace
                .remove(&marketplace.directory_location)
                .unwrap_or_default();
        }

        let scan_state = connection
            .query_row(
                r#"
                SELECT scan_root, refreshed_at, error_message
                FROM installed_marketplace_scan_state
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

        let default_scan_root = self.default_installed_plugins_root();
        let (scan_root, refreshed_at, error_message) =
            scan_state.unwrap_or((default_scan_root.display().to_string(), None, None));

        Ok(InstalledMarketplacesState {
            marketplaces,
            scan_root,
            refreshed_at,
            error_message,
        })
    }

    fn collect_installed_marketplaces(
        &self,
        scan_root: &Path,
    ) -> Result<Vec<InstalledMarketplaceRecord>> {
        let trusted_marketplaces = self.load_catalog_marketplaces_by_id().unwrap_or_default();

        if !scan_root.exists() {
            return Ok(Vec::new());
        }
        if !scan_root.is_dir() {
            return Err(crate::error::TupiError::PluginRead(format!(
                "{} is not a directory",
                scan_root.display()
            )));
        }

        let mut marketplaces = Vec::new();
        let entries = fs::read_dir(scan_root).map_err(|err| {
            crate::error::TupiError::PluginRead(format!("{} ({})", scan_root.display(), err))
        })?;

        for entry in entries {
            let entry =
                entry.map_err(|err| crate::error::TupiError::PluginRead(err.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| crate::error::TupiError::PluginRead(err.to_string()))?;

            if !file_type.is_dir() {
                continue;
            }

            let marketplace_id = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    crate::error::TupiError::PluginValidation(format!(
                        "installed marketplace {} must have a valid directory name",
                        path.display()
                    ))
                })?;

            let matched_marketplace =
                trusted_marketplaces.get(&marketplace_id.to_ascii_lowercase());
            let (name, repository, trust_status) = match matched_marketplace {
                Some(marketplace) => (
                    marketplace.name.clone(),
                    Some(marketplace.repository.clone()),
                    TrustStatus::Trusted,
                ),
                None => (marketplace_id.clone(), None, TrustStatus::Untrusted),
            };

            marketplaces.push(InstalledMarketplaceRecord {
                id: marketplace_id,
                name,
                directory_location: path.display().to_string(),
                repository,
                trust_status,
                plugins: Self::collect_installed_marketplace_plugins(&path)?,
            });
        }

        marketplaces.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| {
                    left.id
                        .to_ascii_lowercase()
                        .cmp(&right.id.to_ascii_lowercase())
                })
        });

        Ok(marketplaces)
    }

    fn collect_installed_marketplace_plugins(
        marketplace_root: &Path,
    ) -> Result<Vec<InstalledPluginRecord>> {
        if !marketplace_root.exists() {
            return Ok(Vec::new());
        }
        if !marketplace_root.is_dir() {
            return Err(crate::error::TupiError::PluginRead(format!(
                "{} is not a directory",
                marketplace_root.display()
            )));
        }

        let mut plugins = Vec::new();
        let entries = fs::read_dir(marketplace_root).map_err(|err| {
            crate::error::TupiError::PluginRead(format!("{} ({})", marketplace_root.display(), err))
        })?;

        for entry in entries {
            let entry =
                entry.map_err(|err| crate::error::TupiError::PluginRead(err.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| crate::error::TupiError::PluginRead(err.to_string()))?;

            if !file_type.is_dir() {
                continue;
            }

            let plugin_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    crate::error::TupiError::PluginValidation(format!(
                        "installed plugin {} must have a valid directory name",
                        path.display()
                    ))
                })?;

            plugins.push(InstalledPluginRecord {
                name: plugin_name,
                directory_location: path.display().to_string(),
            });
        }

        plugins.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| {
                    left.directory_location
                        .to_ascii_lowercase()
                        .cmp(&right.directory_location.to_ascii_lowercase())
                })
        });

        Ok(plugins)
    }

    fn load_catalog_marketplaces_by_id(&self) -> Result<BTreeMap<String, Marketplace>> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let mut marketplaces = BTreeMap::new();

        for marketplace in catalog.marketplaces {
            marketplaces.insert(marketplace.id.to_ascii_lowercase(), marketplace);
        }

        Ok(marketplaces)
    }

    fn persist_installed_marketplaces(
        &self,
        marketplaces: &[InstalledMarketplaceRecord],
        scan_root: &Path,
        refreshed_at: &str,
    ) -> Result<()> {
        let mut connection = self.open()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM installed_marketplaces", [])?;
        tx.execute("DELETE FROM installed_marketplace_plugins", [])?;

        for marketplace in marketplaces {
            tx.execute(
                r#"
                INSERT INTO installed_marketplaces (
                    marketplace_id, marketplace_name, directory_location, repository, trust_status
                )
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    &marketplace.id,
                    &marketplace.name,
                    &marketplace.directory_location,
                    &marketplace.repository,
                    Self::trust_status_label(&marketplace.trust_status)
                ],
            )?;

            for plugin in &marketplace.plugins {
                    tx.execute(
                        r#"
                        INSERT INTO installed_marketplace_plugins (
                            marketplace_directory_location, plugin_name, plugin_directory_location
                        )
                        VALUES (?1, ?2, ?3)
                        "#,
                        params![
                            &marketplace.directory_location,
                            &plugin.name,
                            &plugin.directory_location,
                        ],
                    )?;
            }
        }

        tx.execute(
            r#"
            INSERT INTO installed_marketplace_scan_state (id, scan_root, refreshed_at, error_message)
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

    fn persist_installed_marketplace_scan_error(
        &self,
        scan_root: &Path,
        error_message: &str,
    ) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO installed_marketplace_scan_state (id, scan_root, refreshed_at, error_message)
            VALUES (
                1,
                ?1,
                (SELECT refreshed_at FROM installed_marketplace_scan_state WHERE id = 1),
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
}

#[cfg(test)]
mod tests {
    use crate::state::test_support::{make_test_state, make_test_state_with_catalog};
    use crate::trust::TrustStatus;
    use std::fs;

    #[test]
    fn discovers_installed_marketplaces_and_matches_catalog_trust() {
        let root = crate::state::test_support::unique_temp_dir("installed-marketplaces-trusted");
        let state = make_test_state_with_catalog(
            &root,
            r#"
version: 1
catalogRevision: trusted-test
marketplaces:
  - id: awesome-copilot
    name: Awesome Copilot
    repository: https://github.com/github/awesome-copilot
    branch: main
    revision: abc123
agents: []
"#,
        );

        let installed_root = root
            .join("user-home")
            .join(".copilot")
            .join("installed-plugins");
        fs::create_dir_all(installed_root.join("awesome-copilot")).unwrap();
        fs::create_dir_all(installed_root.join("local-only")).unwrap();
        fs::create_dir_all(
            installed_root
                .join("awesome-copilot")
                .join("plugins")
                .join("nested"),
        )
        .unwrap();
        fs::create_dir_all(installed_root.join("awesome-copilot").join("editor")).unwrap();

        let marketplaces = state
            .collect_installed_marketplaces(&installed_root)
            .unwrap();

        assert_eq!(marketplaces.len(), 2);
        assert_eq!(marketplaces[0].id, "awesome-copilot");
        assert_eq!(marketplaces[0].name, "Awesome Copilot");
        assert_eq!(
            marketplaces[0].repository.as_deref(),
            Some("https://github.com/github/awesome-copilot")
        );
        assert_eq!(marketplaces[0].trust_status, TrustStatus::Trusted);
        assert_eq!(marketplaces[0].plugins.len(), 2);
        assert_eq!(marketplaces[0].plugins[0].name, "editor");
        assert_eq!(marketplaces[0].plugins[1].name, "plugins");
        assert_eq!(marketplaces[1].id, "local-only");
        assert_eq!(marketplaces[1].name, "local-only");
        assert_eq!(marketplaces[1].repository, None);
        assert_eq!(marketplaces[1].trust_status, TrustStatus::Untrusted);
        assert!(marketplaces[1].plugins.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_installed_marketplaces_root_returns_empty_inventory() {
        let root = crate::state::test_support::unique_temp_dir("installed-marketplaces-missing");
        let state = make_test_state(&root);
        let installed_root = root
            .join("user-home")
            .join(".copilot")
            .join("installed-plugins");

        let marketplaces = state
            .collect_installed_marketplaces(&installed_root)
            .unwrap();

        assert!(marketplaces.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn persists_installed_marketplace_scan_state() {
        let root = crate::state::test_support::unique_temp_dir("installed-marketplaces-persist");
        let state = make_test_state(&root);
        let installed_root = root
            .join("user-home")
            .join(".copilot")
            .join("installed-plugins");
        fs::create_dir_all(
            installed_root
                .join("awesome-copilot")
                .join("assistant"),
        )
        .unwrap();

        let marketplaces = state
            .collect_installed_marketplaces(&installed_root)
            .unwrap();
        state
            .persist_installed_marketplaces(&marketplaces, &installed_root, "2026-01-01T00:00:00Z")
            .unwrap();

        let stored = state.load_installed_marketplaces_state().unwrap();

        assert_eq!(stored.scan_root, installed_root.display().to_string());
        assert_eq!(stored.refreshed_at.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(stored.error_message, None);
        assert_eq!(stored.marketplaces.len(), 1);
        assert_eq!(stored.marketplaces[0].id, "awesome-copilot");
        assert_eq!(stored.marketplaces[0].name, "awesome-copilot");
        assert_eq!(stored.marketplaces[0].trust_status, TrustStatus::Untrusted);
        assert_eq!(stored.marketplaces[0].plugins.len(), 1);
        assert_eq!(stored.marketplaces[0].plugins[0].name, "assistant");
        assert_eq!(
            stored.marketplaces[0].plugins[0].directory_location,
            installed_root
                .join("awesome-copilot")
                .join("assistant")
                .display()
                .to_string()
        );

        fs::remove_dir_all(root).unwrap();
    }
}
