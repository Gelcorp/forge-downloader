use std::collections::HashMap;

use regex::Regex;
use reqwest::Client;
use serde::{ Deserialize, Serialize };

const VERSIONS_URL: &str = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct NeoforgeVersions {
  is_snapshot: bool,
  versions: Vec<String>,
}

pub async fn fetch_neoforge_versions() -> Result<NeoforgeVersions, Box<dyn std::error::Error>> {
  Ok(Client::new().get(VERSIONS_URL).send().await?.error_for_status()?.json::<NeoforgeVersions>().await.map_err(Box::new)?)
}

pub fn build_list_neoforge_versions(versions: &NeoforgeVersions) -> HashMap<String, Vec<String>> {
  let version_pattern = Regex::new(r"^(?P<Minecraft_Version>\d*\.\d*)\.(?P<Neo_Version>.+)$").unwrap();
  let mut version_list = HashMap::new();
  for neo_version in &versions.versions {
    let matches = version_pattern.captures(neo_version).unwrap();
    let mc_version = format!("1.{}", matches.name("Minecraft_Version").unwrap().as_str());

    version_list.entry(mc_version).or_insert_with(Vec::new).push(neo_version.clone());
  }
  version_list
}

pub fn build_promoted_versions(versions: &HashMap<String, Vec<String>>) -> HashMap<String, String> {
  let mut promotions = HashMap::new();

  for (mc_version, neo_versions) in versions {
    for neo_version in neo_versions {
      let latest_key = format!("{mc_version}-latest");
      if let Some(latest) = promotions.get(&latest_key) {
        if neo_version > latest {
          promotions.insert(latest_key, neo_version.clone());
        }
      } else {
        promotions.insert(latest_key, neo_version.clone());
      }

      let recommended_key = format!("{mc_version}-recommended");
      if !neo_version.ends_with("-beta") {
        if let Some(recommended) = promotions.get(&recommended_key) {
          if neo_version > recommended {
            promotions.insert(recommended_key, neo_version.clone());
          }
        } else {
          promotions.insert(recommended_key, neo_version.clone());
        }
      }
    }
  }
  promotions
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_fetch_neoforge_versions() {
    let versions = fetch_neoforge_versions().await.unwrap();
    assert!(!versions.versions.is_empty());
    println!("{:?}", versions);
    let version_list = build_list_neoforge_versions(&versions);
    println!("{:#?}", build_promoted_versions(&version_list));
  }
}
