use std::{
    borrow::BorrowMut,
    error::Error,
    fs::{self, create_dir_all, File},
    io::{self, ErrorKind, Write},
    ops::Deref,
    path::PathBuf,
    sync::Arc,
};

use forge_downloader::{get_vanilla_version, Artifact};
use reqwest::Client;
use thiserror::Error;
use zip::{result::ZipError, write::FileOptions, ZipArchive, ZipWriter};

use crate::{
    download_utils::{self, download_library},
    forge_installer_profile::{
        v1::{ForgeLibrary, ForgeOptional},
        v2::MojangLibrary,
        ForgeInstallerProfile, ForgeVersionInfo, ForgeVersionLibrary,
    },
    post_processors::PostProcessors,
};

#[derive(Debug, Error)]
pub enum ForgeInstallError {
    #[error("ForgeInstallError: {0}")]
    Other(String),
}

#[macro_export]
macro_rules! forge_err {
    ($($arg:tt)*) => {
        ForgeInstallError::Other(format!($($arg)*))
    };
}

pub struct ForgeClientInstall {
    installer_path: PathBuf,
    profile: Arc<ForgeInstallerProfile>,
    processors: Option<PostProcessors>,
    version: ForgeVersionInfo,
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
        // let version: ForgeVersionFile = archive
        //     .by_name(&profile.json_filename())
        //     .map_err(Into::into)
        //     .and_then(|reader| serde_json::from_reader(reader).map_err(Into::into))
        //     .map_err(|err: std::io::Error| {
        //         forge_err!(
        //             "Error reading zip file {}: {}",
        //             &profile.json_filename(),
        //             err
        //         )
        //     })?;
        let version = profile.get_version_json(&mut archive)?;

