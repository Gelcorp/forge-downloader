pub mod forge;

use std::{ error::Error, fs::{ self, create_dir_all, File }, io::{ self, ErrorKind, Read, Seek, Write }, path::PathBuf };

use futures::StreamExt;
use log::{info, warn, error, debug};
use reqwest::{ Client, Url };
use sha1::{ Digest, Sha1 };
use zip::{ result::ZipError, ZipArchive };

use crate::{
  forge_client_install::ForgeInstallError,
  forge_err,
  forge_installer_profile::{ v1::ForgeLibrary, v2::{ MojangArtifact, MojangLibrary } },
  Artifact,
  Sha1Sum,
};

// V2 download
pub async fn download_library(
  zip_archive: &mut ZipArchive<impl Read + Seek>,
  library: &MojangLibrary,
  root: &PathBuf,
  optional: fn(&str) -> bool,
  grabbed: &mut Vec<Artifact>,
  additional_library_dirs: &Vec<&PathBuf>
) -> Result<(), Box<dyn Error>> {
  let artifact = &library.name;
  let target = artifact.get_local_path(root);

  let download = library.downloads.artifact.as_ref().cloned().unwrap_or(MojangArtifact::new(artifact.get_path_string()));

  let artifact_str: String = library.name.get_descriptor();
  if !optional(&artifact_str) {
    info!("Considering library {artifact_str}: Not downloading {{Disabled}}");
    return Ok(());
  }
  info!("Considering library {artifact_str}");
  if target.is_file() {
    if let Some(lib_sha1) = &download.sha1 {
      let target_sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
      if lib_sha1 == &target_sha1 {
        info!("  File exists: Checksum validated.");
        return Ok(());
      }
      info!("  File exists: Checksum invalid, deleting file:");
      info!("    Expected: {lib_sha1}");
      info!("    Found:    {target_sha1}");
      if let Err(err) = fs::remove_file(&target) {
        return Err(Box::new(io::Error::new(ErrorKind::Other, format!("Failed to delete file, aborting. {}", err))));
      }
    } else {
      info!("  File exists: No checksum, Assuming valid.");
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
        info!("  Found artifact in local folder {}", lib_dir.to_str().unwrap());
        let sha1 = Sha1Sum::from_reader(&mut File::open(&in_lib_dir)?)?;
        if provided_sha1 == &sha1 {
          info!("    Checksum validated");
        } else {
          info!("    Invalid checksum. Not using.");
          continue;
        }
        if let Err(err) = fs::copy(in_lib_dir, &target) {
          warn!("    Failed to copy from local folder: {err}");
          if target.exists() && fs::remove_file(&target).is_err() {
            error!("Failed to delete failed copy, aborting.");
            return Err(Box::new(io::Error::new(ErrorKind::Other, "Failed to delete failed copy, aborting.")));
          }
        } else {
          info!("    Successfully copied local file");
          grabbed.push(artifact.clone());
          return Ok(());
        }
      }
    }
  }
  let url = download.url.as_ref();
  if url.is_none() || url.unwrap().is_empty() {
    return Err(Box::new(io::Error::new(ErrorKind::Other, "Invalid library, missing url")));
  }
  if let Err(err) = download_lib(/* mirror */ &download, &target).await {
    Err(Box::new(io::Error::new(ErrorKind::Other, format!("Failed to download library: {err}"))))
  } else {
    grabbed.push(artifact.clone());
    Ok(())
  }
}

fn try_to_extract_artifact(
  zip_archive: &mut ZipArchive<impl Read + Seek>,
  artifact: &Artifact,
  download: &MojangArtifact,
  grabbed: &mut Vec<Artifact>,
  target: &PathBuf
) -> Result<Option<()>, Box<dyn Error>> {
  let path = format!("maven/{}", artifact.get_path_string());
  if let Ok(mut input) = zip_archive.by_name(&path) {
    info!("  Extracting library from /{path}");
    io::copy(&mut input, &mut File::create(&target)?)?;
    if let Some(lib_sha1) = download.sha1.as_ref() {
      let target_sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
      if lib_sha1 == &target_sha1 {
        info!("  File exists: Checksum validated.");
        return Ok(Some(()));
      }
      info!("  File exists: Checksum invalid, deleting file:");
      info!("    Expected: {lib_sha1}");
      info!("    Found:    {target_sha1}");
      if let Err(err) = fs::remove_file(&target) {
        error!("Failed to delete file, aborting. {}", err);
        return Err(Box::new(io::Error::new(ErrorKind::Other, format!("Failed to delete file, aborting. {}", err))));
      }
    }
    info!("  File exists: No checksum, Assuming valid.");
    grabbed.push(artifact.clone());
    Ok(Some(()))
  } else {
    Ok(None)
  }
}

