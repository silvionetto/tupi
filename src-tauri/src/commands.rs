use crate::profile::{Profile, ProjectProfileDefaults};
use crate::state::{AppState, CatalogState, MarketplaceOption, RefreshRecord};
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
pub fn refresh_catalog(state: State<'_, AppState>) -> std::result::Result<RefreshRecord, String> {
    state.refresh_catalog().map_err(|err| err.to_string())
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
    state.delete_profile(&profile_id).map_err(|err| err.to_string())
}
