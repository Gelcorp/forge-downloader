#[macro_use]
pub mod forge_client_install;
pub mod forge_installer_profile;
pub mod post_processors;
pub mod download_utils;

use std::{ fmt::{ Debug, Display }, fs, io::Read, path::PathBuf };

use chrono::{ DateTime, Utc };
use reqwest::Client;
use serde::{ Deserialize, Serialize };
use serde_json::Value;
use sha1::{ Digest, Sha1 };

#[derive(Serialize, Deserialize, Clone)]
#[serde(try_from = "String", into = "String")]
pub struct Artifact {
  original_descriptor: Option<String>,
  pub group_id: Vec<String>,
  pub artifact_id: String,
  pub version: String,
  pub classifier: Option<String>,
  pub ext: String,
}

impl Debug for Artifact {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.get_descriptor())
  }
}

impl Display for Artifact {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.get_descriptor())
  }
}

impl Artifact {
  pub fn get_file(&self) -> String {
    let mut name = format!("{}-{}", self.artifact_id, self.version);
    if let Some(classifier) = &self.classifier {
      name.push_str(&format!("-{}", classifier));
    }
    name.push_str(&format!(".{}", self.ext));
    name
  }

  pub fn get_path_vec(&self) -> Vec<String> {
    let mut vec = self.group_id.clone();
    vec.push(self.artifact_id.clone());
    vec.push(self.version.clone());
    vec.push(self.get_file());
    vec
  }

  pub fn get_path_string(&self) -> String {
    self.get_path_vec().join("/")
  }

  pub fn get_local_path(&self, root: &PathBuf) -> PathBuf {
    let mut root = root.clone();
    for s in self.get_path_vec() {
      root = root.join(s);
    }
    root
  }

  pub fn get_descriptor(&self) -> String {
    if let Some(original_descriptor) = &self.original_descriptor {
      original_descriptor.clone()
    } else {
      let mut descriptor = format!("{}:{}:{}", self.group_id.join("."), self.artifact_id, self.version);
      if let Some(classifier) = &self.classifier {
        descriptor.push_str(&format!(":{}", classifier));
      }
      descriptor
    }
  }
}

impl TryFrom<String> for Artifact {
  type Error = String;
  fn try_from(value: String) -> Result<Self, Self::Error> {
    let (value, ext) = value.split_once("@").unwrap_or((&value, "jar"));

    let parts: Vec<&str> = value.split(":").collect();
    if parts.len() < 3 {
      return Err(format!("Invalid artifact path: {}", value));
    }
    let group_id: Vec<String> = parts[0]
      .split(".")
      .map(|s| s.to_string())
      .collect();
    let artifact_id = parts[1].to_string();
    let version = parts[2].to_string();
    let classifier = parts.get(3).map(|s| s.to_string());
    Ok(Self {
      original_descriptor: Some(value.to_string()),
      group_id,
      artifact_id,
      version,
      classifier,
      ext: ext.to_string(),
    })
  }
}

impl Into<String> for Artifact {
  fn into(self) -> String {
    self.get_descriptor()
  }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(try_from = "String", into = "String")]
pub struct Sha1Sum([u8; 20]);

impl Sha1Sum {
  pub fn new(value: [u8; 20]) -> Self {
    Self(value)
  }

  pub fn from_reader<T: Read>(value: &mut T) -> Result<Self, Box<dyn std::error::Error>> {
    let mut sha1_hasher = Sha1::new();
    let mut buf = vec![];
    value.read_to_end(&mut buf)?;
    sha1_hasher.update(&buf);
    Ok(Sha1Sum(sha1_hasher.finalize().into()))
  }
}

impl TryFrom<String> for Sha1Sum {
  type Error = String;
  fn try_from(value: String) -> Result<Self, Self::Error> {
    let mut buf = [0u8; 20];
    hex::decode_to_slice(value, &mut buf).map_err(|e| e.to_string())?;
    Ok(Sha1Sum(buf))
  }
}

impl Into<String> for Sha1Sum {
  fn into(self) -> String {
    hex::encode(self.0)
  }
}

impl Debug for Sha1Sum {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", hex::encode(self.0))
  }
}

impl Display for Sha1Sum {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", hex::encode(self.0))
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PartialVersion {
  pub id: String,
  pub time: DateTime<Utc>,
  pub release_time: DateTime<Utc>,
  #[serde(rename = "type")]
  pub release_type: String,
  pub url: String,
}

async fn download_manifest() -> Result<Vec<PartialVersion>, Box<dyn std::error::Error>> {
  const URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
  let response: Value = Client::new().get(URL).send().await?.json().await?;
  let versions = response.get("versions").unwrap();
  Ok(serde_json::from_value(versions.clone())?)
}

pub async fn get_vanilla_version(mc_version: &str, json_path: &PathBuf) -> Option<Value> {
  let bytes = if json_path.is_file() {
    fs::read(json_path).ok()?
  } else {
    let versions = download_manifest().await.ok()?;
    let url = versions.into_iter().find(|v| v.id == mc_version)?.url;
    let bytes = Client::new().get(url).send().await.ok()?.bytes().await.ok()?;
    fs::write(json_path, &bytes).ok()?;
    bytes.to_vec()
  };
  Some(serde_json::from_slice(&bytes).ok()?)
}

#[cfg(test)]
mod tests {
  use futures::future::join_all;
  use zip::ZipArchive;

