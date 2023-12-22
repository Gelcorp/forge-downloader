mod download_utils;
#[macro_use]
mod forge_client_install;
mod forge_installer_profile;
mod post_processors;

use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{self, create_dir_all}, path::Path,
};

use forge_client_install::ForgeClientInstall;
use forge_downloader::Artifact;
use reqwest::Client;
use serde_json::Value;

const PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const METADATA_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";

pub async fn list_forge_versions() -> Result<HashMap<String, Vec<String>>, Box<dyn Error>> {
    let response = Client::new()
        .get(METADATA_URL)
        .send()
        .await?
        .json::<HashMap<String, Vec<String>>>()
        .await?;
    Ok(response)
}

pub async fn get_promoted_versions() -> Result<HashMap<String, String>, Box<dyn Error>> {
    let result: Value = Client::new()
        .get(PROMOTIONS_URL)
        .send()
        .await?
        .json()
        .await?;

    let mut promos = HashMap::new();
    for (mc_version, forge_version) in result["promos"].as_object().unwrap() {
        let forge_version = forge_version.as_str().unwrap().to_string();
        promos.insert(mc_version.clone(), forge_version);
    }
    Ok(promos)
}

pub async fn get_recommended_versions() -> Result<Vec<String>, Box<dyn Error>> {
    let forge_version_names = list_forge_versions().await?;
    let promos = get_promoted_versions().await?;

    let mut map = HashMap::new();
    for (key, forge_version) in &promos {
        let (mc_version, release_type) = key.split_once("-").unwrap();
        if release_type == "latest" && map.contains_key(mc_version) {
            continue;
        }
        let forge_version = forge_version_names[&mc_version.to_string()]
            .iter()
            .find(|full_forge_version| full_forge_version.contains(forge_version))
            .unwrap();
        map.insert(mc_version, forge_version.clone());
    }
    let mut versions: Vec<(&str, String)> = map.into_iter().collect();
    versions.sort_by_key(|(mc_ver, _)| {
        let parts: Vec<&str> = mc_ver.split(".").collect();
        let major = parts[0].parse::<u8>().unwrap();
        let minor = parts[1].parse::<u8>().unwrap();
        let patch = parts.get(2).unwrap_or(&"0").to_string();
        (major, minor, patch)
    });
    let versions = versions.into_iter().map(|(_, v)| v).collect();
    Ok(versions)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let versions = get_promoted_versions().await?;
    let forge_version = versions.get("1.20.4-latest").unwrap();
    let artifact = Artifact::try_from(format!(
        "net.minecraftforge:forge:1.20.4-{forge_version}:installer"
    ))?;
    // let artifact = Artifact::try_from(format!(
    //     "net.minecraftforge:forge:1.11.2-13.20.1.2588:installer"
    // ))?;
    let url = format!(
        "https://maven.minecraftforge.net/{}",
        artifact.get_path_string()
    );
    println!("Url: {}", url);

    let response = Client::new().get(&url).send().await?;
    if !response.status().is_success() {
        println!("❌ Couldn't download: {}", response.status());
        return Ok(());
    }
    let bytes = response.bytes().await?;
    let game_dir = Path::new(env!("APPDATA")).join(".minecraft");
    // env::temp_dir().join("temporalmc");
    create_dir_all(&game_dir)?;
    fs::write(&env::temp_dir().join("forge-installer.jar"), bytes)?;

    /*
       TODO: add java path configuration and verify java installation on constructor
       TODO: clean up the code
       TODO: refactor serde stuff
       TODO: add monitor struct to manage logs and stuff, see how
    */
    let mut installer = ForgeClientInstall::new(env::temp_dir().join("forge-installer.jar"))?;
    installer.install_forge(&game_dir, |_| true).await?;
    Ok(())
}

#[allow(unused_imports)]
mod tests {
    use std::{
        fs::{self, File},
        io::{Cursor, Read, Write},
        path::PathBuf,
    };

    use futures::future::join_all;
    use zip::ZipArchive;

    use crate::forge_installer_profile::{
        v1::ForgeInstallerProfileV1, v2::ForgeInstallerProfileV2, ForgeInstallerProfile,
    };

    use super::*;

    #[tokio::test]
    async fn test_parser() -> Result<(), Box<dyn std::error::Error>> {
        let cache_folder = std::env::temp_dir().join("forge_cache_versions");
        fs::create_dir_all(&cache_folder)?;

        let recommended_versions = get_recommended_versions().await?;

        println!(
            "Recommended versions: {:?}",
            recommended_versions.join(", ")
        );
        let forge_versions = recommended_versions.chunks(3).collect::<Vec<_>>();
        for versions in forge_versions {
            let futures = versions
                .into_iter()
                .map(|ver| process_version(&ver, &cache_folder))
                .collect::<Vec<_>>();
            join_all(futures).await;
        }
        // for full_forge_version in &recommended_versions {

        // }

        Ok(())
    }

    async fn process_version(
        full_forge_version: &str,
        cache_folder: &PathBuf,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let artifact = Artifact::try_from(format!(
            "net.minecraftforge:forge:{full_forge_version}:installer"
        ))?;

        let path = &cache_folder.join(artifact.get_file());
        let mut zip_archive = if path.is_file() {
            print!("\n💾 Loading {} from cache", full_forge_version);
            ZipArchive::new(Cursor::new(fs::read(path)?))?
        } else {
            let url = format!(
                "https://maven.minecraftforge.net/{}",
                artifact.get_path_string()
            );

            let response = Client::new().get(&url).send().await?;
            if !response.status().is_success() {
                println!(
                    "❌ Couldn't download {}: {}",
                    full_forge_version,
                    response.status()
                );
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
