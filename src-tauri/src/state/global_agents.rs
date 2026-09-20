use super::{AppState, GlobalAgentRecord, GlobalAgentsState};
use crate::catalog::load_catalog_from_str;
use crate::error::Result;
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct DiscoveredGlobalAgent {
    name: String,
    file_location: PathBuf,
    description: Option<String>,
    trust_status: TrustStatus,
}

impl AppState {
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

    pub(crate) fn extract_frontmatter_description(contents: &str) -> Result<Option<String>> {
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
            let Some(agent_path) = Self::resolve_catalog_agent_file_path(&workspace, &agent.path)
            else {
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
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::state::test_support::{make_test_state, make_test_state_with_catalog};
    use crate::trust::TrustStatus;
    use std::fs;

    #[test]
    fn discovers_global_agents_with_frontmatter_description_and_trust_status() {
        let root = crate::state::test_support::unique_temp_dir("global-agents-trusted");
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
        let root = crate::state::test_support::unique_temp_dir("global-agents-untrusted");
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
        let root = crate::state::test_support::unique_temp_dir("global-agents-persist");
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
}
