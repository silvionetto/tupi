use crate::error::{Result, TupiError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedCatalog {
    pub version: u32,
    #[serde(rename = "catalogRevision")]
    pub catalog_revision: String,
    #[serde(default)]
    pub marketplaces: Vec<Marketplace>,
    #[serde(default)]
    pub agents: Vec<AssetEntry>,
    #[serde(default)]
    pub prompts: Vec<AssetEntry>,
    #[serde(default)]
    pub skills: Vec<AssetEntry>,
    #[serde(default)]
    pub instructions: Vec<AssetEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Marketplace {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub branch: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: String,
    pub marketplace: String,
    pub path: String,
    pub version: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogSummary {
    pub version: u32,
    #[serde(rename = "catalogRevision")]
    pub catalog_revision: String,
    pub marketplaces: usize,
    pub assets: usize,
}

pub fn load_catalog_from_str(contents: &str) -> Result<TrustedCatalog> {
    let catalog: TrustedCatalog =
        serde_yaml::from_str(contents).map_err(|err| TupiError::CatalogParse(err.to_string()))?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

pub fn summarize(catalog: &TrustedCatalog) -> CatalogSummary {
    CatalogSummary {
        version: catalog.version,
        catalog_revision: catalog.catalog_revision.clone(),
        marketplaces: catalog.marketplaces.len(),
        assets: catalog.assets_len(),
    }
}

pub fn validate_catalog(catalog: &TrustedCatalog) -> Result<()> {
    if catalog.version == 0 {
        return Err(TupiError::CatalogValidation("catalog version must be positive".into()));
    }
    if catalog.catalog_revision.trim().is_empty() {
        return Err(TupiError::CatalogValidation("catalogRevision is required".into()));
    }

    validate_unique_ids("marketplace", catalog.marketplaces.iter().map(|m| m.id.as_str()))?;
    for marketplace in &catalog.marketplaces {
        validate_identifier("marketplace.id", &marketplace.id)?;
        validate_non_empty("marketplace.name", &marketplace.name)?;
        validate_non_empty("marketplace.branch", &marketplace.branch)?;
        if marketplace.branch != "main" {
            return Err(TupiError::CatalogValidation(format!(
                "marketplace {} must be pinned to main",
                marketplace.id
            )));
        }
        validate_revision("marketplace.revision", &marketplace.revision)?;
        Url::parse(&marketplace.repository)?;
    }

    let marketplace_ids: HashSet<&str> = catalog.marketplaces.iter().map(|m| m.id.as_str()).collect();
    validate_asset_group("agent", &catalog.agents, &marketplace_ids)?;
    validate_asset_group("prompt", &catalog.prompts, &marketplace_ids)?;
    validate_asset_group("skill", &catalog.skills, &marketplace_ids)?;
    validate_asset_group("instruction", &catalog.instructions, &marketplace_ids)?;

    Ok(())
}

fn validate_unique_ids<'a, I>(label: &str, ids: I) -> Result<()>
where
    I: Iterator<Item = &'a str>,
{
    let mut seen = HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(TupiError::CatalogValidation(format!("duplicate {} id: {}", label, id)));
        }
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> Result<()> {
    validate_non_empty(label, value)?;
    if value.contains(' ') {
        return Err(TupiError::CatalogValidation(format!("{label} must not contain spaces")));
    }
    Ok(())
}

fn validate_non_empty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(TupiError::CatalogValidation(format!("{label} is required")));
    }
    Ok(())
}

fn validate_revision(label: &str, value: &str) -> Result<()> {
    validate_non_empty(label, value)?;
    if value == "latest" {
        return Err(TupiError::CatalogValidation(format!("{label} must be immutable")));
    }
    Ok(())
}

fn validate_asset_group(
    label: &str,
    assets: &[AssetEntry],
    marketplace_ids: &HashSet<&str>,
) -> Result<()> {
    validate_unique_ids(label, assets.iter().map(|asset| asset.id.as_str()))?;
    for asset in assets {
        validate_identifier("asset.id", &asset.id)?;
        validate_identifier("asset.marketplace", &asset.marketplace)?;
        validate_non_empty("asset.path", &asset.path)?;
        validate_revision("asset.revision", &asset.revision)?;
        if !marketplace_ids.contains(asset.marketplace.as_str()) {
            return Err(TupiError::CatalogValidation(format!(
                "asset {} references unknown marketplace {}",
                asset.id, asset.marketplace
            )));
        }
    }
    Ok(())
}

impl TrustedCatalog {
    fn assets_len(&self) -> usize {
        self.agents.len() + self.prompts.len() + self.skills.len() + self.instructions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_marketplaces() {
        let catalog = TrustedCatalog {
            version: 1,
            catalog_revision: "abc".into(),
            marketplaces: vec![
                Marketplace {
                    id: "official".into(),
                    name: "Official".into(),
                    repository: "https://example.com/repo.git".into(),
                    branch: "main".into(),
                    revision: "abc".into(),
                },
                Marketplace {
                    id: "official".into(),
                    name: "Official 2".into(),
                    repository: "https://example.com/repo2.git".into(),
                    branch: "main".into(),
                    revision: "def".into(),
                },
            ],
            agents: vec![],
            prompts: vec![],
            skills: vec![],
            instructions: vec![],
        };

        assert!(validate_catalog(&catalog).is_err());
    }

    #[test]
    fn rejects_unknown_marketplace() {
        let catalog = TrustedCatalog {
            version: 1,
            catalog_revision: "abc".into(),
            marketplaces: vec![Marketplace {
                id: "official".into(),
                name: "Official".into(),
                repository: "https://example.com/repo.git".into(),
                branch: "main".into(),
                revision: "abc".into(),
            }],
            agents: vec![AssetEntry {
                id: "agent".into(),
                marketplace: "missing".into(),
                path: "agents/agent".into(),
                version: "1.0.0".into(),
                revision: "abc".into(),
            }],
            prompts: vec![],
            skills: vec![],
            instructions: vec![],
        };

        assert!(validate_catalog(&catalog).is_err());
    }
}
