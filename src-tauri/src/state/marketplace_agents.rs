use super::{AgentOption, AppState, MarketplaceAgentRecord, MarketplaceOption};
use crate::catalog::{load_catalog_from_str, Marketplace, TrustedCatalog};
use crate::error::Result;
use crate::trust::TrustStatus;
use rusqlite::params;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct DiscoveredMarketplaceAgent {
    name: String,
    description: Option<String>,
}

impl AppState {
    pub fn refresh_marketplace_agents(&self) -> Result<()> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let mut agents_by_marketplace = BTreeMap::new();

        for marketplace in &catalog.marketplaces {
            let workspace = self.sync_repository_workspace(
                "marketplaces",
                &marketplace.id,
                &marketplace.repository,
                &marketplace.branch,
                "trusted marketplace repository",
            )?;
            let agents_dir = workspace.join("agents");
            let agents = if agents_dir.is_dir() {
                Self::discover_marketplace_agents(&agents_dir)?
            } else {
                Vec::new()
            };
            agents_by_marketplace.insert(marketplace.id.clone(), agents);
        }

        self.persist_marketplace_agents(&agents_by_marketplace)
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
        let agents_by_marketplace = self.load_marketplace_agents_by_marketplace()?;
        let mut marketplaces = catalog
            .marketplaces
            .iter()
            .map(|marketplace| Self::to_marketplace_option(marketplace, &agents_by_marketplace))
            .collect::<Vec<_>>();
        marketplaces.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        Ok(marketplaces)
    }

    fn trusted_marketplace_agent_roots(
        &self,
        catalog: &TrustedCatalog,
    ) -> Result<Vec<(String, PathBuf)>> {
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

    fn discover_marketplace_agents(agents_root: &Path) -> Result<Vec<MarketplaceAgentRecord>> {
        let mut discovered = BTreeMap::new();
        Self::collect_marketplace_agents_from_directory(agents_root, &mut discovered)?;

        Ok(discovered.into_values().collect())
    }

    fn collect_marketplace_agents_from_directory(
        current_dir: &Path,
        discovered: &mut BTreeMap<String, MarketplaceAgentRecord>,
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
                Self::collect_marketplace_agents_from_directory(&path, discovered)?;
                continue;
            }

            if !file_type.is_file() {
                continue;
            }

            let file_name = path
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    crate::error::TupiError::CatalogValidation(format!(
                        "agent file {} must have a valid filename",
                        path.display()
                    ))
                })?;

            let Some(name) = file_name
                .strip_suffix(".agent.md")
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };

            let contents = fs::read_to_string(&path).map_err(|err| {
                crate::error::TupiError::CatalogRead(format!("{} ({})", path.display(), err))
            })?;
            let agent = DiscoveredMarketplaceAgent {
                name,
                description: Self::extract_frontmatter_description(&contents)?,
            };

            let duplicate_key = agent.name.to_ascii_lowercase();
            if discovered.contains_key(&duplicate_key) {
                return Err(crate::error::TupiError::CatalogValidation(format!(
                    "duplicate trusted marketplace agent {} found under {}",
                    agent.name,
                    current_dir.display()
                )));
            }

            discovered.insert(
                duplicate_key,
                MarketplaceAgentRecord {
                    name: agent.name,
                    description: agent.description,
                    trust_status: TrustStatus::Trusted,
                },
            );
        }

        Ok(())
    }

    fn persist_marketplace_agents(
        &self,
        agents_by_marketplace: &BTreeMap<String, Vec<MarketplaceAgentRecord>>,
    ) -> Result<()> {
        let mut connection = self.open()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM marketplace_agents", [])?;

        for (marketplace_id, agents) in agents_by_marketplace {
            for agent in agents {
                tx.execute(
                    r#"
                    INSERT INTO marketplace_agents (marketplace_id, agent_name, description, trust_status)
                    VALUES (?1, ?2, ?3, ?4)
                    "#,
                    params![
                        marketplace_id,
                        &agent.name,
                        &agent.description,
                        Self::trust_status_label(&agent.trust_status)
                    ],
                )?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn load_marketplace_agents_by_marketplace(
        &self,
    ) -> Result<BTreeMap<String, Vec<MarketplaceAgentRecord>>> {
        let connection = self.open()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT marketplace_id, agent_name, description, trust_status
            FROM marketplace_agents
            ORDER BY lower(marketplace_id) ASC, lower(agent_name) ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                MarketplaceAgentRecord {
                    name: row.get(1)?,
                    description: row.get(2)?,
                    trust_status: Self::trust_status_from_str(&row.get::<_, String>(3)?),
                },
            ))
        })?;

        let mut agents_by_marketplace = BTreeMap::new();
        for row in rows {
            let (marketplace_id, agent) = row?;
            agents_by_marketplace
                .entry(marketplace_id)
                .or_insert_with(Vec::new)
                .push(agent);
        }

        Ok(agents_by_marketplace)
    }

    fn to_marketplace_option(
        marketplace: &Marketplace,
        agents_by_marketplace: &BTreeMap<String, Vec<MarketplaceAgentRecord>>,
    ) -> MarketplaceOption {
        MarketplaceOption {
            id: marketplace.id.clone(),
            name: marketplace.name.clone(),
            repository: marketplace.repository.clone(),
            agents: agents_by_marketplace
                .get(&marketplace.id)
                .cloned()
                .unwrap_or_default(),
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
    use super::{AppState, MarketplaceAgentRecord};
    use crate::state::test_support::make_test_state_with_catalog;
    use crate::trust::TrustStatus;
    use std::collections::BTreeMap;
    use std::fs;

    #[test]
    fn discovers_sorted_markdown_agents_from_nested_directories() {
        let root = crate::state::test_support::unique_temp_dir("agent-discovery");
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
        let root = crate::state::test_support::unique_temp_dir("agent-duplicates");
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
    fn discovers_trusted_marketplace_agents_from_agent_markdown_files() {
        let root = crate::state::test_support::unique_temp_dir("marketplace-agent-discovery");
        let agents_dir = root.join("agents");
        fs::create_dir_all(agents_dir.join("nested")).unwrap();
        fs::write(
            agents_dir.join("planner.agent.md"),
            "---\ndescription: Planner agent\n---\n# Planner\n",
        )
        .unwrap();
        fs::write(
            agents_dir.join("nested").join("reviewer.agent.md"),
            "# Reviewer\n",
        )
        .unwrap();
        fs::write(agents_dir.join("ignored.md"), "# Ignore me\n").unwrap();

        let agents = AppState::discover_marketplace_agents(&agents_dir).unwrap();

        assert_eq!(agents.len(), 2);
        assert_eq!(agents[0].name, "planner");
        assert_eq!(agents[0].description.as_deref(), Some("Planner agent"));
        assert_eq!(agents[0].trust_status, TrustStatus::Trusted);
        assert_eq!(agents[1].name, "reviewer");
        assert_eq!(agents[1].description, None);
        assert_eq!(agents[1].trust_status, TrustStatus::Trusted);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_marketplaces_includes_persisted_marketplace_agents() {
        let root = crate::state::test_support::unique_temp_dir("marketplace-agents-list");
        let state = make_test_state_with_catalog(
            &root,
            r#"
version: 1
catalogRevision: trusted-test
marketplaces:
  - id: awesome-copilot
    name: awesome-copilot
    repository: https://github.com/github/awesome-copilot
    branch: main
    revision: abc123
agents: []
"#,
        );

        let mut agents_by_marketplace = BTreeMap::new();
        agents_by_marketplace.insert(
            String::from("awesome-copilot"),
            vec![MarketplaceAgentRecord {
                name: String::from("planner"),
                description: Some(String::from("Trusted planner")),
                trust_status: TrustStatus::Trusted,
            }],
        );
        state.persist_marketplace_agents(&agents_by_marketplace).unwrap();

        let marketplaces = state.list_marketplaces().unwrap();

        assert_eq!(marketplaces.len(), 1);
        assert_eq!(marketplaces[0].id, "awesome-copilot");
        assert_eq!(marketplaces[0].agents.len(), 1);
        assert_eq!(marketplaces[0].agents[0].name, "planner");
        assert_eq!(
            marketplaces[0].agents[0].description.as_deref(),
            Some("Trusted planner")
        );
        assert_eq!(marketplaces[0].agents[0].trust_status, TrustStatus::Trusted);

        fs::remove_dir_all(root).unwrap();
    }
}
