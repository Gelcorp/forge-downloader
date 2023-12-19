use std::{ collections::HashMap, path::Path, io::{ Read, Seek } };

use serde::{ Deserialize, Serialize };

use self::{
    v2::{ Processor, MojangLibrary, ForgeInstallerProfileV2 },
    v1::ForgeInstallerProfileV1,
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
        let result = serde_json
            ::from_slice::<ForgeInstallerProfileV1>(bytes.as_slice())
            .map(|v| ForgeInstallerProfile::V1(v));
        let result2 = serde_json
            ::from_slice::<ForgeInstallerProfileV2>(bytes.as_slice())
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

    pub fn get_libraries(&self) -> Vec<&MojangLibrary> {
        match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => profile.libraries.iter().collect(),
        }
    }

    pub fn get_minecraft(&self) -> String {
        match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => profile.minecraft.clone(),
        }
    }

    pub fn json_filename(&self) -> String {
        let json = match self {
            ForgeInstallerProfile::V1(profile) => todo!(),
            ForgeInstallerProfile::V2(profile) => &profile.json,
        };
        Path::new(&json)
            .file_name()
            .map(|name| name.to_str().unwrap())
            .unwrap_or("version.json")
            .to_string()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ForgeVersionFile {
    V1(v1::ForgeVersionFileV1),
    V2(v2::ForgeVersionFileV2),
}

impl ForgeVersionFile {
    pub fn get_libraries(&self) -> Vec<&MojangLibrary> {
        match self {
            ForgeVersionFile::V1(version_file) => version_file.libraries.iter().collect(),
            ForgeVersionFile::V2(version_file) => version_file.libraries.iter().collect(),
        }
    }
}
