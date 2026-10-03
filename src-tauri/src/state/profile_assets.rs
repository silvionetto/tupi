use super::AppState;
use crate::catalog::{load_catalog_from_str, Marketplace};
use crate::error::{Result, TupiError};
use crate::profile::Profile;
use crate::trust::TrustStatus;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize)]
pub struct ProfileAsset {
    pub asset_id: String,
    pub kind: String,
    pub marketplace_id: String,
    pub marketplace_name: String,
    pub name: String,
    pub description: Option<String>,
    pub plugin_name: Option<String>,
    pub destination: String,
    pub installation_state: String,
    pub source_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct AssetIdentity {
    kind: String,
    marketplace_id: String,
    plugin_name: Option<String>,
    name: String,
}

#[derive(Debug, Clone)]
struct AssetSource {
    identity: AssetIdentity,
    marketplace_name: String,
    description: Option<String>,
    source: PathBuf,
    destination: PathBuf,
}

#[derive(Debug, Clone)]
struct InstalledAsset {
    asset_id: String,
    destination: String,
    digest: String,
}

impl AppState {
    pub fn list_profile_assets(&self, profile_id: &str) -> Result<Vec<ProfileAsset>> {
        let profile = self.load_profile(profile_id)?;
        let project_root = Self::canonical_project_root(&profile)?;
        let sources = self.trusted_asset_sources()?;
        let mut known_asset_ids = std::collections::HashSet::new();
        let mut assets = Vec::new();

        for source in sources {
            let asset_id = Self::asset_id(&source.identity)?;
            known_asset_ids.insert(asset_id.clone());
            let installed = self.read_installed_asset(&project_root, &asset_id)?;
            assets.push(ProfileAsset {
                asset_id: asset_id.clone(),
                kind: source.identity.kind,
                marketplace_id: source.identity.marketplace_id,
                marketplace_name: source.marketplace_name,
                name: source.identity.name,
                description: source.description,
                plugin_name: source.identity.plugin_name,
                destination: project_root.join(&source.destination).display().to_string(),
                installation_state: Self::asset_installation_state(
                    &project_root,
                    &source.destination,
                    &asset_id,
                    installed.as_ref(),
                )?,
                source_available: true,
            });
        }

        for installed in self.read_installed_assets(&project_root)? {
            if known_asset_ids.contains(&installed.asset_id) {
                continue;
            }
            let identity = Self::parse_asset_id(&installed.asset_id)?;
            let destination = Self::asset_destination(&identity)?;
            assets.push(ProfileAsset {
                asset_id: installed.asset_id.clone(),
                kind: identity.kind,
                marketplace_id: identity.marketplace_id.clone(),
                marketplace_name: format!("{} (no longer available)", identity.marketplace_id),
                name: identity.name,
                description: None,
                plugin_name: identity.plugin_name,
                destination: project_root.join(&destination).display().to_string(),
                installation_state: Self::asset_installation_state(
                    &project_root,
                    &destination,
                    &installed.asset_id,
                    Some(&installed),
                )?,
                source_available: false,
            });
        }
        Ok(assets)
    }

