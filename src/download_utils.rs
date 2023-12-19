use std::{
    fs::{self, create_dir_all, File},
    io::{self, ErrorKind, Read, Seek},
    path::PathBuf,
};

use forge_downloader::{Artifact, Sha1Sum};
use reqwest::Client;
use zip::{result::ZipError, ZipArchive};

use crate::forge_installer_profile::v2::{MojangArtifact, MojangLibrary};

pub async fn download_library<T: Read + Seek>(
    /* TODO: mirror */
    zip_archive: &mut ZipArchive<T>,
    library: &MojangLibrary,
    root: &PathBuf,
    optional: fn(&str) -> bool,
    grabbed: &mut Vec<Artifact>,
    additional_library_dirs: &Vec<&PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let artifact = &library.name;
    let target = artifact.get_local_path(root);

    let download = library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.artifact.as_ref())
        .cloned()
        .unwrap_or(MojangArtifact::new(artifact.get_path_string()));

    let artifact_str: String = library.name.get_descriptor();
    if !optional(&artifact_str) {
        println!("Considering library {artifact_str}: Not downloading {{Disabled}}");
        return Ok(());
    }
    println!("Considering library {artifact_str}");
    if target.is_file() {
        if let Some(lib_sha1) = &download.sha1 {
            let target_sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
            if lib_sha1 == &target_sha1 {
                println!("  File exists: Checksum validated.");
                return Ok(());
            }
            println!("  File exists: Checksum invalid, deleting file:");
            println!("    Expected: {lib_sha1}");
            println!("    Found:    {target_sha1}");
            if let Err(err) = fs::remove_file(&target) {
                return Err(Box::new(io::Error::new(
                    ErrorKind::Other,
                    format!("Failed to delete file, aborting. {}", err),
                )));
            }
        } else {
            println!("  File exists: No checksum, Assuming valid.");
            return Ok(());
        }
    }
    create_dir_all(&target.parent().unwrap())?;
    if let Some(_) = try_to_extract_artifact(zip_archive, artifact, &download, grabbed, &target)? {
        return Ok(());
    }
    if let Some(ref provided_sha1) = download.sha1 {
        for lib_dir in additional_library_dirs {
            let in_lib_dir = artifact.get_local_path(&lib_dir);
            if in_lib_dir.is_file() {
                println!(
                    "  Found artifact in local folder {}",
                    lib_dir.to_str().unwrap()
                );
                let sha1 = Sha1Sum::from_reader(&mut File::open(&in_lib_dir)?)?;
                if provided_sha1 == &sha1 {
                    println!("    Checksum validated");
                } else {
                    println!("    Invalid checksum. Not using.");
                    continue;
                }
                if let Err(err) = fs::copy(in_lib_dir, &target) {
                    println!("    Failed to copy from local folder: {err}");
                    if target.exists() && fs::remove_file(&target).is_err() {
                        return Err(Box::new(io::Error::new(
                            ErrorKind::Other,
                            "Failed to delete failed copy, aborting.",
                        )));
                    }
                } else {
                    println!("    Successfully copied local file");
                    grabbed.push(artifact.clone());
                    return Ok(());
                }
            }
        }
    }
    let url = download.url.as_ref();
    if url.is_none() || url.unwrap().is_empty() {
        return Err(Box::new(io::Error::new(
            ErrorKind::Other,
            "Invalid library, missing url",
        )));
    }
    if let Err(err) = download_lib(/* mirror */ &download, &target).await {
        Err(Box::new(io::Error::new(
            ErrorKind::Other,
            format!("Failed to download library: {err}"),
        )))
    } else {
        grabbed.push(artifact.clone());
        Ok(())
    }
}

fn try_to_extract_artifact<T: Read + Seek>(
    zip_archive: &mut ZipArchive<T>,
    artifact: &Artifact,
    download: &MojangArtifact,
    grabbed: &mut Vec<Artifact>,
    target: &PathBuf,
) -> Result<Option<()>, Box<dyn std::error::Error>> {
    let path = format!("maven/{}", artifact.get_path_string());
    if let Ok(mut input) = zip_archive.by_name(&path) {
        println!("  Extracting library from /{path}");
        io::copy(&mut input, &mut File::create(&target)?)?;
        if let Some(lib_sha1) = download.sha1.as_ref() {
            let target_sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
            if lib_sha1 == &target_sha1 {
                println!("  File exists: Checksum validated.");
                return Ok(Some(()));
            }
            println!("  File exists: Checksum invalid, deleting file:");
            println!("    Expected: {lib_sha1}");
            println!("    Found:    {target_sha1}");
            if let Err(err) = fs::remove_file(&target) {
                return Err(Box::new(io::Error::new(
                    ErrorKind::Other,
                    format!("Failed to delete file, aborting. {}", err),
                )));
            }
        }
        println!("  File exists: No checksum, Assuming valid.");
        grabbed.push(artifact.clone());
        Ok(Some(()))
    } else {
        Ok(None)
    }
}

async fn download_lib(
    /* mirror */ download: &MojangArtifact,
    target: &PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = download.url.as_ref().unwrap();
    println!("  Downloading library from {url}");
    let bytes = Client::new().get(url).send().await?.bytes().await?;
    fs::write(&target, bytes)?;
    if let Some(sha1_lib) = &download.sha1 {
        let sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
        if sha1_lib == &sha1 {
            println!("    Download completed: Checksum validated.");
            return Ok(());
        }
        println!("    Download failed: Checksum invalid, deleting file:");
        println!("      Expected: {sha1_lib}");
        println!("      Actual:   {sha1}");
        if fs::remove_file(&target).is_err() {
            return Err(Box::new(io::Error::new(
                ErrorKind::Other,
                "Failed to delete file, aborting.",
            )));
        }
    }
    Ok(())
}

pub fn extract_file<T: Read + Seek>(
    name: &str,
    target: &PathBuf,
    zip_archive: &mut ZipArchive<T>,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = if name.starts_with("/") {
        &name[1..]
    } else {
        name
    };

    let input = zip_archive.by_name(&path);
    if let Err(err) = input {
        match err {
            ZipError::FileNotFound => Err(Box::new(io::Error::new(
                ErrorKind::Other,
                format!("File not found in installer archive: {}", path),
            ))),
            _ => Err(Box::new(err)),
        }
    } else {
        create_dir_all(target.parent().unwrap())?;
        io::copy(&mut input?, &mut File::create(&target)?)?;
        Ok(())
    }
}
