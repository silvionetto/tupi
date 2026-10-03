use crate::profile::{Profile, ProjectProfileDefaults};
use crate::state::{
    AppState, CatalogState, GlobalAgentsState, GlobalSkillsState, InstalledMarketplacesState,
    MarketplaceOption, ProfileAsset, RefreshRecord,
};
use tauri::State;

#[tauri::command]
pub fn get_catalog_summary(
    state: State<'_, AppState>,
) -> std::result::Result<crate::catalog::CatalogSummary, String> {
    state.load_catalog_summary().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn get_catalog_state(state: State<'_, AppState>) -> std::result::Result<CatalogState, String> {
    state.load_catalog_state().map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn refresh_startup_data(state: State<'_, AppState>) -> std::result::Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut failures = Vec::new();

        if let Err(err) = state.refresh_installed_marketplaces() {
            failures.push(err.to_string());
        }
        if let Err(err) = state.refresh_global_agents() {
            failures.push(err.to_string());
        }
        if let Err(err) = state.refresh_global_skills() {
            failures.push(err.to_string());
        }
        if let Err(err) = state.refresh_marketplace_agents() {
            failures.push(err.to_string());
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    })
    .await
    .map_err(|err| format!("startup refresh task failed: {err}"))?
}

#[tauri::command]
pub async fn refresh_global_inventory(
    state: State<'_, AppState>,
) -> std::result::Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut failures = Vec::new();
        if let Err(err) = state.refresh_global_agents() {
            failures.push(err.to_string());
        }
        if let Err(err) = state.refresh_global_skills() {
            failures.push(err.to_string());
        }
        if let Err(err) = state.refresh_installed_marketplaces() {
            failures.push(err.to_string());
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    })
    .await
    .map_err(|err| format!("global inventory refresh task failed: {err}"))?
}

#[tauri::command]
pub async fn refresh_catalog(
    state: State<'_, AppState>,
) -> std::result::Result<RefreshRecord, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state.refresh_catalog().map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("catalog refresh task failed: {err}"))?
}

#[tauri::command]
pub fn list_marketplaces(
    state: State<'_, AppState>,
) -> std::result::Result<Vec<MarketplaceOption>, String> {
    state.list_marketplaces().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> std::result::Result<Vec<Profile>, String> {
    state.read_profiles().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn list_profile_assets(
    state: State<'_, AppState>,
    profile_id: String,
) -> std::result::Result<Vec<ProfileAsset>, String> {
    state
        .list_profile_assets(&profile_id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn install_profile_asset(
    state: State<'_, AppState>,
    profile_id: String,
    asset_id: String,
) -> std::result::Result<(), String> {
    state
        .install_profile_asset(&profile_id, &asset_id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn uninstall_profile_asset(
    state: State<'_, AppState>,
    profile_id: String,
    asset_id: String,
) -> std::result::Result<(), String> {
    state
        .uninstall_profile_asset(&profile_id, &asset_id)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn get_global_agents_state(
    state: State<'_, AppState>,
) -> std::result::Result<GlobalAgentsState, String> {
    state
        .load_global_agents_state()
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn get_global_skills_state(
    state: State<'_, AppState>,
) -> std::result::Result<GlobalSkillsState, String> {
    state
        .load_global_skills_state()
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn remove_local_asset(
    state: State<'_, AppState>,
    kind: String,
    source: String,
    location: String,
) -> std::result::Result<(), String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        state
            .remove_local_asset(&kind, &source, &location)
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("asset removal task failed: {err}"))?
}

#[tauri::command]
pub fn get_installed_marketplaces_state(
    state: State<'_, AppState>,
) -> std::result::Result<InstalledMarketplacesState, String> {
    state
        .load_installed_marketplaces_state()
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn get_project_profile_defaults(state: State<'_, AppState>) -> ProjectProfileDefaults {
    state.project_profile_defaults()
}

#[tauri::command]
pub fn upsert_profile(
    state: State<'_, AppState>,
    profile: Profile,
) -> std::result::Result<Profile, String> {
    state.upsert_profile(profile).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn delete_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> std::result::Result<(), String> {
    state
        .delete_profile(&profile_id)
        .map_err(|err| err.to_string())
}
