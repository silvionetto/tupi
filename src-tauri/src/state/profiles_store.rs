use super::AppState;
use crate::error::Result;
use crate::profile::{Profile, ProjectProfileDefaults};
use rusqlite::{params, OptionalExtension};

impl AppState {
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
            let candidate =
                connection.query_row("SELECT lower(hex(randomblob(8)))", [], |row| {
                    row.get::<_, String>(0)
                })?;
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
}

#[cfg(test)]
mod tests {
    use crate::state::test_support::make_test_state;
    use std::fs;

    #[test]
    fn upsert_profile_generates_database_backed_id_and_optional_description() {
        let root = crate::state::test_support::unique_temp_dir("profile-upsert");
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
        let root = crate::state::test_support::unique_temp_dir("legacy-profile");
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
        let root = crate::state::test_support::unique_temp_dir("missing-project-location");
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
}
