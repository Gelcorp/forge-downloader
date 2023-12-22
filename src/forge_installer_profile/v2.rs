use std::collections::HashMap;

use chrono::{DateTime, Utc};
use forge_downloader::{Sha1Sum, Artifact};
use serde::{ Deserialize, Serialize };
use serde_json::Value;

use super::ForgeVersionLibrary;

#[derive(Debug, Serialize, Deserialize)]
#[serde(/*deny_unknown_fields, */ rename_all = "camelCase")]
pub struct ForgeInstallerProfileV2 {
    // #[serde(default, rename="_comment", skip_serializing_if = "Vec::is_empty")]
    // _comment: Vec<String>,
    pub spec: u8,
    pub profile: String, // "forge"
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub json: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Artifact>, // GradleSpecifierProperty
    pub logo: String,
    pub minecraft: String,
    pub welcome: String,
    pub data: HashMap<String, DataFile>,
    pub processors: Vec<Processor>,
    pub libraries: Vec<ForgeVersionLibrary>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hide_extract: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_list: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_jar_path: Option<String>,
}

impl ForgeInstallerProfileV2 {
    pub fn get_processors(&self, side: &str) -> Vec<&Processor> {
        self.processors
            .iter()
            .filter(|p| p.is_side(side))
            .collect()
    }

    pub fn get_data(&self, is_client: bool) -> HashMap<&String, &String> {
        self.data
            .iter()
            .map(|(key, data_file)| {
                let url = if is_client { &data_file.client } else { &data_file.server };
                (key, url)
            })
            .collect::<HashMap<_, _>>()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataFile {
    client: String,
    server: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Processor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sides: Option<Vec<String>>,
    pub jar: Artifact,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classpath: Vec<Artifact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub outputs: HashMap<String, Option<String>>,
}

impl Processor {
    pub fn is_side(&self, side: &str) -> bool {
        if let Some(sides) = &self.sides { sides.contains(&side.to_string()) } else { true }
    }
}

// Move to mod
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangLibrary {
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // extract: Option<MojangLibraryExtractRules>,
    pub name: Artifact,
    pub downloads: /*Option<*/MojangLibraryDownloads/* >*/,
    // natives:
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangLibraryDownloads {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<MojangArtifact>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, MojangArtifact>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String /*Artifact*/>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha1: Option</*String*/ Sha1Sum>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u32>,
}

impl MojangArtifact {
    pub fn new(artifact: String) -> Self {
        Self {
            path: Some(artifact),
            sha1: None,
            size: None,
            url: None,
        }
    }
}

// Only change between them: minecraftArguments TODO: check!
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeVersionFileV2 {
    pub id: String,
    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(rename="type")]
    pub release_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    logging: Option<Value>,
    pub main_class: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub downloads: HashMap<String, Download>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<MojangLibrary>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub arguments: HashMap<String, Value>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Download {
    pub sha1: Sha1Sum /*String*/,
    pub size: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default)]
    pub provided: bool,
}

impl Download {
    pub fn get_url(&self) -> Option<String> {
        if self.url.is_none() || self.provided { Some(String::new()) } else { self.url.clone() }
    }
}

// // TODO: extend Download
// #[derive(Debug, Serialize, Deserialize)]
// pub struct LibraryDownload {
//   pub sha1: String,
//   pub size: u16,
//   #[serde(default, skip_serializing_if = "Option::is_none")]
//   pub url: Option<String>,
//   #[serde(default)]
//   pub provided: bool,
//   pub path: String
// }

// #[derive(Debug, Serialize, Deserialize)]
// pub struct Library {
//   pub name: Artifact,
//   pub downloads: Downloads,
// }

// #[derive(Debug, Serialize, Deserialize)]
// pub struct Downloads {
//   pub artifact: LibraryDownload,
//   #[serde(default, skip_serializing_if = "HashMap::is_empty")]
//   pub classifiers: HashMap<String, LibraryDownload>,
// }
