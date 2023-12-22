use std::{
    collections::HashMap,
    io::{Read, Seek},
    path::Path,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zip::{read::ZipFile, result::ZipError, ZipArchive};

use self::{
    v1::{ForgeInstallerProfileV1, ForgeLibrary},
    v2::{ForgeInstallerProfileV2, MojangLibrary, Processor},
};

pub mod v1;
pub mod v2;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ForgeInstallerProfile {
    V1(v1::ForgeInstallerProfileV1),
    V2(v2::ForgeInstallerProfileV2),
}

impl ForgeInstallerProfile {
    pub fn from_reader<T: Read>(mut reader: T) -> Self {
        let mut bytes = vec![];
        reader.read_to_end(&mut bytes).unwrap();
        let result = serde_json::from_slice::<ForgeInstallerProfileV1>(bytes.as_slice())
            .map(|v| ForgeInstallerProfile::V1(v));
        let result2 = serde_json::from_slice::<ForgeInstallerProfileV2>(bytes.as_slice())
            .map(|v| ForgeInstallerProfile::V2(v));

        if result.is_err() && result2.is_err() {
            println!("");
            if let Err(err) = &result {
                println!("❌ Error V1: {}", err);
            }
            if let Err(err) = &result2 {
                println!("❌ Error V2: {}", err);
            }
            panic!("Couldn't parse installer profile");
        }
        result.or(result2).unwrap()
    }

    pub fn get_processors(&self, side: &str) -> Vec<&Processor> {
        match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => profile.get_processors(side),
        }
    }

    pub fn get_data(&self, is_client: bool) -> HashMap<&String, &String> {
        match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => profile.get_data(is_client),
        }
    }

    pub fn get_libraries(&self) -> Vec<&ForgeVersionLibrary> {
        match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => profile.libraries.iter().collect(),
        }
    }

    pub fn get_version_id(&self) -> String {
        match self {
            ForgeInstallerProfile::V1(profile) => profile.install.target.clone(),
            ForgeInstallerProfile::V2(profile) => profile.version.clone(),
        }
    }

    pub fn get_minecraft(&self) -> String {
        match self {
            ForgeInstallerProfile::V1(profile) => profile.install.minecraft.clone(),
            ForgeInstallerProfile::V2(profile) => profile.minecraft.clone(),
        }
    }

    // pub fn json_filename(&self) -> Option<String> {
    //     if let ForgeInstallerProfile::V2(profile) = self {
    //         Some(Path::new(&profile.json).file_name()?.to_str()?.to_string())
    //     } else {
    //         None
    //     }
    // }

    pub fn get_version_json(
        &self,
        archive: &mut ZipArchive<impl Read + Seek>,
    ) -> Result<ForgeVersionInfo, std::io::Error> {
        match self {
            ForgeInstallerProfile::V1(profile) => Ok(profile.version_info.clone()),
            ForgeInstallerProfile::V2(profile) => {
                let path = Path::new(&profile.json)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string();

                archive
                    .by_name(&path)
                    .map_err(|err| <ZipError as Into<std::io::Error>>::into(err))
                    .and_then(|file| serde_json::from_reader(file).map_err(Into::into))
            }
        }

        // if let Some(filename) = self.json_filename() {
        //     archive.by_name(&filename).ok()
        // } else {
        //     self.
        // }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForgeVersionInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub release_type: String,

    pub time: DateTime<Utc>,
    pub release_time: DateTime<Utc>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherits_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    pub main_class: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub libraries: Vec<ForgeVersionLibrary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jar: Option<String>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub minecraft_arguments: String,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub arguments: HashMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ForgeVersionLibrary {
    Mojang(MojangLibrary),
    Forge(ForgeLibrary),
}

impl ForgeVersionLibrary {
    pub fn to_forge(&self) -> Option<&ForgeLibrary> {
        if let ForgeVersionLibrary::Forge(forge) = &self {
            Some(forge)
        } else {
            None
        }
    }

    pub fn to_mojang(&self) -> Option<&MojangLibrary> {
        if let ForgeVersionLibrary::Mojang(mojang) = &self {
            Some(mojang)
        } else {
            None
        }
    }

    pub fn to_forge_slim(self) -> Option<ForgeVersionLibrary> {
        if let ForgeVersionLibrary::Forge(forge) = self {
            Some(ForgeVersionLibrary::Forge(ForgeLibrary {
                name: forge.name,
                url: forge.url,
                clientreq: None,
                serverreq: None,
                checksums: vec![],
                comment: String::new(),
                enabled: true,
            }))
        } else {
            None
        }
    }
}

// #[derive(Debug, Serialize, Deserialize)]
// #[serde(untagged)]
// pub enum ForgeVersionFile {
//     V1(v1::ForgeVersionInfoV1),
//     V2(v2::ForgeVersionFileV2),
// }

// impl ForgeVersionFile {
//     pub fn get_libraries(&self) -> Vec<&MojangLibrary> {
//         match self {
//             ForgeVersionFile::V1(version_file) => todo!(),
//             ForgeVersionFile::V2(version_file) => version_file.libraries.iter().collect(),
//         }
//     }
// }
