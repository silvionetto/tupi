use super::AppState;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn make_test_state(root: &Path) -> AppState {
    make_test_state_with_catalog(
        root,
        "version: 1\ncatalogRevision: test\nmarketplaces: []\nagents: []\n",
    )
}

pub(crate) fn make_test_state_with_catalog(root: &Path, catalog_contents: &str) -> AppState {
    let state_dir = root.join(".tupi");
    let cache_dir = state_dir.join("catalog-cache");
    let catalog_dir = root.join("catalog");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::create_dir_all(&catalog_dir).unwrap();
    fs::write(catalog_dir.join("trusted-assets.yaml"), catalog_contents).unwrap();

    let state = AppState {
        db_path: state_dir.join("state.sqlite"),
        catalog_path: catalog_dir.join("trusted-assets.yaml"),
        cache_dir,
    };
    state.initialize().unwrap();
    state
}

pub(crate) fn unique_temp_dir(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tupi-{name}-{suffix}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}