        let profile = Arc::new(profile);
        let mut client_install = Self {
            installer_path,
            profile: Arc::clone(&profile),
            processors: None,
            version,
            archive,
            grabbed: vec![],
        };
        if let ForgeInstallerProfile::V2(_) = *profile {
            client_install.processors = Some(PostProcessors::new(Arc::clone(&profile), true));
        }
        // client_install.processors.set_profile(&profile);
        Ok(client_install)
    }

    pub async fn install_forge(
        &mut self,
        mc_dir: &PathBuf,
        /* installer */ optionals: fn(&str) -> bool,
    ) -> Result<(), Box<dyn Error>> {
        create_dir_all(&mc_dir)?;

        let versions_root_dir = mc_dir.join("versions");
        create_dir_all(&versions_root_dir)?;
        let libraries_root_dir = mc_dir.join("libraries");
        create_dir_all(&libraries_root_dir)?;

        // Check install_version version
        let version_dir = versions_root_dir.join(&self.profile.get_version_id());
        if create_dir_all(&version_dir).is_err() && !version_dir.is_dir() {
            if fs::remove_dir_all(&version_dir).is_err() {
                Err(forge_err!(
                    "Failed to clear version folder. You will need to clear {} manually.",
                    version_dir.display()
                ))?;
            } else {
                create_dir_all(&version_dir)?;
            }
        }
        let version_json = version_dir.join(format!("{}.json", &self.profile.get_version_id()));

        match self.profile.deref().borrow_mut() {
            ForgeInstallerProfile::V1(profile) => {
                println!("Profile manifest version: v1");

                let mut profile = profile.clone();
                // println!("📦 Extracting version.json from installer_profile.json...");
                let libraries = profile.get_libraries("clientreq", optionals);
                let minecraft_jar_file =
                    self.download_vanilla_client_jar(&versions_root_dir).await?;
                if !profile.is_inherited_json() {
                    let client_jar_file =
                        version_dir.join(format!("{}.jar", &self.profile.get_version_id()));
                    if profile
                        .install
                        .strip_meta
                        .is_some_and(|strip_meta| strip_meta)
                    {
                        println!("Copying and filtering minecraft client jar");
                        self.copy_and_strip(&minecraft_jar_file, &client_jar_file)?;
                    } else {
                        println!("Copying minecraft client jar");
                        fs::copy(minecraft_jar_file, client_jar_file)?;
                    }
                }
                let target_library_file = profile.install.path.get_local_path(&libraries_root_dir);
                self.grabbed = vec![];
                let mut bad = vec![];
                download_utils::download_installed_libraries(
                    true,
                    &libraries_root_dir,
                    &libraries,
                    &mut self.grabbed,
                    &mut bad,
                    &mut self.archive,
                )
                .await?;
                if bad.len() > 0 {
                    let list = bad
                        .iter()
                        .map(|a| a.get_descriptor())
                        .collect::<Vec<_>>()
                        .join("\n");
                    Err(forge_err!(
                        "These libraries failed to download. Try again.\n{list}"
                    ))?
                }
                // TODO:
                // if (!targetLibraryFile.getParentFile().mkdirs() && !targetLibraryFile.getParentFile().isDirectory()) {
                //     if (!targetLibraryFile.getParentFile().delete()) {
                //       JOptionPane.showMessageDialog(null, "There was a problem with the launcher version data. You will need to clear " + targetLibraryFile.getAbsolutePath() + " manually", "Error", 0);
                //       return false;
                //     }
                //     targetLibraryFile.getParentFile().mkdirs();
                //   }
                create_dir_all(target_library_file.parent().unwrap())?;
                let mod_list_type = &profile.install.mod_list;
                let mut mod_list_file = mc_dir.join("mods").join("mod_list.json");
                match mod_list_type.as_str() {
                    "absolute" => {
                        mod_list_file = version_dir.join("mod_list.json");
                        profile.version_info.minecraft_arguments.push_str(&format!(
                            " --modListFile \"absolute: {}\"",
                            mod_list_file.canonicalize().unwrap().to_str().unwrap()
                        ));
                    }
                    "none" => { /* Do nothing*/ }
                    _ => {
                        if ForgeOptional::save_mod_list_json(
                            &libraries_root_dir,
                            &mod_list_file,
                            &profile.optionals,
                            optionals,
                        )
                        .is_err()
                        {
                            Err(forge_err!(
                                "Failed to write mod_list.json, optional mods may not be loaded."
                            ))?
                        }
                    }
                }
                let version_json_file =
                    version_dir.join(format!("{}.json", &self.profile.get_version_id()));
                // let mut output = profile.version_info.clone();
                let mut lst = vec![];
                for opt in &profile.optionals {
                    if optionals(&opt.artifact.get_descriptor()) && opt.inject {
                        lst.push(ForgeVersionLibrary::Forge(ForgeLibrary {
                            name: opt.artifact.clone(),
                            url: Some(opt.maven.clone()),
                            serverreq: None,
                            clientreq: None,
                            checksums: vec![],
                            comment: String::new(),
                            enabled: true,
                        }));
                    }
                }

                let mut output = self.version.clone();
                output
                    .libraries
                    .into_iter()
                    .filter_map(|lib| lib.to_forge_slim())
                    .for_each(|lib| lst.push(lib));
                output.libraries = lst;
                println!("Writing to {}", version_json_file.display());
                serde_json::to_writer_pretty(File::create(&version_json_file)?, &output)?;

                // Extract file
                let contained_file = &mut self.archive.by_name(&profile.install.file_path)?;
                io::copy(contained_file, &mut File::create(target_library_file)?)?;
            }
            ForgeInstallerProfile::V2(profile) => {
                println!("Profile manifest version: v2");
                println!("📦 Extracting version.json...");

                let mut file = File::create(version_json)?;
                let bytes = &serde_json::to_vec_pretty(&self.version)?[..];
                file.write_all(bytes);

                println!("✅ {} bytes were extracted!", bytes.len());

                //
                /*println!("☕ Considering minecraft client jar...");
                let version_vanilla = versions_root_dir.join(&profile.minecraft);
                if create_dir_all(&version_vanilla).is_err() && !version_vanilla.is_dir() {
                    if fs::remove_dir(&version_vanilla).is_err() {
                        Err(forge_err!(
                            "There was a problem with the launcher version data. You will need to clear {} manually.",
                            version_vanilla.display()
                        ))?;
                    }
                    let _ = create_dir_all(&version_vanilla);
                }*/

                let client_target = self.download_vanilla_client_jar(&versions_root_dir).await?;
                //version_vanilla.join(format!("{}.jar", &profile.minecraft));
                // if !client_target.is_file() {
                //     let version_json = version_vanilla.join(format!("{}.json", &profile.minecraft));
                //     let vanilla = get_vanilla_version(&profile.minecraft, &version_json).await;
                //     if vanilla.is_none() {
                //         return Err(Box::new(std::io::Error::new(
                //             ErrorKind::Other,
                //             "Failed to download version manifest, can not find client jar URL.",
                //         )));
                //     }
                //     let vanilla = vanilla.unwrap();
                //     let client = &vanilla["downloads"].get("client");
                //     if client.is_none() {
                //         return Err(
                //             Box::new(
                //                 std::io::Error::new(
                //                     ErrorKind::Other,
                //                     format!(
                //                         "Failed to download minecraft client, info missing from manifest: {}",
                //                         version_json.display()
                //                     )
                //                 )
                //             )
                //         );
                //     }
                //     let client = client.unwrap()["url"].as_str().unwrap();

                //     // TODO: get mirror?
                //     let bytes = Client::new().get(client).send().await?.bytes().await?;
                //     // TODO: check sha1
                //     // "Downloading minecraft client failed, invalid checksum.\nTry again, or use the vanilla launcher to install the vanilla version."
                //     fs::write(&client_target, bytes)?;
                // }

                if let Err(err) = self
                    .download_libraries(&libraries_root_dir, optionals, vec![])
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
                    .process(
                        &libraries_root_dir,
                        &client_target,
                        &mc_dir,
                        &mut self.archive,
                    )
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
        println!(
            "Successfully installed version {} and grabbed {} required libraries",
            self.profile.get_version_id(),
            self.grabbed.len()
        );
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
        let mut libraries = vec![];
        libraries.extend(&self.version.libraries.iter().collect::<Vec<_>>()); // Download version libraries
        libraries.extend(self.processors.as_ref().unwrap().get_libraries()); // Download profile libraries
        let mut output = String::new();
        let steps = libraries.len();
        let mut progress = 1;
        for lib in libraries {
            if let ForgeVersionLibrary::Mojang(lib) = lib {
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
                    let download = lib.downloads.artifact.as_ref();
                    // .as_ref()
                    // .and_then(|downloads| downloads.artifact.as_ref());
                    if let Some(download) = download {
                        if download.url.as_ref().is_some_and(|url| !url.is_empty()) {
                            output.push_str(&format!("\n{}", lib.name.get_descriptor()));
                        }
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

    pub async fn download_vanilla_client_jar(
        &self,
        versions_root: &PathBuf,
    ) -> Result<PathBuf, Box<dyn Error>> {
        println!("☕ Considering minecraft client jar...");
        let version_vanilla = versions_root.join(self.profile.get_minecraft());
        if fs::create_dir_all(&version_vanilla).is_err() && !version_vanilla.is_dir() {
            if fs::remove_dir(&version_vanilla).is_err() {
                Err(forge_err!("There was a problem with the launcher version data. You will need to clear {} manually.", version_vanilla.display()))?;
            }
            fs::create_dir_all(&version_vanilla)?;
        }
        let client_target = version_vanilla.join(format!("{}.jar", self.profile.get_minecraft()));
        if !client_target.is_file() {
            let version_json =
                version_vanilla.join(format!("{}.json", &self.profile.get_minecraft()));
            let vanilla = get_vanilla_version(&self.profile.get_minecraft(), &version_json).await;
            if vanilla.is_none() {
                Err(forge_err!(
                    "Failed to download version manifest, can not find client jar URL."
                ))?;
            }
            let vanilla = vanilla.unwrap();
            let client = &vanilla["downloads"].get("client");
            if client.is_none() {
                Err(forge_err!(
                    "Failed to download minecraft client, info missing from manifest: {}",
                    version_json.display()
                ))?;
            }
            let client = client.unwrap()["url"].as_str().unwrap();

            // TODO: get mirror?
            let bytes = Client::new().get(client).send().await?.bytes().await?;
            // TODO: check sha1
            // "Downloading minecraft client failed, invalid checksum.\nTry again, or use the vanilla launcher to install the vanilla version."
            fs::write(&client_target, bytes)?;
        }
        Ok(client_target)
    }

    fn copy_and_strip(
        &self,
        source_jar: &PathBuf,
        target_jar: &PathBuf,
    ) -> Result<(), Box<dyn Error>> {
        let mut zip_in = ZipArchive::new(File::open(source_jar)?)?;
        let mut zip_out = ZipWriter::new(File::create(target_jar)?);
        for i in 0..zip_in.len() {
            let mut file = zip_in.by_index(i)?;
            if file.name().starts_with("META-INF") {
                continue;
            }
            zip_out.start_file(
                file.name(),
                FileOptions::default().last_modified_time(file.last_modified()),
            )?;
            io::copy(&mut file, &mut zip_out)?;
        }
        zip_out.finish()?;
        Ok(())
    }
}