    pub fn install_profile_asset(&self, profile_id: &str, asset_id: &str) -> Result<()> {
        let profile = self.load_profile(profile_id)?;
        let project_root = Self::canonical_project_root(&profile)?;
        let source = self.find_trusted_asset(asset_id)?;
        let installed = self.read_installed_asset(&project_root, asset_id)?;

        let current_state = Self::asset_installation_state(
            &project_root,
            &source.destination,
            asset_id,
            installed.as_ref(),
        )?;
        if current_state != "Available" {
            return Err(TupiError::ProfileValidation(format!(
                "cannot install {}: destination state is {current_state}",
                source.identity.name
            )));
        }

        let destination = project_root.join(&source.destination);
        let parent = destination.parent().ok_or_else(|| {
            TupiError::ProfileValidation("asset destination has no parent directory".into())
        })?;
        Self::create_safe_directory(&project_root, parent)?;

        let stage = parent.join(format!(
            ".tupi-install-{}-{}",
            std::process::id(),
            TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let copy_result = if source.source.is_dir() {
            Self::copy_directory_without_symlinks(&source.source, &stage)
        } else {
            Self::copy_file_without_overwrite(&source.source, &stage)
        };
        if let Err(err) = copy_result {
            let _ = Self::remove_path(&stage);
            return Err(err);
        }

        let staged_digest = match Self::content_digest(&stage) {
            Ok(digest) => digest,
            Err(err) => {
                let _ = Self::remove_path(&stage);
                return Err(err);
            }
        };
        let install_result = if source.source.is_dir() {
            match fs::create_dir(&destination) {
                Ok(()) => Self::copy_staged_directory_without_overwrite(&stage, &destination),
                Err(err) => Err(Self::asset_io_error(
                    "create skill destination",
                    &destination,
                    err,
                )),
            }
        } else {
            Self::copy_file_without_overwrite(&stage, &destination)
        };
        if let Err(err) = install_result {
            let _ = Self::remove_path(&stage);
            return Err(TupiError::ProfileValidation(format!(
                "asset installation was incomplete; any partial destination at {} was preserved ({err})",
                destination.display()
            )));
        }
        let installed_digest = Self::content_digest(&destination)?;
        if installed_digest != staged_digest {
            let _ = Self::remove_path(&stage);
            return Err(TupiError::ProfileValidation(format!(
                "installed asset {} changed while it was being copied; the destination was preserved",
                source.identity.name
            )));
        }
        if let Err(err) = Self::remove_path(&stage) {
            return Err(TupiError::ProfileValidation(format!(
                "asset was copied to {} but its temporary source could not be removed ({err})",
                destination.display()
            )));
        }

        let installed = InstalledAsset {
            asset_id: asset_id.to_string(),
            destination: Self::relative_path_string(&source.destination)?,
            digest: staged_digest.clone(),
        };
        if let Err(err) = self.store_installed_asset(&project_root, &installed) {
            if let Ok(current_digest) = Self::content_digest(&destination) {
                if current_digest == staged_digest {
                    let _ = Self::remove_path(&destination);
                }
            }
            return Err(err);
        }
        Ok(())
    }

    pub fn uninstall_profile_asset(&self, profile_id: &str, asset_id: &str) -> Result<()> {
        let profile = self.load_profile(profile_id)?;
        let project_root = Self::canonical_project_root(&profile)?;
        let installed = self.read_installed_asset(&project_root, asset_id)?;
        let Some(installed) = installed else {
            return Err(TupiError::ProfileValidation(
                "asset is not recorded as installed for this project".into(),
            ));
        };
        let identity = Self::parse_asset_id(asset_id)?;
        let destination = Self::asset_destination(&identity)?;
        let state = Self::asset_installation_state(
            &project_root,
            &destination,
            asset_id,
            Some(&installed),
        )?;
        if state != "Installed" {
            return Err(TupiError::ProfileValidation(format!(
                "cannot uninstall {}: destination state is {state}; modified or unmanaged content was preserved",
                identity.name
            )));
        }

        Self::remove_path(&project_root.join(&destination))?;
        Self::remove_path(&destination)?;
        self.remove_installed_asset(&project_root, asset_id)
    }

    fn load_profile(&self, profile_id: &str) -> Result<Profile> {
        self.read_profiles()?
            .into_iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| TupiError::ProfileValidation(format!("unknown profile {profile_id}")))
    }

    fn read_installed_asset(
        &self,
        project_root: &Path,
        asset_id: &str,
    ) -> Result<Option<InstalledAsset>> {
        let connection = self.open()?;
        Ok(connection
            .query_row(
                r#"
                SELECT destination, digest
                FROM profile_asset_installs
                WHERE project_location = ?1 AND asset_id = ?2
                "#,
                params![project_root.to_string_lossy().as_ref(), asset_id],
                |row| {
                    Ok(InstalledAsset {
                        asset_id: asset_id.to_string(),
                        destination: row.get(0)?,
                        digest: row.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    fn read_installed_assets(&self, project_root: &Path) -> Result<Vec<InstalledAsset>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            r#"
            SELECT asset_id, destination, digest
            FROM profile_asset_installs
            WHERE project_location = ?1
            ORDER BY asset_id
            "#,
        )?;
        let rows =
            statement.query_map(params![project_root.to_string_lossy().as_ref()], |row| {
                Ok(InstalledAsset {
                    asset_id: row.get(0)?,
                    destination: row.get(1)?,
                    digest: row.get(2)?,
                })
            })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(TupiError::from)
    }

    fn store_installed_asset(&self, project_root: &Path, asset: &InstalledAsset) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            r#"
            INSERT INTO profile_asset_installs (project_location, asset_id, destination, digest)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(project_location, asset_id) DO UPDATE SET
                destination = excluded.destination,
                digest = excluded.digest
            "#,
            params![
                project_root.to_string_lossy().as_ref(),
                &asset.asset_id,
                &asset.destination,
                &asset.digest
            ],
        )?;
        Ok(())
    }

    fn remove_installed_asset(&self, project_root: &Path, asset_id: &str) -> Result<()> {
        let connection = self.open()?;
        connection.execute(
            "DELETE FROM profile_asset_installs WHERE project_location = ?1 AND asset_id = ?2",
            params![project_root.to_string_lossy().as_ref(), asset_id],
        )?;
        Ok(())
    }

    fn canonical_project_root(profile: &Profile) -> Result<PathBuf> {
        let location = profile
            .project_location
            .as_ref()
            .ok_or_else(|| TupiError::ProfileValidation("project location is required".into()))?;
        let root = fs::canonicalize(location).map_err(|err| {
            TupiError::ProfileValidation(format!(
                "project directory {} is unavailable ({err})",
                location
            ))
        })?;
        if !root.is_dir() {
            return Err(TupiError::ProfileValidation(format!(
                "project location {} is not a directory",
                location
            )));
        }
        Ok(root)
    }

    fn trusted_asset_sources(&self) -> Result<Vec<AssetSource>> {
        let state = self.load_catalog_state()?;
        let configured_repository = self.source_repository();
        if state.trust_status != TrustStatus::Trusted
            || state.stale
            || state.source_branch != "main"
            || state.source_repository != configured_repository
            || configured_repository.is_none()
        {
            return Err(TupiError::CatalogValidation(
                "refresh the trusted catalog from its configured main branch before installing project assets".into(),
            ));
        }
        let catalog = load_catalog_from_str(&self.read_active_catalog_contents()?)?;
        let mut assets = Vec::new();
        for marketplace in &catalog.marketplaces {
            let workspace = self.resolve_cached_marketplace_workspace(marketplace)?;
            let agents_root = workspace.join("agents");
            if Self::is_plain_directory(&agents_root)? {
                Self::collect_agent_assets(&agents_root, marketplace, &mut assets)?;
            }

            let skills_root = workspace.join("skills");
            if Self::is_plain_directory(&skills_root)? {
                Self::collect_skill_assets(&skills_root, marketplace, None, &mut assets)?;
            }

            let plugins_root = workspace.join("plugins");
            if Self::is_plain_directory(&plugins_root)? {
                for entry in fs::read_dir(&plugins_root)
                    .map_err(|err| Self::asset_io_error("read plugins", &plugins_root, err))?
                {
                    let entry = entry.map_err(|err| TupiError::CatalogRead(err.to_string()))?;
                    let file_type = entry
                        .file_type()
                        .map_err(|err| TupiError::CatalogRead(err.to_string()))?;
                    if file_type.is_symlink() || !file_type.is_dir() {
                        continue;
                    }
                    let plugin_name = entry.file_name().into_string().map_err(|_| {
                        TupiError::CatalogValidation(
                            "plugin directory name is not valid UTF-8".into(),
                        )
                    })?;
                    let skills_root = entry.path().join("skills");
                    if Self::is_plain_directory(&skills_root)? {
                        Self::collect_skill_assets(
                            &skills_root,
                            marketplace,
                            Some(&plugin_name),
                            &mut assets,
                        )?;
                    }
                }
            }
        }
        assets.sort_by(|left, right| {
            left.identity
                .kind
                .cmp(&right.identity.kind)
                .then_with(|| left.marketplace_name.cmp(&right.marketplace_name))
                .then_with(|| left.identity.plugin_name.cmp(&right.identity.plugin_name))
                .then_with(|| left.identity.name.cmp(&right.identity.name))
        });
        Ok(assets)
    }

    fn find_trusted_asset(&self, asset_id: &str) -> Result<AssetSource> {
        for asset in self.trusted_asset_sources()? {
            if Self::asset_id(&asset.identity)? == asset_id {
                return Ok(asset);
            }
        }
        Err(TupiError::CatalogValidation(
            "asset is not available from the active trusted catalog".into(),
        ))
    }

    fn resolve_cached_marketplace_workspace(&self, marketplace: &Marketplace) -> Result<PathBuf> {
        let workspace = self
            .cache_dir
            .join("marketplaces")
            .join(Self::sanitize_cache_key(&marketplace.id));
        let workspace_metadata = fs::symlink_metadata(&workspace).map_err(|err| {
            TupiError::CatalogRead(format!(
                "trusted marketplace {} is not cached ({err}); refresh startup data first",
                marketplace.id
            ))
        })?;
        let canonical_cache = fs::canonicalize(&self.cache_dir)
            .map_err(|err| Self::asset_io_error("resolve catalog cache", &self.cache_dir, err))?;
        let canonical_workspace = fs::canonicalize(&workspace)
            .map_err(|err| Self::asset_io_error("resolve marketplace cache", &workspace, err))?;
        if workspace_metadata.file_type().is_symlink()
            || !workspace_metadata.is_dir()
            || !canonical_workspace.starts_with(&canonical_cache)
        {
            return Err(TupiError::CatalogValidation(format!(
                "cached marketplace {} is not a safe workspace",
                marketplace.id
            )));
        }
        let git_directory = workspace.join(".git");
        let git_metadata = fs::symlink_metadata(&git_directory).map_err(|err| {
            TupiError::CatalogRead(format!(
                "trusted marketplace {} has no Git metadata ({err})",
                marketplace.id
            ))
        })?;
        if git_metadata.file_type().is_symlink() || !git_metadata.is_dir() {
            return Err(TupiError::CatalogRead(format!(
                "trusted marketplace {} is not cached; refresh startup data first",
                marketplace.id
            )));
        }

        let head = Self::git_output(&workspace, &["rev-parse", "HEAD"])?;
        let origin = Self::git_output(&workspace, &["config", "--get", "remote.origin.url"])?;
        if head != marketplace.revision || origin != marketplace.repository {
            return Err(TupiError::CatalogValidation(format!(
                "cached marketplace {} does not match its trusted catalog revision",
                marketplace.id
            )));
        }
        let status = Self::git_output(
            &workspace,
            &[
                "status",
                "--porcelain",
                "--untracked-files=all",
                "--ignored=matching",
            ],
        )?;
        if !status.is_empty() {
            return Err(TupiError::CatalogValidation(format!(
                "cached marketplace {} contains local modifications",
                marketplace.id
            )));
        }
        Ok(workspace)
    }

    fn git_output(directory: &Path, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .map_err(|err| TupiError::CatalogRead(format!("failed to run git: {err}")))?;
        if !output.status.success() {
            return Err(TupiError::CatalogRead(format!(
                "git {} failed in {}",
                args.join(" "),
                directory.display()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn collect_agent_assets(
        directory: &Path,
        marketplace: &Marketplace,
        assets: &mut Vec<AssetSource>,
    ) -> Result<()> {
        for entry in fs::read_dir(directory)
            .map_err(|err| Self::asset_io_error("read agents", directory, err))?
        {
            let entry = entry.map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                Self::collect_agent_assets(&entry.path(), marketplace, assets)?;
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            let filename = entry.file_name().into_string().map_err(|_| {
                TupiError::CatalogValidation("agent filename is not valid UTF-8".into())
            })?;
            let Some(name) = filename.strip_suffix(".agent.md") else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }
            let identity = AssetIdentity {
                kind: "agent".into(),
                marketplace_id: marketplace.id.clone(),
                plugin_name: None,
                name: name.to_string(),
            };
            assets.push(AssetSource {
                identity,
                marketplace_name: marketplace.name.clone(),
                description: None,
                source: entry.path(),
                destination: PathBuf::from(".github").join("agents").join(filename),
            });
        }
        Ok(())
    }

    fn collect_skill_assets(
        skills_root: &Path,
        marketplace: &Marketplace,
        plugin_name: Option<&str>,
        assets: &mut Vec<AssetSource>,
    ) -> Result<()> {
        for entry in fs::read_dir(skills_root)
            .map_err(|err| Self::asset_io_error("read skills", skills_root, err))?
        {
            let entry = entry.map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            if !file_type.is_dir() {
                continue;
            }
            let skill_root = entry.path();
            let skill_file = skill_root.join("SKILL.md");
            match fs::symlink_metadata(&skill_file) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
                Ok(_) => continue,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(Self::asset_io_error(
                        "inspect trusted skill manifest",
                        &skill_file,
                        err,
                    ))
                }
            }
            let name = entry.file_name().into_string().map_err(|_| {
                TupiError::CatalogValidation("skill directory name is not valid UTF-8".into())
            })?;
            let identity = AssetIdentity {
                kind: "skill".into(),
                marketplace_id: marketplace.id.clone(),
                plugin_name: plugin_name.map(str::to_string),
                name: name.clone(),
            };
            assets.push(AssetSource {
                identity,
                marketplace_name: marketplace.name.clone(),
                description: None,
                source: skill_root,
                destination: PathBuf::from(".github").join("skills").join(name),
            });
        }
        Ok(())
    }

    fn asset_id(identity: &AssetIdentity) -> Result<String> {
        Ok(serde_json::to_string(identity)?)
    }

    fn parse_asset_id(asset_id: &str) -> Result<AssetIdentity> {
        let identity: AssetIdentity = serde_json::from_str(asset_id).map_err(|err| {
            TupiError::ProfileValidation(format!("invalid installed asset identity ({err})"))
        })?;
        if Self::asset_id(&identity)? != asset_id {
            return Err(TupiError::ProfileValidation(
                "installed asset identity is not in canonical form".into(),
            ));
        }
        Ok(identity)
    }

    fn asset_destination(identity: &AssetIdentity) -> Result<PathBuf> {
        if identity.name.is_empty()
            || identity.name == "."
            || identity.name == ".."
            || identity.name.contains('/')
            || identity.name.contains('\\')
        {
            return Err(TupiError::ProfileValidation(
                "installed asset name is not a safe path component".into(),
            ));
        }
        match identity.kind.as_str() {
            "agent" => Ok(PathBuf::from(".github")
                .join("agents")
                .join(format!("{}.agent.md", identity.name))),
            "skill" => Ok(PathBuf::from(".github").join("skills").join(&identity.name)),
            _ => Err(TupiError::ProfileValidation(
                "installed asset has an unsupported kind".into(),
            )),
        }
    }

    fn asset_installation_state(
        project_root: &Path,
        destination: &Path,
        asset_id: &str,
        installed: Option<&InstalledAsset>,
    ) -> Result<String> {
        if Self::has_unsafe_destination_parent(project_root, destination)? {
            return Ok("Conflict".into());
        }
        let full_destination = project_root.join(destination);
        let relative_destination = Self::relative_path_string(destination)?;
        let entry = installed.filter(|entry| {
            entry.asset_id == asset_id && entry.destination == relative_destination
        });
        match fs::symlink_metadata(&full_destination) {
            Ok(metadata) if metadata.file_type().is_symlink() => Ok("Conflict".into()),
            Ok(_) => match entry {
                Some(entry) => {
                    if Self::content_digest(&full_destination)? == entry.digest {
                        Ok("Installed".into())
                    } else {
                        Ok("Modified".into())
                    }
                }
                None => Ok("Conflict".into()),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok("Available".into()),
            Err(err) => Err(Self::asset_io_error(
                "inspect project asset",
                &full_destination,
                err,
            )),
        }
    }

    fn create_safe_directory(project_root: &Path, directory: &Path) -> Result<()> {
        let relative = directory.strip_prefix(project_root).map_err(|_| {
            TupiError::ProfileValidation("asset path escapes the project directory".into())
        })?;
        let mut current = project_root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(TupiError::ProfileValidation(
                    "asset path contains an invalid directory component".into(),
                ));
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Err(TupiError::ProfileValidation(format!(
                        "asset directory {} is not a safe directory",
                        current.display()
                    )));
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)
                    .map_err(|err| Self::asset_io_error("create asset directory", &current, err))?,
                Err(err) => {
                    return Err(Self::asset_io_error(
                        "inspect asset directory",
                        &current,
                        err,
                    ))
                }
            }
        }
        Ok(())
    }

    fn has_unsafe_destination_parent(project_root: &Path, destination: &Path) -> Result<bool> {
        let relative = destination.parent().ok_or_else(|| {
            TupiError::ProfileValidation("asset destination has no parent directory".into())
        })?;
        let mut current = project_root.to_path_buf();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Ok(true);
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                    return Ok(true)
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(err) => {
                    return Err(Self::asset_io_error(
                        "inspect asset destination directory",
                        &current,
                        err,
                    ))
                }
            }
        }
        Ok(false)
    }

    fn is_plain_directory(path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(metadata.is_dir() && !metadata.file_type().is_symlink()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(Self::asset_io_error(
                "inspect marketplace directory",
                path,
                err,
            )),
        }
    }

    fn copy_directory_without_symlinks(source: &Path, destination: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(source)
            .map_err(|err| Self::asset_io_error("inspect trusted asset", source, err))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(TupiError::CatalogValidation(format!(
                "trusted skill directory {} is not a regular directory",
                source.display()
            )));
        }
        fs::create_dir(destination)
            .map_err(|err| Self::asset_io_error("create skill directory", destination, err))?;
        for entry in fs::read_dir(source)
            .map_err(|err| Self::asset_io_error("read skill directory", source, err))?
        {
            let entry = entry.map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            let source_child = entry.path();
            let destination_child = destination.join(entry.file_name());
            let file_type = entry
                .file_type()
                .map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            if file_type.is_symlink() {
                return Err(TupiError::CatalogValidation(format!(
                    "trusted skill contains a symbolic link: {}",
                    source_child.display()
                )));
            }
            if file_type.is_dir() {
                Self::copy_directory_without_symlinks(&source_child, &destination_child)?;
            } else if file_type.is_file() {
                fs::copy(&source_child, &destination_child)
                    .map_err(|err| Self::asset_io_error("copy skill file", &source_child, err))?;
            } else {
                return Err(TupiError::CatalogValidation(format!(
                    "trusted skill contains an unsupported file: {}",
                    source_child.display()
                )));
            }
        }
        Ok(())
    }

