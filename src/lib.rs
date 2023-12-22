// https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.4-49.0.7/forge-1.20.4-49.0.7-installer.jar

use std::{
    fmt::{Debug, Display},
    fs,
    io::Read,
    path::PathBuf,
};

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha1::{digest::generic_array::GenericArray, Digest, Sha1};

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
            let mut descriptor = format!(
                "{}:{}:{}",
                self.group_id.join("."),
                self.artifact_id,
                self.version
            );
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
        let group_id: Vec<String> = parts[0].split(".").map(|s| s.to_string()).collect();
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
        let result = sha1_hasher.finalize() as GenericArray<u8, _>;
        Ok(Sha1Sum(result.into()))
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
        let bytes = Client::new()
            .get(url)
            .send()
            .await
            .ok()?
            .bytes()
            .await
            .ok()?;
        fs::write(json_path, &bytes).ok()?;
        bytes.to_vec()
    };
    Some(serde_json::from_slice(&bytes).ok()?)
}
