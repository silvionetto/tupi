mod catalog_store;
mod global_agents;
mod installed_marketplaces;
mod marketplace_agents;
mod profiles_store;
mod repo_sync;
#[cfg(test)]
mod test_support;

use crate::catalog::CatalogSummary;
use crate::error::Result;
use crate::trust::TrustStatus;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
    #[serde(default)]
    pub agents: Vec<MarketplaceAgentRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceAgentRecord {
    pub name: String,
    pub description: Option<String>,
    pub trust_status: TrustStatus,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMarketplaceRecord {
    pub id: String,
    pub name: String,
    pub directory_location: String,
    pub repository: Option<String>,
    pub trust_status: TrustStatus,
    #[serde(default)]
    pub plugins: Vec<InstalledPluginRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPluginRecord {
    pub name: String,
    pub directory_location: String,
    #[serde(default)]
    pub skills: Vec<InstalledSkillRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkillRecord {
    pub name: String,
    pub directory_location: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledMarketplacesState {
    pub marketplaces: Vec<InstalledMarketplaceRecord>,
    pub scan_root: String,
    pub refreshed_at: Option<String>,
    pub error_message: Option<String>,
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
            if candidate
                .join("catalog")
                .join("trusted-assets.yaml")
                .is_file()
            {
                return Some(candidate.to_path_buf());
            }
        }

        None
    }

    pub fn open(&self) -> Result<rusqlite::Connection> {
        Ok(rusqlite::Connection::open(&self.db_path)?)
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
        Self::resolve_copilot_child_dir("agents")
    }

    fn default_global_agents_root(&self) -> PathBuf {
        Self::default_copilot_child_dir("agents")
    }

    fn resolve_installed_plugins_root(&self) -> Result<PathBuf> {
        Self::resolve_copilot_child_dir("installed-plugins")
    }

    fn default_installed_plugins_root(&self) -> PathBuf {
        Self::default_copilot_child_dir("installed-plugins")
    }

    fn resolve_copilot_child_dir(child: &str) -> Result<PathBuf> {
        Ok(Self::resolve_user_home_dir()?.join(".copilot").join(child))
    }

    fn default_copilot_child_dir(child: &str) -> PathBuf {
        Self::resolve_user_home_dir()
            .map(|home| home.join(".copilot").join(child))
            .unwrap_or_else(|_| PathBuf::from(".copilot").join(child))
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

    fn sanitize_cache_key(value: &str) -> String {
        value
            .chars()
            .map(|ch| match ch {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
                _ => '-',
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::AppState;
    use std::fs;

    #[test]
    fn finds_workspace_root_from_src_tauri_directory() {
        let root = crate::state::test_support::unique_temp_dir("workspace-root");
        fs::create_dir_all(root.join("catalog")).unwrap();
        fs::create_dir_all(root.join("src-tauri").join("src")).unwrap();
        fs::write(
            root.join("catalog").join("trusted-assets.yaml"),
            "version: 1",
        )
        .unwrap();

        let resolved = AppState::find_workspace_root_from(&root.join("src-tauri").join("src"));

        assert_eq!(resolved.as_deref(), Some(root.as_path()));

        fs::remove_dir_all(root).unwrap();
    }
}
