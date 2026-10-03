use super::AppState;
use crate::error::{Result, TupiError};
use std::fs;
use std::path::{Component, Path};

impl AppState {
    pub fn remove_local_asset(&self, kind: &str, source: &str, location: &str) -> Result<()> {
        let (root, discovered_location) = match (source, kind) {
            ("global", "agent") => {
                let state = self.refresh_global_agents()?;
                let agent = state
                    .agents
                    .iter()
                    .find(|agent| agent.file_location == location)
                    .ok_or_else(|| {
                        TupiError::AgentValidation(
                            "agent is no longer present in the global inventory".into(),
                        )
                    })?;
                (
                    Path::new(&state.scan_root).to_path_buf(),
                    agent.file_location.clone(),
                )
            }
            ("global", "skill") => {
                let state = self.refresh_global_skills()?;
                let skill = state
                    .skills
                    .iter()
                    .find(|skill| skill.directory_location == location)
                    .ok_or_else(|| {
                        TupiError::PluginValidation(
                            "skill is no longer present in the global inventory".into(),
                        )
                    })?;
                (
                    Path::new(&state.scan_root).to_path_buf(),
                    skill.directory_location.clone(),
                )
            }
            ("marketplace", "agent") | ("marketplace", "skill") => {
                let state = self.refresh_installed_marketplaces()?;
                let marketplace_root = Path::new(&state.scan_root).to_path_buf();
                let found = state.marketplaces.iter().find_map(|marketplace| {
                    marketplace.plugins.iter().find_map(|plugin| {
                        if kind == "agent" {
                            plugin
                                .agents
                                .iter()
                                .find(|agent| agent.file_location == location)
                                .map(|agent| agent.file_location.clone())
                        } else {
                            plugin
                                .skills
                                .iter()
                                .find(|skill| skill.directory_location == location)
                                .map(|skill| skill.directory_location.clone())
                        }
                    })
                });
                let found = found.ok_or_else(|| {
                    TupiError::PluginValidation(
                        "asset is no longer present in the installed marketplace inventory".into(),
                    )
                })?;
                (marketplace_root, found)
            }
            _ => {
                return Err(TupiError::PluginValidation(
                    "unsupported local asset type or inventory source".into(),
                ))
            }
        };

        Self::remove_inventory_path(&root, Path::new(&discovered_location), kind)
    }

    fn remove_inventory_path(root: &Path, target: &Path, kind: &str) -> Result<()> {
        let relative = target.strip_prefix(root).map_err(|_| {
            TupiError::PluginValidation(
                "refusing to remove an asset outside its user inventory root".into(),
            )
        })?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(TupiError::PluginValidation(
                "refusing to remove an asset with an invalid inventory path".into(),
            ));
        }

        let canonical_root = fs::canonicalize(root).map_err(|err| {
            TupiError::PluginRead(format!(
                "could not resolve inventory root {} ({err})",
                root.display()
            ))
        })?;
        if let Some(parent) = root.parent() {
            let parent_metadata = fs::symlink_metadata(parent).map_err(|err| {
                TupiError::PluginRead(format!(
                    "could not inspect inventory parent {} ({err})",
                    parent.display()
                ))
            })?;
            if parent_metadata.file_type().is_symlink() {
                return Err(TupiError::PluginValidation(format!(
                    "refusing to remove assets through symbolic link {}",
                    parent.display()
                )));
            }
        }
        let root_metadata = fs::symlink_metadata(root).map_err(|err| {
            TupiError::PluginRead(format!(
                "could not inspect inventory root {} ({err})",
                root.display()
            ))
        })?;
        if root_metadata.file_type().is_symlink() {
            return Err(TupiError::PluginValidation(format!(
                "refusing to remove assets through symbolic link {}",
                root.display()
            )));
        }
        let canonical_target = fs::canonicalize(target).map_err(|err| {
            TupiError::PluginRead(format!(
                "could not resolve asset {} ({err})",
                target.display()
            ))
        })?;
        if !canonical_target.starts_with(&canonical_root) || canonical_target == canonical_root {
            return Err(TupiError::PluginValidation(
                "refusing to remove an asset outside its user inventory root".into(),
            ));
        }

        let mut current = root.to_path_buf();
        for component in relative.components() {
            current.push(component.as_os_str());
            let metadata = fs::symlink_metadata(&current).map_err(|err| {
                TupiError::PluginRead(format!(
                    "could not inspect asset {} ({err})",
                    current.display()
                ))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(TupiError::PluginValidation(format!(
                    "refusing to remove asset through symbolic link {}",
                    current.display()
                )));
            }
        }

        let metadata = fs::symlink_metadata(target).map_err(|err| {
            TupiError::PluginRead(format!(
                "could not inspect asset {} ({err})",
                target.display()
            ))
        })?;
        match kind {
            "agent"
                if metadata.is_file()
                    && target.file_name().is_some_and(|name| {
                        name.to_string_lossy()
                            .to_ascii_lowercase()
                            .ends_with(".agent.md")
                    }) =>
            {
                fs::remove_file(target)
            }
            "skill" if metadata.is_dir() && target.join("SKILL.md").is_file() => {
                fs::remove_dir_all(target)
            }
            _ => {
                return Err(TupiError::PluginValidation(
                    "refusing to remove a path that is not a discovered agent or skill".into(),
                ))
            }
        }
        .map_err(|err| {
            TupiError::PluginRead(format!(
                "could not remove asset {} ({err})",
                target.display()
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::state::test_support::unique_temp_dir;
    use std::fs;

    #[test]
    fn removes_only_the_selected_agent_under_the_user_root() {
        let root = unique_temp_dir("remove-local-agent");
        let agents = root.join(".copilot").join("agents");
        fs::create_dir_all(&agents).unwrap();
        let target = agents.join("remove.agent.md");
        let other = agents.join("keep.agent.md");
        fs::write(&target, "# remove").unwrap();
        fs::write(&other, "# keep").unwrap();

        AppState::remove_inventory_path(&agents, &target, "agent").unwrap();

        assert!(!target.exists());
        assert!(other.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_to_remove_a_path_outside_the_user_root() {
        let root = unique_temp_dir("reject-outside-local-asset");
        let inventory = root.join(".copilot").join("agents");
        fs::create_dir_all(&inventory).unwrap();
        let outside = root.join("outside.agent.md");
        fs::write(&outside, "# outside").unwrap();

        assert!(AppState::remove_inventory_path(&inventory, &outside, "agent").is_err());
        assert!(outside.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn removes_only_the_selected_skill_directory() {
        let root = unique_temp_dir("remove-local-skill");
        let skills = root.join(".copilot").join("skills");
        let target = skills.join("remove-skill");
        let other = skills.join("keep-skill");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&other).unwrap();
        fs::write(target.join("SKILL.md"), "# remove").unwrap();
        fs::write(target.join("guide.md"), "additional skill content").unwrap();
        fs::write(other.join("SKILL.md"), "# keep").unwrap();

        AppState::remove_inventory_path(&skills, &target, "skill").unwrap();

        assert!(!target.exists());
        assert!(other.join("SKILL.md").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
