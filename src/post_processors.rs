use log::info;
use zip::ZipArchive;
use std::{
  collections::HashMap,
  env,
  error::Error,
  fs::{ self, create_dir_all },
  io::{ Read, Seek },
  path::{ PathBuf, MAIN_SEPARATOR_STR },
  sync::Arc,
  ops::Deref,
};

use crate::{
  Artifact,
  forge_client_install::ForgeInstallError,
  forge_installer_profile::{ v2::ForgeInstallerProfileV2, ForgeInstallerProfile },
  download_utils,
  forge_installer_profile::{ v2::Processor, ForgeVersionLibrary },
};

pub struct PostProcessors {
  profile: Arc<ForgeInstallerProfile>,
  java_path: PathBuf,
  is_client: bool,
  has_tasks: bool,
  processors: Vec<Processor>,
  data: HashMap<String, String>,
}

impl PostProcessors {
  pub fn get_inner_profile(&self) -> &ForgeInstallerProfileV2 {
    if let ForgeInstallerProfile::V2(profile) = self.profile.deref() { profile } else { Err(forge_err!("Not a v2 profile.")).unwrap() }
  }

  pub fn new(arc_profile: Arc<ForgeInstallerProfile>, is_client: bool, java_path: PathBuf) -> Result<Self, ForgeInstallError> {
    if let ForgeInstallerProfile::V2(profile) = arc_profile.deref() {
      let side = if is_client { "client" } else { "server" };
      let data: HashMap<String, String> = profile
        .get_data(is_client)
        .clone()
        .into_iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

      let processors: Vec<Processor> = profile.get_processors(side).clone().into_iter().cloned().collect();
      let has_tasks = !processors.is_empty();

      Ok(Self {
        profile: arc_profile,
        java_path,
        is_client,
        data,
        has_tasks,
        processors,
      })
    } else {
      Err(forge_err!("Not a v2 profile."))?
    }
  }

  pub fn get_libraries(&self) -> Vec<&ForgeVersionLibrary> {
    if self.has_tasks { self.get_inner_profile().get_libraries() } else { vec![] }
  }

  pub fn get_task_count(&self) -> usize {
    if self.has_tasks {
      return 0;
    }
    self.get_inner_profile().get_libraries().len() + self.processors.len() + self.get_inner_profile().get_data(self.is_client).len()
  }

  pub async fn process(
    &mut self,
    libraries_dir: &PathBuf,
    client_jar: &PathBuf,
    mc_dir: &PathBuf,
    installer_path: &PathBuf,
    archive: &mut ZipArchive<impl Read + Seek>
  ) -> Result<(), Box<dyn Error>> {
    if !self.data.is_empty() {
      let mut err = String::new();
      let temp = env::temp_dir().join("forge_installer");
      let _ = fs::remove_dir_all(&temp);
      create_dir_all(&temp)?;
      info!("Created Temporary Directory: {}", temp.display());
      let steps = self.data.len();
      let mut i = 1;
      for (key, value) in &self.data.clone() {
        info!("Processing library {i}/{steps}");
        i += 1;
        if value.starts_with('[') && value.ends_with(']') {
          let inner_value = value[1..value.len() - 1].to_string();
          let artifact = Artifact::try_from(inner_value)?;
          let local_path = artifact.get_local_path(&libraries_dir).to_str().unwrap().to_string();
          self.data.insert(key.clone(), local_path);
          continue;
        }
        if value.starts_with('\'') && value.ends_with('\'') {
          let inner_value = value[1..value.len() - 1].to_string();
          self.data.insert(key.clone(), inner_value);
          continue;
        }
        let target = temp.join(value.replace("/", MAIN_SEPARATOR_STR));
        info!("  Extracting: {} to {}", &value, target.display());
        if let Err(e) = download_utils::extract_file(&value, &target, archive) {
          info!("Failed to extract {value}: {e}");
          err.push_str(&format!("\n  {}", &value));
        }

        let target = target.to_str().unwrap().to_string();
        self.data.insert(key.clone(), target);
      }
      if !err.is_empty() {
        Err(forge_err!("Failed to extract files from archive: {err}"))?;
      }
    }
    self.data.insert("SIDE".to_string(), (if self.is_client { "client" } else { "server" }).to_string());
    self.data.insert("MINECRAFT_JAR".to_string(), client_jar.to_str().unwrap().to_string());
    self.data.insert("MINECRAFT_VERSION".to_string(), self.get_inner_profile().minecraft.clone());
    self.data.insert("ROOT".to_string(), mc_dir.to_str().unwrap().to_string());
    self.data.insert("INSTALLER".to_string(), installer_path.to_str().unwrap().to_string());
    self.data.insert("LIBRARY_DIR".to_string(), libraries_dir.to_str().unwrap().to_string());
    let mut progress = 1;
    if self.processors.len() == 1 {
      info!("Building Processor");
    } else {
      info!("Building Processors");
    }
    for proc in &self.processors {
      info!("Building processor {progress}/{}...", self.processors.len());
      progress += 1;
      info!("===============================================================================");
      proc.process(&self.data, libraries_dir, &self.java_path)?;
    }
    Ok(())
  }
}
