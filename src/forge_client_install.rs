use std::{
    collections::HashMap,
    error::Error,
    fs::{self, create_dir_all, File},
    io::{self, ErrorKind, Read, Seek},
    ops::Deref,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use forge_downloader::Artifact;
use reqwest::Client;
use zip::ZipArchive;

use crate::{
    download_utils::download_library,
    forge_installer_profile::{
        v2::{ForgeVersionFileV2, MojangLibrary, Processor},
        ForgeInstallerProfile, ForgeVersionFile,
    },
    lib::get_vanilla_version,
    post_processors::PostProcessors,
};

pub struct ForgeClientInstall {
    installer_path: PathBuf,
    profile: Arc<ForgeInstallerProfile>,
    processors: Option<PostProcessors>,
    version: ForgeVersionFile,
    archive: ZipArchive<File>,
    grabbed: Vec<Artifact>,
}

impl ForgeClientInstall {
    pub fn new(installer_path: PathBuf) -> Result<Self, Box<dyn Error>> {
        let installer_reader = File::open(&installer_path)?;
        let mut archive = ZipArchive::new(installer_reader)?;
        /*let profile: ForgeInstallerProfile = serde_json::from_reader(
            archive.by_name("install_profile.json")?
        )?;*/
        let profile = ForgeInstallerProfile::from_reader(archive.by_name("install_profile.json")?);
        println!("Profile {:#?}", profile);
        let version: ForgeVersionFile =
            serde_json::from_reader(archive.by_name(&profile.json_filename())?)?;

        let profile = Arc::new(profile);
        let mut client_install = Self {
            installer_path,
            profile: Arc::clone(&profile),
            processors: None,
            version,
            archive,
            grabbed: vec![],
        };
        client_install.processors = Some(PostProcessors::new(Arc::clone(&profile), true));
        // client_install.processors.set_profile(&profile);
        Ok(client_install)
    }

    pub async fn install_forge(
        &mut self,
        mc_dir: &PathBuf,
        /* installer */ optionals: fn(&str) -> bool,
    ) -> Result<(), Box<dyn Error>> {
        create_dir_all(&mc_dir)?;

        let versions_dir = mc_dir.join("versions");
        create_dir_all(&versions_dir)?;
        let libraries_dir = mc_dir.join("libraries");
        create_dir_all(&libraries_dir)?;

        // Check install_version version
        match &self.profile.deref() {
            ForgeInstallerProfile::V1(installer_profile) => {
                todo!();
            }
            ForgeInstallerProfile::V2(profile) => {
                println!("📦 Extracting version.json...");

                let version_json = versions_dir.join(&profile.version);
                create_dir_all(&version_json)?;
                let version_json = version_json.join(format!("{}.json", &profile.version));
                let mut file = File::create(version_json)?;
                let bytes = io::copy(
                    &mut self.archive.by_name(&self.profile.json_filename())?,
                    &mut file,
                )?;
                println!("✅ {bytes} bytes were extracted!");

                //
                println!("☕ Considering minecraft client jar...");
                let version_vanilla = versions_dir.join(&profile.minecraft);
                if create_dir_all(&version_vanilla).is_err() && !version_vanilla.is_dir() {
                    if fs::remove_dir(&version_vanilla).is_err() {
                        return Err(
                            Box::new(
                                std::io::Error::new(
                                    ErrorKind::Other,
                                    format!(
                                        "There was a problem with the launcher version data. You will need to clear {} manually.",
                                        version_vanilla.display()
                                    )
                                )
                            )
                        );
                    }
                    let _ = create_dir_all(&version_vanilla);
                }

                let client_target = version_vanilla.join(format!("{}.jar", &profile.minecraft));
                if !client_target.is_file() {
                    let version_json = version_vanilla.join(format!("{}.json", &profile.minecraft));
                    let vanilla = get_vanilla_version(&profile.minecraft, &version_json).await;
                    if vanilla.is_none() {
                        return Err(Box::new(std::io::Error::new(
                            ErrorKind::Other,
                            "Failed to download version manifest, can not find client jar URL.",
                        )));
                    }
                    let vanilla = vanilla.unwrap();
                    let client = &vanilla["downloads"].get("client");
                    if client.is_none() {
                        return Err(
                            Box::new(
                                std::io::Error::new(
                                    ErrorKind::Other,
                                    format!(
                                        "Failed to download minecraft client, info missing from manifest: {}",
                                        version_json.display()
                                    )
                                )
                            )
                        );
                    }
                    let client = client.unwrap()["url"].as_str().unwrap();

                    // TODO: get mirror?
                    let bytes = Client::new().get(client).send().await?.bytes().await?;
                    // TODO: check sha1
                    // "Downloading minecraft client failed, invalid checksum.\nTry again, or use the vanilla launcher to install the vanilla version."
                    fs::write(&client_target, bytes)?;
                }

                if let Err(err) = self
                    .download_libraries(&libraries_dir, optionals, vec![])
                    .await
                {
                    println!("{err}");
                    return Err(Box::new(std::io::Error::new(
                        ErrorKind::Other,
                        "Could not download libraries.",
                    )));
                }

                let processors = self.processors.as_mut().unwrap();
                if let Err(err) = processors
                    .process(&libraries_dir, &client_target, &mc_dir, &mut self.archive)
                    .await
                {
                    println!("{err}");
                    return Err(Box::new(std::io::Error::new(
                        ErrorKind::Other,
                        "Could not process libraries.",
                    )));
                }
            }
        }
        Ok(())
    }

    async fn download_libraries(
        &mut self,
        libraries_dir: &PathBuf,
        optionals: fn(&str) -> bool,
        additional_lib_dirs: Vec<&PathBuf>,
    ) -> Result<(), Box<dyn Error>> {
        println!("🗃️  Downloading libraries...");
        println!(
            "Found {} additional library directories",
            additional_lib_dirs.len()
        );
        let mut libraries: Vec<&MojangLibrary> = vec![];
        libraries.extend(&self.version.get_libraries()); // Download version libraries
        libraries.extend(&self.processors.as_ref().unwrap().get_libraries()); // Download profile libraries
        let mut output = String::new();
        let steps = libraries.len();
        let mut progress = 1;
        for lib in libraries {
            println!("Downloading library {progress}/{steps}...");
            progress += 1;
            if download_library(
                &mut self.archive,
                lib,
                libraries_dir,
                optionals,
                &mut self.grabbed,
                &additional_lib_dirs,
            )
            .await
            .is_err()
            {
                let download = lib
                    .downloads
                    .as_ref()
                    .and_then(|downloads| downloads.artifact.as_ref());
                if let Some(download) = download {
                    if download.url.as_ref().is_some_and(|url| !url.is_empty()) {
                        output.push_str(&format!("\n{}", lib.name.get_descriptor()));
                    }
                }
            }
        }

        if !output.is_empty() {
            Err(Box::new(std::io::Error::new(
                ErrorKind::Other,
                format!("These libraries failed to download. Try again.\n{}", output),
            )))
        } else {
            Ok(())
        }
    }
}
