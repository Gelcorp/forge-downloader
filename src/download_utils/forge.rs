use std::{ collections::HashMap, error::Error };
use reqwest::Client;
use serde::{ Deserialize, Serialize };
use serde_json::Value;

use crate::Artifact;

const PROMOTIONS_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const METADATA_URL: &str = "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";

pub struct ForgeVersionHandler {
  pub versions: Vec<ForgeVersionInfo>,
}

impl ForgeVersionHandler {
  pub async fn new() -> Result<Self, Box<dyn Error>> {
    let promotions = get_promoted_versions().await?;

    let mut versions = vec![];
    for (mc_ver, forge_versions) in list_forge_versions().await? {
      let recommended = promotions.get(&format!("{mc_ver}-recommended"));
      let latest = promotions.get(&format!("{mc_ver}-latest"));

      for full_forge_ver in forge_versions {
        let forge_ver = full_forge_ver.split_once("-").unwrap().1;

        let (forge_ver, suffix) = match forge_ver.split_once("-") {
          Some(parts) => (parts.0, Some(parts.1)),
          None => (forge_ver, None),
        };

        let recommended = recommended.is_some_and(|ver| ver == forge_ver);
        let latest = latest.is_some_and(|ver| ver == forge_ver);
        versions.push(ForgeVersionInfo {
          mc_version: mc_ver.clone(),
          forge_version: forge_ver.to_string(),
          suffix: suffix.map(|s| s.to_string()),
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
    self.versions.iter().find(|v| v.forge_version == forge_ver)
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
  pub forge_version: String,
  pub suffix: Option<String>,
  pub latest: bool,
  pub recommended: bool,
}

impl ForgeVersionInfo {
  pub fn get_full_version(&self) -> String {
    let mut parts: Vec<&str> = vec![&self.mc_version, &self.forge_version];
    if let Some(suffix) = &self.suffix {
      parts.push(suffix);
    }
    parts.join("-")
  }

  pub fn get_artifact(&self) -> Artifact {
    let path = format!("net.minecraftforge:forge:{}:installer", self.get_full_version());
    Artifact::try_from(path).unwrap()
  }

  pub fn get_installer_url(&self) -> String {
    let path = self.get_artifact().get_path_string();
    format!("https://maven.minecraftforge.net/{path}")
  }
}

// [mc_ver]: "{mc_ver}-{forge_ver}"
pub async fn list_forge_versions() -> Result<HashMap<String, Vec<String>>, reqwest::Error> {
  Client::new().get(METADATA_URL).send().await?.json().await
}

// "{mc_ver}-latest": "{forge_ver}"
pub async fn get_promoted_versions() -> Result<HashMap<String, String>, Box<dyn Error>> {
  let result: Value = Client::new().get(PROMOTIONS_URL).send().await?.json().await?;

  let mut promos = HashMap::new();
  for (mc_version, forge_version) in result["promos"].as_object().unwrap() {
    let forge_version = forge_version.as_str().unwrap().to_string();
    promos.insert(mc_version.clone(), forge_version);
  }
  Ok(promos)
}