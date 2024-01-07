use std::{
  collections::HashMap,
  path::{ PathBuf, Path },
  io::{ ErrorKind, Read, BufReader, BufRead, Cursor },
  fs::{ File, self },
  process::{ Command, Stdio }, os::windows::process::CommandExt,
};

use chrono::{ DateTime, Utc };
use log::{ info, debug, error };
use zip::ZipArchive;
use crate::{ Sha1Sum, Artifact };
use serde::{ Deserialize, Serialize };
use serde_json::Value;

use super::ForgeVersionLibrary;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeInstallerProfileV2 {
  // #[serde(default, rename="_comment", skip_serializing_if = "Vec::is_empty")]
  // _comment: Vec<String>,
  pub spec: u8,
  pub profile: String, // "forge"
  pub version: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub icon: Option<String>,
  pub json: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub path: Option<Artifact>, // GradleSpecifierProperty
  pub logo: String,
  pub minecraft: String,
  pub welcome: String,
  pub data: HashMap<String, DataFile>,
  pub processors: Vec<Processor>,
  pub libraries: Vec<ForgeVersionLibrary>,

  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub hide_extract: Option<bool>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub mirror_list: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub server_jar_path: Option<String>,
}

impl ForgeInstallerProfileV2 {
  pub fn get_processors(&self, side: &str) -> Vec<&Processor> {
    self.processors
      .iter()
      .filter(|p| p.is_side(side))
      .collect()
  }

  pub fn get_data(&self, is_client: bool) -> HashMap<&String, &String> {
    self.data
      .iter()
      .map(|(key, data_file)| {
        let url = if is_client { &data_file.client } else { &data_file.server };
        (key, url)
      })
      .collect::<HashMap<_, _>>()
  }