    fn copy_staged_directory_without_overwrite(source: &Path, destination: &Path) -> Result<()> {
        for entry in fs::read_dir(source)
            .map_err(|err| Self::asset_io_error("read staged skill", source, err))?
        {
            let entry = entry.map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            let source_child = entry.path();
            let destination_child = destination.join(entry.file_name());
            let file_type = entry
                .file_type()
                .map_err(|err| TupiError::CatalogRead(err.to_string()))?;
            if file_type.is_symlink() {
                return Err(TupiError::ProfileValidation(format!(
                    "staged skill contains a symbolic link: {}",
                    source_child.display()
                )));
            }
            if file_type.is_dir() {
                fs::create_dir(&destination_child).map_err(|err| {
                    Self::asset_io_error(
                        "create installed skill directory",
                        &destination_child,
                        err,
                    )
                })?;
                Self::copy_staged_directory_without_overwrite(&source_child, &destination_child)?;
            } else if file_type.is_file() {
                Self::copy_file_without_overwrite(&source_child, &destination_child)?;
            } else {
                return Err(TupiError::ProfileValidation(format!(
                    "staged skill contains an unsupported file: {}",
                    source_child.display()
                )));
            }
        }
        Ok(())
    }

    fn copy_file_without_overwrite(source: &Path, destination: &Path) -> Result<()> {
        let mut source_file = fs::File::open(source)
            .map_err(|err| Self::asset_io_error("open staged asset", source, err))?;
        let mut destination_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|err| Self::asset_io_error("create copied asset", destination, err))?;
        let copy_result = (|| {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                let bytes_read = source_file
                    .read(&mut buffer)
                    .map_err(|err| Self::asset_io_error("read staged asset", source, err))?;
                if bytes_read == 0 {
                    break;
                }
                destination_file
                    .write_all(&buffer[..bytes_read])
                    .map_err(|err| Self::asset_io_error("write copied asset", destination, err))?;
            }
            destination_file
                .flush()
                .map_err(|err| Self::asset_io_error("flush copied asset", destination, err))
        })();
        copy_result
    }

    pub(super) fn content_digest(path: &Path) -> Result<String> {
        let mut digest = Sha256::new();
        Self::hash_path(path, Path::new(""), &mut digest)?;
        Ok(format!("{:x}", digest.finalize()))
    }

    fn hash_path(path: &Path, relative: &Path, digest: &mut Sha256) -> Result<()> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|err| Self::asset_io_error("inspect asset content", path, err))?;
        if metadata.file_type().is_symlink() {
            return Err(TupiError::ProfileValidation(format!(
                "asset contains a symbolic link: {}",
                path.display()
            )));
        }
        if metadata.is_file() {
            digest.update(b"file\0");
            digest.update(relative.to_string_lossy().as_bytes());
            digest.update(b"\0");
            let contents = fs::read(path)
                .map_err(|err| Self::asset_io_error("read asset content", path, err))?;
            digest.update((contents.len() as u64).to_be_bytes());
            digest.update(contents);
            return Ok(());
        }
        if metadata.is_dir() {
            digest.update(b"dir\0");
            digest.update(relative.to_string_lossy().as_bytes());
            digest.update(b"\0");
            let mut entries = fs::read_dir(path)
                .map_err(|err| Self::asset_io_error("read asset content", path, err))?
                .map(|entry| entry.map_err(|err| TupiError::CatalogRead(err.to_string())))
                .collect::<Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                Self::hash_path(&entry.path(), &relative.join(entry.file_name()), digest)?;
            }
            return Ok(());
        }
        Err(TupiError::ProfileValidation(format!(
            "unsupported asset type at {}",
            path.display()
        )))
    }

    fn relative_path_string(path: &Path) -> Result<String> {
        path.components()
            .map(|component| match component {
                Component::Normal(value) => value.to_str().map(str::to_string).ok_or_else(|| {
                    TupiError::ProfileValidation("asset destination is not valid UTF-8".into())
                }),
                _ => Err(TupiError::ProfileValidation(
                    "asset destination contains an invalid path component".into(),
                )),
            })
            .collect::<Result<Vec<_>>>()
            .map(|components| components.join("/"))
    }

    fn remove_path(path: &Path) -> Result<()> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(Self::asset_io_error("inspect asset path", path, err)),
        };
        if metadata.file_type().is_symlink() {
            return Err(TupiError::ProfileValidation(format!(
                "refusing to remove symbolic link {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            fs::remove_dir_all(path)
                .map_err(|err| Self::asset_io_error("remove installed skill", path, err))
        } else {
            fs::remove_file(path)
                .map_err(|err| Self::asset_io_error("remove installed agent", path, err))
        }
    }

    fn asset_io_error(action: &str, path: &Path, err: std::io::Error) -> TupiError {
        TupiError::ProfileValidation(format!("{action} at {} failed ({err})", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::{AppState, AssetIdentity, InstalledAsset};
    use crate::catalog::{load_catalog_from_str, summarize};
    use crate::profile::Profile;
    use rusqlite::params;
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    fn run_git(directory: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git should be available for profile installation tests");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    #[test]
    fn installed_asset_provenance_is_kept_in_tupi_state() {
        let state_root = crate::state::test_support::unique_temp_dir("profile-asset-registry");
        let project = crate::state::test_support::unique_temp_dir("profile-asset-registry-project");
        let state = crate::state::test_support::make_test_state(&state_root);
        let installed = InstalledAsset {
            asset_id: "agent:official:reviewer".into(),
            destination: ".github/agents/reviewer.agent.md".into(),
            digest: "sha256".into(),
        };

        state.store_installed_asset(&project, &installed).unwrap();
        assert_eq!(
            state
                .read_installed_asset(&project, &installed.asset_id)
                .unwrap()
                .unwrap()
                .digest,
            "sha256"
        );
        state
            .remove_installed_asset(&project, &installed.asset_id)
            .unwrap();
        assert!(state
            .read_installed_asset(&project, &installed.asset_id)
            .unwrap()
            .is_none());

        fs::remove_dir_all(state_root).unwrap();
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn stale_bundled_catalog_is_rejected_with_refresh_guidance() {
        let state_root = crate::state::test_support::unique_temp_dir("stale-profile-catalog");
        let project = crate::state::test_support::unique_temp_dir("stale-profile-project");
        let state = crate::state::test_support::make_test_state(&state_root);
        let profile = state
            .upsert_profile(Profile {
                id: "stale-profile".into(),
                name: "Project".into(),
                project_location: Some(project.to_string_lossy().to_string()),
                description: None,
                enabled: true,
                version: Some("1".into()),
                catalog_revision: None,
                selected_assets: Vec::new(),
            })
            .unwrap();

        let error = state.list_profile_assets(&profile.id).unwrap_err();
        assert!(error.to_string().contains("refresh the trusted catalog"));

        fs::remove_dir_all(state_root).unwrap();
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn profile_assets_install_and_uninstall_safely_from_a_pinned_marketplace() {
        let root = crate::state::test_support::unique_temp_dir("profile-assets-e2e");
        let repository = root.join("marketplace");
        let project = root.join("project");
        fs::create_dir_all(&repository).unwrap();
        fs::create_dir_all(&project).unwrap();
        run_git(&repository, &["init"]);
        run_git(&repository, &["checkout", "-b", "main"]);
        run_git(&repository, &["config", "user.name", "Tupi Test"]);
        run_git(
            &repository,
            &["config", "user.email", "tupi-test@example.invalid"],
        );
        fs::create_dir_all(repository.join("catalog")).unwrap();
        fs::write(
            repository.join("catalog").join("trusted-assets.yaml"),
            "version: 1\ncatalogRevision: source-test\nmarketplaces: []\nagents: []\nskills: []\n",
        )
        .unwrap();
        fs::create_dir_all(repository.join("agents")).unwrap();
        fs::write(
            repository.join("agents").join("reviewer.agent.md"),
            "# Reviewer",
        )
        .unwrap();
        let skill = repository
            .join("plugins")
            .join("test-plugin")
            .join("skills")
            .join("test-skill");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(skill.join("SKILL.md"), "# Test skill").unwrap();
        fs::write(skill.join("references").join("guide.md"), "# Guide").unwrap();
        fs::write(
            repository.join("agents").join("existing.agent.md"),
            "# Existing",
        )
        .unwrap();
        run_git(&repository, &["add", "."]);
        run_git(&repository, &["commit", "-m", "Add trusted test assets"]);
        let revision = run_git(&repository, &["rev-parse", "HEAD"]);
        let repository_url = url::Url::from_file_path(&repository)
            .expect("test repository path should map to a file URL")
            .to_string();
        let catalog_contents = format!(
            "version: 1\ncatalogRevision: test-catalog\nmarketplaces:\n  - id: official\n    name: Official\n    repository: {repository_url}\n    branch: main\n    revision: {revision}\nagents: []\nskills: []\n"
        );
        let state = crate::state::test_support::make_test_state_with_catalog(
            &root.join("tupi"),
            &catalog_contents,
        );
        assert_eq!(
            state.fetch_from_git(&repository_url, "main").unwrap(),
            "version: 1\ncatalogRevision: source-test\nmarketplaces: []\nagents: []\nskills: []\n"
        );
        let catalog = load_catalog_from_str(&catalog_contents).unwrap();
        let summary = summarize(&catalog);
        let connection = state.open().unwrap();
        connection
            .execute(
                r#"
                INSERT INTO catalog_cache (
                    id, version, catalog_revision, summary_json, catalog_yaml,
                    source_repository, source_branch, trust_status, stale, refreshed_at
                )
                VALUES (1, ?1, ?2, ?3, ?4, ?5, 'main', 'Trusted', 0, 'test')
                "#,
                params![
                    summary.version,
                    summary.catalog_revision,
                    serde_json::to_string(&summary).unwrap(),
                    catalog_contents,
                    "https://github.com/silvionetto/tupi.git"
                ],
            )
            .unwrap();
        drop(connection);
        state
            .sync_repository_workspace(
                "marketplaces",
                "official",
                &repository_url,
                "main",
                &revision,
                "test marketplace",
            )
            .unwrap();
        let profile = state
            .upsert_profile(Profile {
                id: "profile-assets-test".into(),
                name: "Project".into(),
                project_location: Some(project.to_string_lossy().to_string()),
                description: None,
                enabled: true,
                version: Some("1".into()),
                catalog_revision: Some(summary.catalog_revision),
                selected_assets: Vec::new(),
            })
            .unwrap();

        let assets = state.list_profile_assets(&profile.id).unwrap();
        assert_eq!(assets.len(), 3);
        let reviewer = assets
            .iter()
            .find(|asset| asset.name == "reviewer")
            .unwrap();
        let reviewer_id = reviewer.asset_id.clone();
        let skill = assets
            .iter()
            .find(|asset| asset.name == "test-skill")
            .unwrap();
        let skill_id = skill.asset_id.clone();
        let conflict_id = assets
            .iter()
            .find(|asset| asset.name == "existing")
            .unwrap()
            .asset_id
            .clone();
        let agent_target = project.join(".github/agents/reviewer.agent.md");
        let skill_target = project.join(".github/skills/test-skill");
        let conflict_target = project.join(".github/agents/existing.agent.md");
        fs::create_dir_all(conflict_target.parent().unwrap()).unwrap();
        fs::write(&conflict_target, "project-owned file").unwrap();

        state
            .install_profile_asset(&profile.id, &reviewer_id)
            .unwrap();
        state.install_profile_asset(&profile.id, &skill_id).unwrap();
        assert_eq!(fs::read_to_string(&agent_target).unwrap(), "# Reviewer");
        assert_eq!(
            fs::read_to_string(skill_target.join("references").join("guide.md")).unwrap(),
            "# Guide"
        );
        assert_eq!(
            state
                .list_profile_assets(&profile.id)
                .unwrap()
                .into_iter()
                .find(|asset| asset.asset_id == reviewer_id)
                .unwrap()
                .installation_state,
            "Installed"
        );

        assert!(state
            .install_profile_asset(&profile.id, &conflict_id)
            .is_err());
        assert_eq!(
            fs::read_to_string(&conflict_target).unwrap(),
            "project-owned file"
        );

        fs::write(&agent_target, "project-edited file").unwrap();
        assert!(state
            .uninstall_profile_asset(&profile.id, &reviewer_id)
            .is_err());
        assert_eq!(
            fs::read_to_string(&agent_target).unwrap(),
            "project-edited file"
        );
        fs::write(&agent_target, "# Reviewer").unwrap();
        state
            .uninstall_profile_asset(&profile.id, &reviewer_id)
            .unwrap();
        state
            .uninstall_profile_asset(&profile.id, &skill_id)
            .unwrap();
        assert!(!agent_target.exists());
        assert!(!skill_target.exists());

        drop(state);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installation_state_distinguishes_unmanaged_and_modified_assets() {
        let project = crate::state::test_support::unique_temp_dir("profile-asset-state");
        let destination = std::path::Path::new(".github/agents/reviewer.agent.md");
        let installed_file = project.join(destination);
        fs::create_dir_all(installed_file.parent().unwrap()).unwrap();
        fs::write(&installed_file, "trusted agent").unwrap();

        let identity = AssetIdentity {
            kind: "agent".into(),
            marketplace_id: "official".into(),
            plugin_name: None,
            name: "reviewer".into(),
        };
        let asset_id = AppState::asset_id(&identity).unwrap();
        assert_eq!(
            AppState::asset_installation_state(&project, destination, &asset_id, None).unwrap(),
            "Conflict"
        );

        let installed = InstalledAsset {
            asset_id: asset_id.clone(),
            destination: ".github/agents/reviewer.agent.md".into(),
            digest: AppState::content_digest(&installed_file).unwrap(),
        };
        assert_eq!(
            AppState::asset_installation_state(&project, destination, &asset_id, Some(&installed))
                .unwrap(),
            "Installed"
        );

        fs::write(&installed_file, "user-edited agent").unwrap();
        assert_eq!(
            AppState::asset_installation_state(&project, destination, &asset_id, Some(&installed))
                .unwrap(),
            "Modified"
        );
        assert_eq!(
            fs::read_to_string(&installed_file).unwrap(),
            "user-edited agent"
        );

        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn copy_and_digest_preserve_nested_skill_content() {
        let root = crate::state::test_support::unique_temp_dir("profile-skill-copy");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("references")).unwrap();
        fs::write(source.join("SKILL.md"), "# Skill").unwrap();
        fs::write(source.join("references").join("guide.md"), "# Guide").unwrap();

        AppState::copy_directory_without_symlinks(&source, &destination).unwrap();
        assert_eq!(
            AppState::content_digest(&source).unwrap(),
            AppState::content_digest(&destination).unwrap()
        );
        assert_eq!(
            fs::read_to_string(destination.join("references").join("guide.md")).unwrap(),
            "# Guide"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn install_copy_never_replaces_an_existing_file() {
        let root = crate::state::test_support::unique_temp_dir("profile-asset-no-overwrite");
        let source = root.join("source.agent.md");
        let destination = root.join("destination.agent.md");
        fs::write(&source, "trusted content").unwrap();
        fs::write(&destination, "user content").unwrap();

        assert!(AppState::copy_file_without_overwrite(&source, &destination).is_err());
        assert_eq!(fs::read_to_string(&destination).unwrap(), "user content");

        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn installation_state_rejects_symlinked_destination_parent() {
        let project = crate::state::test_support::unique_temp_dir("profile-asset-symlink");
        let outside = crate::state::test_support::unique_temp_dir("profile-asset-outside");
        let github = project.join(".github");
        std::os::unix::fs::symlink(&outside, &github).unwrap();

        let identity = AssetIdentity {
            kind: "agent".into(),
            marketplace_id: "official".into(),
            plugin_name: None,
            name: "reviewer".into(),
        };
        let result = AppState::asset_installation_state(
            &project,
            std::path::Path::new(".github/agents/reviewer.agent.md"),
            &AppState::asset_id(&identity).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(result, "Conflict");

        fs::remove_file(github).unwrap();
        fs::remove_dir_all(project).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
}