async fn download_lib(/* mirror */ download: &MojangArtifact, target: &PathBuf) -> Result<(), Box<dyn Error>> {
  let url = download.url.as_ref().unwrap();
  info!("  Downloading library from {url}");
  let bytes = Client::new().get(url).send().await?.bytes().await?;
  fs::write(&target, bytes)?;
  if let Some(sha1_lib) = &download.sha1 {
    let sha1 = Sha1Sum::from_reader(&mut File::open(&target)?)?;
    if sha1_lib == &sha1 {
      info!("    Download completed: Checksum validated.");
      return Ok(());
    }
    info!("    Download failed: Checksum invalid, deleting file:");
    info!("      Expected: {sha1_lib}");
    info!("      Actual:   {sha1}");
    if fs::remove_file(&target).is_err() {
      error!("Failed to delete file, aborting.");
      return Err(Box::new(io::Error::new(ErrorKind::Other, "Failed to delete file, aborting.")));
    }
  }
  Ok(())
}

// V1

pub fn extract_file<T: Read + Seek>(name: &str, target: &PathBuf, zip_archive: &mut ZipArchive<T>) -> Result<(), Box<dyn Error>> {
  let path = if name.starts_with("/") { &name[1..] } else { name };

  let input = zip_archive.by_name(&path);
  if let Err(err) = input {
    match err {
      ZipError::FileNotFound => { Err(forge_err!("File not found in installer archive: {}", path))? }
      _ => Err(err)?,
    }
  } else {
    create_dir_all(target.parent().unwrap())?;
    io::copy(&mut input?, &mut File::create(&target)?)?;
    Ok(())
  }
}

pub async fn download_installed_libraries(
  is_client: bool,
  libraries_dir: &PathBuf,
  libraries: &Vec<ForgeLibrary>,
  grabbed: &mut Vec<Artifact>,
  bad: &mut Vec<Artifact>,
  archive: &mut ZipArchive<impl Read + Seek>
) -> Result<i32, Box<dyn Error>> {
  let mut progress = 1;
  for library in libraries {
    let artifact = &library.name;
    let checksums = &library.checksums;
    if library.is_side(if is_client { "clientreq" } else { "serverreq" }) && library.enabled {
      info!("📚 Considering library {} ({}/{})", artifact.get_descriptor(), progress, libraries.len());
      let lib_path = artifact.get_local_path(&libraries_dir);
      let checksum = Sha1Sum::from_reader(&mut File::open(&lib_path)?)?;
      if lib_path.exists() && !checksums.is_empty() && checksums.contains(&checksum) {
        progress += 1;
        continue;
      }
      create_dir_all(&lib_path.parent().unwrap())?;
      info!("  Downloading library {}", artifact.get_descriptor());
      let mut lib_url = Url::parse(&library.get_url())?;
      lib_url.set_path(&artifact.get_path_string());
      let lib_url = lib_url.as_str().to_string();
      info!("  Trying unpacked library {}", artifact.get_descriptor());

      let download_file_result = download_file(&lib_path, &lib_url, &checksums).await;
      let extract_file_result = extract_file(&artifact.get_path_string(), &lib_path, archive);
      if download_file_result.is_err() && extract_file_result.is_err() {
        if !lib_url.starts_with("https://libraries.minecraft.net/") || !is_client {
          debug!("Download file error: {}", download_file_result.unwrap_err());
          debug!("Extract file error: {}", extract_file_result.unwrap_err());
          bad.push(artifact.clone());
        } else {
          warn!("  ❌ Unmirrored file failed, Mojang launcher should download at next run, non fatal");
        }
      } else {
        grabbed.push(artifact.clone());
      }
    } else if library.is_side(if is_client { "clientreq" } else { "serverreq" }) {
      warn!("❌ Considering library {}: Not Downloading {}", artifact.get_descriptor(), "{Disabled}");
    } else {
      warn!("❌ Considering library {}: Not downloading {}", artifact.get_descriptor(), "{Wrong Side}");
    }
    progress += 1;
  }

  Ok(progress)
}

pub async fn download_file(lib_path: &PathBuf, lib_url: &str, checksums: &Vec<Sha1Sum>) -> Result<(), Box<dyn Error>> {
  let response = Client::new().get(lib_url).send().await?;
  if !response.status().is_success() {
    Err(forge_err!("Failed to download file: {}. Status: {}", lib_url, response.status().as_u16()))?;
  }
  let mut stream = response.bytes_stream();
  create_dir_all(lib_path.parent().unwrap())?;

  let mut sha1_hasher = Sha1::new();
  let mut writer = File::create(&lib_path)?;
  while let Some(item) = stream.next().await {
    let chunk = item?;
    sha1_hasher.update(&chunk);
    writer.write_all(&chunk)?;
  }
  let sum = Sha1Sum::new(sha1_hasher.finalize().into());
  if !checksums.is_empty() && !checksums.contains(&sum) {
    Err(forge_err!("Checksum failed: Actual: {sum} Expected: {checksums:?}"))?;
  }
  Ok(())
}
