use super::{AppState, GlobalSkillRecord, GlobalSkillsState};
use crate::catalog::load_catalog_from_str;
use crate::error::{Result, TupiError};
use crate::trust::TrustStatus;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

impl AppState {
    pub fn refresh_global_skills(&self) -> Result<GlobalSkillsState> {
        let scan_root = match self.resolve_global_skills_root() {
            Ok(scan_root) => scan_root,
            Err(err) => {
                let default_scan_root = self.default_global_skills_root();
                self.persist_global_skill_scan_error(&default_scan_root, &err.to_string())?;
                return Err(err);
            }
        };

        match self.collect_global_skills(&scan_root) {
            Ok((skills, error_message)) => {
                let refreshed_at = Utc::now().to_rfc3339();
                self.persist_global_skills(
                    &skills,
                    &scan_root,
                    &refreshed_at,
                    error_message.as_deref(),
                )?;
                Ok(GlobalSkillsState {
                    skills,
                    scan_root: scan_root.display().to_string(),
                    refreshed_at: Some(refreshed_at),
                    error_message,
                })
            }
            Err(err) => {
                self.persist_global_skill_scan_error(&scan_root, &err.to_string())?;
                Err(err)
            }
        }
    }

    pub fn load_global_skills_state(&self) -> Result<GlobalSkillsState> {
        let connection = self.open()?;
        let mut stmt = connection.prepare(
            r#"
            SELECT skill_name, directory_location, trust_status
            FROM global_skills
            ORDER BY lower(skill_name) ASC, lower(directory_location) ASC
            "#,
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(GlobalSkillRecord {
                name: row.get(0)?,
                directory_location: row.get(1)?,
                trust_status: Self::trust_status_from_str(&row.get::<_, String>(2)?),
            })
        })?;
        let skills = rows.collect::<std::result::Result<Vec<_>, _>>()?;

        let scan_state = connection
            .query_row(
                r#"
                SELECT scan_root, refreshed_at, error_message
                FROM global_skill_scan_state
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
        let default_scan_root = self.default_global_skills_root();
        let (scan_root, refreshed_at, error_message) =
            scan_state.unwrap_or((default_scan_root.display().to_string(), None, None));

        Ok(GlobalSkillsState {
            skills,
            scan_root,
            refreshed_at,
            error_message,
        })
    }

