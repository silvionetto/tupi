use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub project_location: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(rename = "catalogRevision", default)]
    pub catalog_revision: Option<String>,
    #[serde(default)]
    pub selected_assets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectProfileDefaults {
    #[serde(rename = "displayName")]
    pub display_name: String,
}