  pub fn get_libraries(&self) -> Vec<&ForgeVersionLibrary> {
    self.libraries.iter().collect()
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataFile {
  client: String,
  server: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Processor {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sides: Option<Vec<String>>,
  pub jar: Artifact,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub classpath: Vec<Artifact>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub args: Vec<String>,
  #[serde(default, skip_serializing_if = "HashMap::is_empty")]
  pub outputs: HashMap<String, Option<String>>,
}

impl Processor {
  pub fn is_side(&self, side: &str) -> bool {
    if let Some(sides) = &self.sides { sides.contains(&side.to_string()) } else { true }
  }

  pub fn process(&self, data: &HashMap<String, String>, libraries_dir: &PathBuf, java_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut outputs = HashMap::new();
    if !&self.outputs.is_empty() {
      let mut miss = false;
      info!("  Cache: ");
      for (e_key, e_value) in &self.outputs.clone() {
        let key = if e_key.starts_with('[') && e_key.ends_with(']') {
          let artifact = Artifact::try_from(e_key[1..e_key.len() - 1].to_string())?;
          Some(artifact.get_local_path(&libraries_dir).to_str().unwrap().to_string())
        } else {
          Some(replace_tokens(data, &e_key)?)
        };
        let mut value = e_value.clone();
        if let Some(value1) = value {
          value = replace_tokens(data, &value1).ok();
        }
        if key.is_none() || value.is_none() {
          return Err(
            Box::new(
              std::io::Error::new(ErrorKind::Other, format!("Invalid configuration, bad output config: [{}: {}]", key.unwrap(), value.unwrap()))
            )
          );
        }
        let (key, value) = (key.unwrap(), value.unwrap());
        outputs.insert(key.clone(), value.clone());
        let artifact = Path::new(&key);
        if !artifact.exists() {
          info!("    {key} Missing");
          miss = true;
          continue;
        }
        let sha = Sha1Sum::from_reader(&mut File::open(artifact)?).ok();
        if sha == Sha1Sum::try_from(value.clone()).ok() {
          info!("    {key} Validated: {value}");
          continue;
        }
        info!("    {key}");
        info!("      Expected: {}", value);
        info!("      Actual:   {}", sha.unwrap());
        miss = true;
        fs::remove_file(artifact)?;
      }
      if !miss {
        info!("  Cache Hit!");
        // continue;
        return Ok(());
      }
    }
    let jar = &self.jar.get_local_path(&libraries_dir);
    if !jar.exists() || !jar.is_file() {
      return Err(Box::new(std::io::Error::new(ErrorKind::Other, format!("  Missing Jar for processor: {}", jar.display()))));
    }

    let main_class = {
      let mut buf = String::new();
      let mut jar_file = ZipArchive::new(File::open(&jar)?)?;
      jar_file.by_name("META-INF/MANIFEST.MF")?.read_to_string(&mut buf)?;
      buf
        .lines()
        .filter_map(|line| line.split_once(":"))
        .find(|(key, _)| key == &"Main-Class")
        .map(|(_, value)| value.trim())
        .unwrap_or_default()
        .to_string()
    };
    if main_class.is_empty() {
      return Err(Box::new(std::io::Error::new(ErrorKind::Other, format!("  Jar does not have main class: {}", jar.to_str().unwrap()))));
    }
    info!("  MainClass: {main_class}");
    let mut classpath = vec![];
    let mut err = String::new();
    info!("  Classpath:");
    info!("    {}", jar.to_str().unwrap());
    classpath.push(jar.clone());
    for dep in &self.classpath {
      let lib = dep.get_local_path(&libraries_dir);
      if !lib.is_file() {
        err.push_str(&format!("\n  {}", dep.get_descriptor()));
      }
      info!("    {}", lib.to_str().unwrap());
      classpath.push(lib);
    }
    if err.len() > 0 {
      return Err(Box::new(std::io::Error::new(ErrorKind::Other, format!("  Missing Processor Dependencies: {err}"))));
    }
    let mut args = vec![];
    for arg in &self.args {
      if arg.starts_with('[') && arg.ends_with(']') {
        let artifact = Artifact::try_from(arg[1..arg.len() - 1].to_string())?;
        args.push(artifact.get_local_path(&libraries_dir).to_str().unwrap().to_string());
      } else {
        args.push(replace_tokens(&data, &arg)?);
      }
    }
    if err.len() > 0 {
      return Err(Box::new(std::io::Error::new(ErrorKind::Other, format!("  Missing Processor data values: {err}"))));
    }
    info!(
      "  Args: {}",
      args
        .iter()
        .map(|a| if a.contains(' ') || a.contains(',') { format!("\"{}\"", a) } else { a.clone() })
        .collect::<Vec<String>>()
        .join(", ")
    );

    let classpath_separator = if cfg!(windows) { ";" } else { ":" };
    let mut cmd_args = vec!["-cp".to_string()];
    let classpath = classpath
      .iter()
      .map(|path| path.to_str().unwrap())
      .collect::<Vec<_>>()
      .join(classpath_separator);
    cmd_args.push(classpath);
    cmd_args.push(main_class);
    cmd_args.extend(args);

    {
      let child = Command::new(java_path.to_str().unwrap()).stdout(Stdio::piped()).stderr(Stdio::piped()).args(cmd_args).creation_flags(0x08000000).spawn()?.wait_with_output()?;
      let stdout = BufReader::new(Cursor::new(child.stdout));
      let stderr = BufReader::new(Cursor::new(child.stderr));
      for line in stdout.lines() {
        if let Ok(line) = line {
          info!("{line}");
        }
      }
      for line in stderr.lines() {
        if let Ok(line) = line {
          error!("{line}");
        }
      }
    }

    for (key, value) in outputs {
      let artifact = Path::new(&key);
      if !artifact.exists() {
        err.push_str(&format!("\n    {key} missing"));
        continue;
      }
      let sha = Sha1Sum::from_reader(&mut File::open(artifact)?)?;
      if sha == Sha1Sum::try_from(value.clone())? {
        info!("  Output: {key} Checksum Validated: {sha}");
        continue;
      }
      err.push_str(&format!("\n    {key}\n      Expected: {value}\n      Actual:  {sha}"));
      if fs::remove_file(&artifact).is_err() {
        err.push_str(&format!("\n      Could not delete file"));
      }
    }
    if err.len() > 0 {
      return Err(Box::new(std::io::Error::new(ErrorKind::Other, format!("  Processor failed, invalid outputs: {err}"))));
    }

    Ok(())
  }
}

// Move to mod
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangLibrary {
  // #[serde(default, skip_serializing_if = "Option::is_none")]
  // extract: Option<MojangLibraryExtractRules>,
  pub name: Artifact,
  pub downloads: /*Option<*/ MojangLibraryDownloads /* >*/,
  // natives:
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangLibraryDownloads {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub artifact: Option<MojangArtifact>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub classifiers: Option<HashMap<String, MojangArtifact>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MojangArtifact {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub path: Option<String /*Artifact*/>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub sha1: Option</*String*/ Sha1Sum>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub size: Option<u32>,
}

impl MojangArtifact {
  pub fn new(artifact: String) -> Self {
    Self {
      path: Some(artifact),
      sha1: None,
      size: None,
      url: None,
    }
  }
}

// Only change between them: minecraftArguments TODO: check!
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForgeVersionFileV2 {
  pub id: String,
  pub time: DateTime<Utc>,
  pub release_time: DateTime<Utc>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub inherits_from: Option<String>,
  #[serde(rename = "type")]
  pub release_type: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  logging: Option<Value>,
  pub main_class: String,
  #[serde(default, skip_serializing_if = "HashMap::is_empty")]
  pub downloads: HashMap<String, Download>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub libraries: Vec<MojangLibrary>,

  #[serde(default, skip_serializing_if = "HashMap::is_empty")]
  pub arguments: HashMap<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Download {
  pub sha1: Sha1Sum /*String*/,
  pub size: u16,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  #[serde(default)]
  pub provided: bool,
}

impl Download {
  pub fn get_url(&self) -> Option<String> {
    if self.url.is_none() || self.provided { Some(String::new()) } else { self.url.clone() }
  }
}

fn replace_tokens(tokens: &HashMap<String, String>, value: &str) -> Result<String, String> {
  let mut buf = String::new();
  let mut char_index = 0;

  while char_index < value.len() {
    let ch = value.chars().nth(char_index).unwrap();
    if ch == '\\' {
      if char_index == value.len() - 1 {
        return Err(format!("Illegal pattern (Bad escape): {}", value));
      }
      buf.push(
        value
          .chars()
          .nth(char_index + 1)
          .unwrap()
      );
      char_index += 2;
    } else if ch == '{' || ch == '\'' {
      let mut key = String::new();
      let mut y = char_index + 1;

      while y <= value.len() {
        if y == value.len() {
          return Err(format!("Illegal pattern (Unclosed {}): {}", ch, value));
        }
        let d = value.chars().nth(y).unwrap();

        if d == '\\' {
          if y == value.len() - 1 {
            return Err(format!("Illegal pattern (Bad escape): {}", value));
          }
          key.push(
            value
              .chars()
              .nth(y + 1)
              .unwrap()
          );
          y += 2;
        } else {
          if (ch == '{' && d == '}') || (ch == '\'' && d == '\'') {
            char_index = y;
            break;
          }
          key.push(d);
          y += 1;
        }
      }

      if ch == '\'' {
        buf.push_str(&key);
      } else {
        if !tokens.contains_key(&key) {
          return Err(format!("Illegal pattern: {} Missing Key: {}", value, key));
        }
        buf.push_str(tokens.get(&key).unwrap());
      }
      char_index += 1;
    } else {
      buf.push(ch);
      char_index += 1;
    }
  }

  Ok(buf)
}