    fn collect_global_skills(
        &self,
        skills_root: &Path,
    ) -> Result<(Vec<GlobalSkillRecord>, Option<String>)> {
        if !skills_root.exists() {
            return Ok((Vec::new(), None));
        }
        if !skills_root.is_dir() {
            return Err(TupiError::PluginRead(format!(
                "{} is not a directory",
                skills_root.display()
            )));
        }

        let (trusted_skill_digests, verification_error) = match self.load_trusted_skill_digests() {
            Ok(digests) => (digests, None),
            Err(err) => (
                HashSet::new(),
                Some(format!(
                    "could not verify global skill trust; listed skills are marked untrusted: {err}"
                )),
            ),
        };
        let entries = fs::read_dir(skills_root)
            .map_err(|err| TupiError::PluginRead(format!("{} ({err})", skills_root.display())))?;
        let mut skills = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|err| TupiError::PluginRead(err.to_string()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|err| TupiError::PluginRead(err.to_string()))?;
            if !file_type.is_dir() || file_type.is_symlink() || !path.join("SKILL.md").is_file() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    TupiError::PluginValidation(format!(
                        "skill directory {} must have a valid name",
                        path.display()
                    ))
                })?
                .to_string();
            let digest = Self::content_digest(&path)?;
            skills.push(GlobalSkillRecord {
                name,
                directory_location: path.display().to_string(),
                trust_status: if trusted_skill_digests.contains(&digest) {
                    TrustStatus::Trusted
                } else {
                    TrustStatus::Untrusted
                },
            });
        }

        skills.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| {
                    left.directory_location
                        .to_ascii_lowercase()
                        .cmp(&right.directory_location.to_ascii_lowercase())
                })
        });
        Ok((skills, verification_error))
    }

    fn load_trusted_skill_digests(&self) -> Result<HashSet<String>> {
        let contents = self.read_active_catalog_contents()?;
        let catalog = load_catalog_from_str(&contents)?;
        let mut digests = HashSet::new();

        for skill in &catalog.skills {
            let workspace = self
                .cache_dir
                .join("marketplaces")
                .join(Self::sanitize_cache_key(&skill.marketplace));
            let Some(path) = Self::resolve_catalog_skill_path(&workspace, &skill.path) else {
                continue;
            };
            digests.insert(Self::content_digest(&path)?);
        }
        Ok(digests)
    }

    fn resolve_catalog_skill_path(workspace: &Path, catalog_path: &str) -> Option<PathBuf> {
        let direct_path = workspace.join(catalog_path);
        if direct_path.is_dir() && direct_path.join("SKILL.md").is_file() {
            return Some(direct_path);
        }
        if direct_path.is_file()
            && direct_path
                .file_name()
                .is_some_and(|name| name == "SKILL.md")
        {
            return direct_path.parent().map(Path::to_path_buf);
        }
        None
    }

    fn persist_global_skills(
        &self,
        skills: &[GlobalSkillRecord],
        scan_root: &Path,
        refreshed_at: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        let mut connection = self.open()?;
        let tx = connection.transaction()?;
        tx.execute("DELETE FROM global_skills", [])?;
        for skill in skills {
            tx.execute(
                r#"
                INSERT INTO global_skills (directory_location, skill_name, trust_status)
                VALUES (?1, ?2, ?3)
                "#,
                params![
                    &skill.directory_location,
                    &skill.name,
                    Self::trust_status_label(&skill.trust_status)
                ],
            )?;
        }
        tx.execute(
            r#"
            INSERT INTO global_skill_scan_state (id, scan_root, refreshed_at, error_message)
            VALUES (1, ?1, ?2, ?3)
            ON CONFLICT(id) DO UPDATE SET
                scan_root = excluded.scan_root,
                refreshed_at = excluded.refreshed_at,
                error_message = excluded.error_message
            "#,
            params![scan_root.display().to_string(), refreshed_at, error_message],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn persist_global_skill_scan_error(&self, scan_root: &Path, error_message: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO global_skill_scan_state (id, scan_root, refreshed_at, error_message)
            VALUES (
                1,
                ?1,
                (SELECT refreshed_at FROM global_skill_scan_state WHERE id = 1),
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
    use crate::state::test_support::{make_test_state_with_catalog, unique_temp_dir};
    use crate::trust::TrustStatus;
    use std::fs;

    #[test]
    fn discovers_global_skills_and_matches_trusted_catalog_content() {
        let root = unique_temp_dir("global-skills");
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
skills:
  - id: rust
    marketplace: official
    path: skills/rust
    version: 1.0.0
    revision: abc123
"#,
        );
        let trusted_skill = root
            .join(".tupi")
            .join("catalog-cache")
            .join("marketplaces")
            .join("official")
            .join("skills")
            .join("rust");
        fs::create_dir_all(&trusted_skill).unwrap();
        fs::write(trusted_skill.join("SKILL.md"), "# Rust\nTrusted skill.\n").unwrap();

        let global_skills = root.join("user-home").join(".copilot").join("skills");
        let matching_skill = global_skills.join("rust");
        let other_skill = global_skills.join("local");
        fs::create_dir_all(&matching_skill).unwrap();
        fs::create_dir_all(&other_skill).unwrap();
        fs::write(matching_skill.join("SKILL.md"), "# Rust\nTrusted skill.\n").unwrap();
        fs::write(other_skill.join("SKILL.md"), "# Local\n").unwrap();

        let (found, warning) = state.collect_global_skills(&global_skills).unwrap();

        assert!(warning.is_none());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "local");
        assert_eq!(found[0].trust_status, TrustStatus::Untrusted);
        assert_eq!(found[1].name, "rust");
        assert_eq!(found[1].trust_status, TrustStatus::Trusted);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn keeps_global_skills_visible_as_untrusted_when_catalog_cannot_be_verified() {
        let root = unique_temp_dir("global-skills-invalid-catalog");
        let state = make_test_state_with_catalog(&root, "not: valid: catalog");
        let global_skills = root.join("user-home").join(".copilot").join("skills");
        let skill = global_skills.join("local");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "# Local").unwrap();

        let (found, warning) = state.collect_global_skills(&global_skills).unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].trust_status, TrustStatus::Untrusted);
        assert!(warning.is_some());
        fs::remove_dir_all(root).unwrap();
    }
}
