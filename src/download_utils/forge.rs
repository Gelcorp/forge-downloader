use std::error::Error;
use serde::{ Deserialize, Serialize };

use crate::Artifact;

use super::neoforge;

pub struct ForgeVersionHandler {
  pub versions: Vec<ForgeVersionInfo>,
}

impl ForgeVersionHandler {
  pub async fn new() -> Result<Self, Box<dyn Error>> {
    let neoforge_version = neoforge::fetch_neoforge_versions().await?;
    let neoforge_versions = neoforge::build_list_neoforge_versions(&neoforge_version);
    let promotions = neoforge::build_promoted_versions(&neoforge_versions);

    let mut versions = vec![];
    for (mc_ver, forge_versions) in neoforge_versions {
      let recommended = promotions.get(&format!("{mc_ver}-recommended"));
      let latest = promotions.get(&format!("{mc_ver}-latest"));

      for forge_ver in forge_versions {
        let (forge_ver, suffix) = match forge_ver.split_once("-") {
          Some(parts) => (parts.0, Some(parts.1)),
          None => (forge_ver.as_str(), None),
        };

        let recommended = recommended.is_some_and(|ver| ver == forge_ver);
        let latest = latest.is_some_and(|ver| ver == forge_ver);
        versions.push(ForgeVersionInfo {
          mc_version: mc_ver.clone(),
          neoforge_version: forge_ver.to_string(),
          suffix: suffix.map(str::to_string),
          latest,
          recommended,
        });
      }
    }
    Ok(Self { versions })
  }

  pub fn get_best_version(&self, mc_ver: &str) -> Option<&ForgeVersionInfo> {
    let mut versions = self.get_by_mc_version(mc_ver).into_iter();
    versions.find(|v| v.recommended).or(versions.find(|v| v.latest))
  }

  pub fn get_by_mc_version(&self, mc_ver: &str) -> Vec<&ForgeVersionInfo> {
    self.versions
      .iter()
      .filter(|v| v.mc_version == mc_ver)
      .collect()
  }

  pub fn get_by_forge_version(&self, forge_ver: &str) -> Option<&ForgeVersionInfo> {
    self.versions.iter().find(|v| v.neoforge_version == forge_ver)
  }

  pub fn get_recommended_versions(&self) -> Vec<&ForgeVersionInfo> {
    self.versions
      .iter()
      .filter(|v| v.recommended)
      .collect()
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ForgeVersionInfo {
  pub mc_version: String,
  pub neoforge_version: String,
  pub suffix: Option<String>,
  pub latest: bool,
  pub recommended: bool,
}

impl ForgeVersionInfo {
  pub fn get_full_version(&self) -> String {
    let mut full_version = self.neoforge_version.clone();
    if let Some(suffix) = &self.suffix {
      full_version.push('-');
      full_version.push_str(suffix);
    }
    full_version
  }

  pub fn get_artifact(&self) -> Artifact {
    let path = format!("net.neoforged:neoforge:{}:installer", self.neoforge_version);
    Artifact::try_from(path).unwrap()
  }

  pub fn get_installer_url(&self) -> String {
    let path = self.get_artifact().get_path_string();
    format!("https://maven.neoforged.net/releases/{path}")
  }
}
