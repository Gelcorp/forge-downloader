use std::{
    fmt::Debug,
    fs::{create_dir_all, File},
    path::PathBuf,
};

use chrono::{DateTime, Utc};
use forge_downloader::{Artifact, Sha1Sum};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zip::{write::FileOptions, ZipArchive, ZipWriter};

use super::{ForgeVersionInfo, ForgeVersionLibrary};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInstallerProfileV1 {
    pub install: InstallSectionV1,
    pub version_info: ForgeVersionInfo, /*ForgeVersionInfoV1*/
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optionals: Vec<ForgeOptional>,
}

impl ForgeInstallerProfileV1 {
    pub fn get_libraries(&self, marker: &str, filter: fn(&str) -> bool) -> Vec<ForgeLibrary> {
        let mut ret = vec![];
        self.version_info
            .libraries
            .iter()
            .filter_map(ForgeVersionLibrary::to_forge)
            .filter(|lib| lib.is_side(marker))
            .for_each(|lib| ret.push(lib.clone()));

        for opt in &self.optionals {
            let mut info = ForgeLibrary::new(&opt, marker);
            info.enabled = filter(&opt.artifact.get_descriptor());
            ret.push(info);
        }
        ret
    }

    pub fn is_inherited_json(&self) -> bool {
        self.version_info.inherits_from.is_some() && self.version_info.jar.is_some()
    }
}

// Only change between them: minecraftArguments TODO: check!
// #[derive(Debug, Serialize, Deserialize, Clone)]
// #[serde(rename_all = "camelCase")]
// pub struct ForgeVersionInfoV1 {
//     pub id: String,
//     pub time: DateTime<Utc>,
//     pub release_time: DateTime<Utc>,
//     #[serde(default, skip_serializing_if = "Option::is_none")]
//     pub inherits_from: Option<String>,
//     #[serde(rename = "type")]
//     pub release_type: String,
//     #[serde(default, skip_serializing_if = "Option::is_none")]
//     logging: Option<Value>,
//     pub main_class: String,
//     #[serde(default, skip_serializing_if = "Vec::is_empty")]
//     pub libraries: Vec<ForgeLibrary>,
//     #[serde(default, skip_serializing_if = "Option::is_none")]
//     pub jar: Option<String>,

//     #[serde(default, skip_serializing_if = "String::is_empty")]
//     pub minecraft_arguments: String,
// }

// #[derive(Serialize, Deserialize)]
// #[serde(untagged)]
// pub enum LibraryV1 {
//     Mojang(MojangLibrary),
//     Forge(ForgeLibrary),
// }

// impl Debug for LibraryV1 {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             LibraryV1::Mojang(mojang) => {
//                 write!(f, "{:?}", mojang)
//             }
//             LibraryV1::Forge(forge) => {
//                 write!(f, "{:?}", forge)
//             }
//         }
//     }
// }

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForgeLibrary {
    pub name: Artifact,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serverreq: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clientreq: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checksums: Vec</*String*/ Sha1Sum>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,

    #[serde(default = "ForgeLibrary::enabled_by_default", skip)]
    pub enabled: bool,
}

impl ForgeLibrary {
    pub fn new(lib: &ForgeOptional, marker: &str) -> Self {
        let mut serverreq = None;
        let mut clientreq = None;
        if marker == "clientreq" {
            clientreq = Some(true);
        } else {
            serverreq = Some(true);
        }

        Self {
            name: lib.artifact.clone(),
            clientreq,
            serverreq,
            url: Some(lib.url.clone()),
            checksums: vec![],
            comment: String::new(),
            enabled: true,
        }
    }

    pub fn get_url(&self) -> String {
        if let Some(url) = &self.url {
            // If it has mirrors, return mirror url (so self.url is ignored, idk why)
            format!("{url}/")
        } else {
            return "https://libraries.minecraft.net/".to_string();
        }
    }

    pub fn is_side(&self, side: &str) -> bool {
        if side == "clientreq" {
            self.clientreq.unwrap_or_default()
        } else {
            self.serverreq.unwrap_or_default()
        }
    }

    fn enabled_by_default() -> bool {
        true
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForgeOptional {
    pub name: String,
    pub client: bool,
    pub server: bool,
    pub default: bool,
    pub inject: bool,
    pub desc: String,
    pub url: String,
    pub artifact: Artifact,
    pub maven: String,
}

impl ForgeOptional {
    pub fn save_mod_list_json(
        root: &PathBuf,
        json: &PathBuf,
        libs: &Vec<ForgeOptional>,
        filter: fn(&str) -> bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut artifacts = vec![];
        for lib in libs {
            if filter(&lib.artifact.get_descriptor()) {
                artifacts.push(lib.artifact.clone());
            }
        }
        if artifacts.is_empty() {
            return Ok(());
        }
        let parent = json.parent().unwrap();
        if !parent.exists() {
            create_dir_all(parent)?;
        }
        println!("Saving optional modlist to: {}", json.display());
        let mut buf = json!({
            "repositoryRoot": root.to_str().unwrap().replace("\\", "/"),
            "modRef": artifacts.iter().map(|art| art.get_descriptor()).collect::<Vec<String>>()
        });
        serde_json::to_writer_pretty(File::create(json)?, &buf)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InstallSectionV1 {
    pub profile_name: String,
    pub target: String,
    pub path: Artifact,
    pub version: String,
    pub file_path: String,
    pub welcome: String,
    pub minecraft: String,
    pub logo: String,
    pub mirror_list: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mod_list: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_meta: Option<bool>,
}
