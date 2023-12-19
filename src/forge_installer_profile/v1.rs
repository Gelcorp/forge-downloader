use chrono::{ Utc, DateTime };
use serde::{ Serialize, Deserialize };

use crate::lib::Artifact;

use super::v2::MojangLibrary;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInstallerProfileV1 {
    install: InstallSection,
    version_info: ForgeVersionFileV1,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    optionals: Vec<ForgeOptional>,
}

// Only change between them: minecraftArguments TODO: check!
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeVersionFileV1 {
    pub id: String,
    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,
    #[serde(rename = "type")]
    pub release_type: String,
    pub minecraft_arguments: String,
    pub main_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,
    // logging,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<MojangLibrary>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeLibrary {
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    serverreq: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    clientreq: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    checksums: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    comment: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeOptional {
    name: String,
    client: bool,
    server: bool,
    default: bool,
    inject: bool,
    desc: String,
    url: String,
    artifact: Artifact,
    maven: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSection {
    profile_name: String,
    target: String,
    path: Artifact,
    version: String,
    file_path: String,
    welcome: String,
    minecraft: String,
    logo: String,
    mirror_list: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mod_list: Option<String>,
}