  use crate::forge_installer_profile::{ ForgeInstallerProfile, v2::ForgeInstallerProfileV2, v1::ForgeInstallerProfileV1 };

  use super::{ *, download_utils::forge::ForgeVersionHandler, forge_client_install::ForgeClientInstall };
  use std::{ env::temp_dir, io::{ Cursor, Write }, fs::File, str::FromStr };

  #[tokio::test]
  async fn install_test() -> Result<(), Box<dyn std::error::Error>> {
    let versions = ForgeVersionHandler::new().await?;
    let version = versions.get_best_version("1.20.1").unwrap();

    let url = version.get_installer_url();
    println!("☕ Installer jar url: {}", url);

    let response = Client::new().get(&url).send().await?;
    if !response.status().is_success() {
      println!("❌ Couldn't download: {}", response.status());
      return Ok(());
    }
    let bytes = response.bytes().await?;

    let game_dir = temp_dir().join(".minecraft-core-test"); //Path::new(env!("APPDATA")).join(".minecraft");

    fs::create_dir_all(&temp_dir())?;
    let installer_path = temp_dir().join("forge-installer.jar");

    fs::write(&installer_path, bytes)?;

    /*
       TODO: refactor serde stuff
       TODO: add monitor struct to manage logs and stuff, see how
    */
    let mut installer = ForgeClientInstall::new(
      installer_path,
      PathBuf::from_str("C:/Program Files/Eclipse Adoptium/jdk-17.0.6.10-hotspot/bin/java.exe").unwrap()
    )?;
    installer.install_forge(&game_dir, |_| true).await?;
    Ok(())
  }

  #[tokio::test]
  async fn test_parser() -> Result<(), Box<dyn std::error::Error>> {
    let cache_folder = std::env::temp_dir().join("forge_cache_versions");
    fs::create_dir_all(&cache_folder)?;

    let versions = ForgeVersionHandler::new().await?;
    let recommended_versions: Vec<String> = versions
      .get_recommended_versions()
      .iter()
      .map(|ver| ver.get_full_version())
      .collect();

    let forge_versions = recommended_versions.chunks(3).collect::<Vec<_>>();
    for versions in forge_versions {
      let futures = versions
        .into_iter()
        .map(|ver| process_version(&ver, &cache_folder))
        .collect::<Vec<_>>();
      join_all(futures).await;
    }
    Ok(())
  }

  async fn process_version(full_forge_version: &str, cache_folder: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let artifact = Artifact::try_from(format!("net.minecraftforge:forge:{full_forge_version}:installer"))?;

    let path = &cache_folder.join(artifact.get_file());
    let mut zip_archive = if path.is_file() {
      print!("\n💾 Loading {} from cache", full_forge_version);
      ZipArchive::new(Cursor::new(fs::read(path)?))?
    } else {
      let url = format!("https://maven.minecraftforge.net/{}", artifact.get_path_string());

      let response = Client::new().get(&url).send().await?;
      if !response.status().is_success() {
        println!("❌ Couldn't download {}: {}", full_forge_version, response.status());
        // println!("  \\- Error: {}", response.status());
        // continue;
        return Ok(());
      }
      print!("\n⏳ Downloading {} ", full_forge_version);
      print!("\n Url: {} ", url);

      let bytes = response.bytes().await?.to_vec();
      File::create(path)?.write_all(&bytes)?;
      ZipArchive::new(Cursor::new(bytes))?
    };
    // println!("  \\- Archive downloaded! Opening it...");
    if let Ok(mut install_profile) = zip_archive.by_name("install_profile.json") {
      let mut bytes = vec![];
      install_profile.read_to_end(&mut bytes)?;
      let result = serde_json::from_slice::<ForgeInstallerProfileV1>(bytes.as_slice()).map(|v| ForgeInstallerProfile::V1(v));
      let result2 = serde_json::from_slice::<ForgeInstallerProfileV2>(bytes.as_slice()).map(|v| ForgeInstallerProfile::V2(v));

      if result.is_err() && result2.is_err() {
        println!("");
        if let Err(err) = &result {
          println!("❌ Error V1: {}", err);
        }
        if let Err(err) = &result2 {
          println!("❌ Error V2: {}", err);
        }
        // continue;
        return Ok(());
      }
    } else {
      println!("❌ Install profile not found");
      // continue;
      return Ok(());
      // return Err("Install profile not found".into());
    }
    println!("✅ OK");
    Ok(())
  }
}
