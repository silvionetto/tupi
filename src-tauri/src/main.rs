#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod catalog;
mod commands;
mod error;
mod profile;
mod state;
mod trust;

use state::AppState;

fn main() {
  let state = AppState::new().expect("failed to initialize application state");
  let _ = state.refresh_global_agents();

  tauri::Builder::default()
    .manage(state)
    .invoke_handler(tauri::generate_handler![
      commands::get_catalog_summary,
      commands::get_catalog_state,
      commands::refresh_catalog,
      commands::list_marketplaces,
      commands::list_profiles,
      commands::get_global_agents_state,
      commands::get_project_profile_defaults,
      commands::upsert_profile,
      commands::delete_profile
    ])
    .run(tauri::generate_context!())
    .expect("failed to run Tauri application");
}
