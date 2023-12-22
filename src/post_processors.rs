use std::{
    collections::HashMap,
    env,
    error::Error,
    fs::{self, create_dir_all, File},
    io::{ErrorKind, Read, Seek},
    path::{Path, PathBuf, MAIN_SEPARATOR_STR},
    process::Command,
    sync::Arc,
};

use forge_downloader::{Artifact, Sha1Sum};
use regex::Regex;
use reqwest::Url;
use zip::ZipArchive;

use crate::{
    download_utils,
    forge_installer_profile::{
        v2::{MojangLibrary, Processor},
        ForgeInstallerProfile, ForgeVersionLibrary,
    },
};

pub struct PostProcessors {
    profile: Arc<ForgeInstallerProfile>,
    is_client: bool,
    has_tasks: bool,
    processors: Vec<Processor>,
    data: HashMap<String, String>,
}

impl PostProcessors {
    pub fn new(profile: Arc<ForgeInstallerProfile>, is_client: bool) -> Self {
        let side = if is_client { "client" } else { "server" };
        let data: HashMap<String, String> = profile
            .get_data(is_client)
            .clone()
            .into_iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();

        let processors: Vec<Processor> = profile
            .get_processors(side)
            .clone()
            .into_iter()
            .map(|processor| processor.clone())
            .collect();
        let has_tasks = !processors.is_empty();

        Self {
            profile,
            is_client,
            data,
            has_tasks,
            processors,
        }
    }

    pub fn get_libraries(&self) -> Vec<&ForgeVersionLibrary> {
        if self.has_tasks {
            self.profile.get_libraries()
        } else {
            vec![]
        }
    }

    pub fn get_task_count(&self) -> usize {
        if self.has_tasks {
            return 0;
        }
        self.profile.get_libraries().len()
            + self.processors.len()
            + self.profile.get_data(self.is_client).len()
    }

    pub async fn process(
        &mut self,
        libraries_dir: &PathBuf,
        client_jar: &PathBuf,
        mc_dir: &PathBuf,
        archive: &mut ZipArchive<impl Read + Seek>,
    ) -> Result<(), Box<dyn Error>> {
        if !self.data.is_empty() {
            let mut err = String::new();
            let temp = env::temp_dir().join("forge_installer");
            let _ = fs::remove_dir_all(&temp);
            create_dir_all(&temp)?;
            println!("Created Temporary Directory: {}", temp.display());
            let steps = self.data.len();
            let mut i = 1;
            for (key, value) in &self.data.clone() {
                println!("Processing library {i}/{steps}");
                i += 1;
                if value.starts_with('[') && value.ends_with(']') {
                    let inner_value = value[1..value.len() - 1].to_string();
                    let artifact = Artifact::try_from(inner_value)?;
                    let local_path = artifact
                        .get_local_path(&libraries_dir)
                        .to_str()
                        .unwrap()
                        .to_string();
                    self.data.insert(key.clone(), local_path);
                    continue;
                }
                if value.starts_with('\'') && value.ends_with('\'') {
                    let inner_value = value[1..value.len() - 1].to_string();
                    self.data.insert(key.clone(), inner_value);
                    continue;
                }
                let target = temp.join(value.replace("/", MAIN_SEPARATOR_STR));
                println!("  Extracting: {} to {}", &value, target.display());
                if let Err(e) = download_utils::extract_file(&value, &target, archive) {
                    println!("Failed to extract {value}: {e}");
                    err.push_str(&format!("\n  {}", &value));
                }

                let target = target.to_str().unwrap().to_string();
                self.data.insert(key.clone(), target);
            }
            if !err.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("Failed to extract files from archive: {}", err),
                )));
            }
        }
        self.data.insert(
            "SIDE".to_string(),
            if self.is_client { "client" } else { "server" }.to_string(),
        );
        self.data.insert(
            "MINECRAFT_JAR".to_string(),
            client_jar.to_str().unwrap().to_string(),
        );
        self.data.insert(
            "MINECRAFT_VERSION".to_string(),
            self.profile.get_minecraft(),
        );
        self.data
            .insert("ROOT".to_string(), mc_dir.to_str().unwrap().to_string());
        // TODO: self.data.insert("INSTALLER".to_string(), installer.to_str().unwrap().to_string());
        self.data.insert(
            "LIBRARY_DIR".to_string(),
            libraries_dir.to_str().unwrap().to_string(),
        );
        let mut progress = 1;
        if self.processors.len() == 1 {
            println!("Building Processor");
        } else {
            println!("Building Processors");
        }
        for proc in &self.processors {
            println!("Building processor {progress}/{}...", self.processors.len());
            progress += 1;
            println!(
                "==============================================================================="
            );
            let mut outputs = HashMap::new();
            if !proc.outputs.is_empty() {
                let mut miss = false;
                println!("  Cache: ");
                for (e_key, e_value) in proc.outputs.clone() {
                    let mut key = Some(e_key.clone());
                    if e_key.starts_with('[') && e_key.ends_with(']') {
                        let artifact = Artifact::try_from(e_key[1..e_key.len() - 1].to_string())?;
                        key = Some(
                            artifact
                                .get_local_path(&libraries_dir)
                                .to_str()
                                .unwrap()
                                .to_string(),
                        );
                    } else {
                        key = Some(Self::replace_tokens(&self.data, &e_key)?);
                    }
                    let mut value = e_value.clone();
                    if let Some(value1) = value {
                        value = Self::replace_tokens(&self.data, &value1).ok();
                    }
                    if key.is_none() || value.is_none() {
                        return Err(Box::new(std::io::Error::new(
                            ErrorKind::Other,
                            format!(
                                "Invalid configuration, bad output config: [{}: {}]",
                                key.unwrap(),
                                value.unwrap()
                            ),
                        )));
                    }
                    let (key, value) = (key.unwrap(), value.unwrap());
                    outputs.insert(key.clone(), value.clone());
                    let artifact = Path::new(&key);
                    if !artifact.exists() {
                        println!("    {key} Missing");
                        miss = true;
                        continue;
                    }
                    let sha = Sha1Sum::from_reader(&mut File::open(artifact)?).ok();
                    if sha == Sha1Sum::try_from(value.clone()).ok() {
                        println!("    {key} Validated: {value}");
                        continue;
                    }
                    println!("    {key}");
                    println!("      Expected: {}", value);
                    println!("      Actual:   {}", sha.unwrap());
                    miss = true;
                    fs::remove_file(artifact)?;
                }
                if !miss {
                    println!("  Cache Hit!");
                    continue;
                }
            }
            let jar = proc.jar.get_local_path(&libraries_dir);
            if !jar.exists() || !jar.is_file() {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("  Missing Jar for processor: {}", jar.display()),
                )));
            }

            let main_class = {
                let mut buf = String::new();
                let mut jar_file = ZipArchive::new(File::open(&jar)?)?;
                jar_file
                    .by_name("META-INF/MANIFEST.MF")?
                    .read_to_string(&mut buf)?;
                buf.lines()
                    .filter_map(|line| line.split_once(":"))
                    .find(|(key, _)| key == &"Main-Class")
                    .map(|(_, value)| value.trim())
                    .unwrap_or_default()
                    .to_string()
            };
            if main_class.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("  Jar does not have main class: {}", jar.to_str().unwrap()),
                )));
            }
            println!("  MainClass: {main_class}");
            let mut classpath = vec![];
            let mut err = String::new();
            println!("  Classpath:");
            println!("    {}", jar.to_str().unwrap());
            classpath.push(jar);
            for dep in &proc.classpath {
                let lib = dep.get_local_path(&libraries_dir);
                if !lib.is_file() {
                    err.push_str(&format!("\n  {}", dep.get_descriptor()));
                }
                println!("    {}", lib.to_str().unwrap());
                classpath.push(lib);
            }
            if err.len() > 0 {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("  Missing Processor Dependencies: {err}"),
                )));
            }
            let mut args = vec![];
            for arg in &proc.args {
                if arg.starts_with('[') && arg.ends_with(']') {
                    let artifact = Artifact::try_from(arg[1..arg.len() - 1].to_string())?;
                    args.push(
                        artifact
                            .get_local_path(&libraries_dir)
                            .to_str()
                            .unwrap()
                            .to_string(),
                    );
                } else {
                    args.push(Self::replace_tokens(&self.data, &arg)?);
                }
            }
            if err.len() > 0 {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("  Missing Processor data values: {err}"),
                )));
            }
            println!(
                "  Args: {}",
                args.iter()
                    .map(|a| if a.contains(' ') || a.contains(',') {
                        format!("\"{}\"", a)
                    } else {
                        a.clone()
                    })
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
            let _ = Command::new("java") // TODO: load java
                .args(cmd_args)
                .spawn()?
                .wait()?;
            for (key, value) in outputs {
                let artifact = Path::new(&key);
                if !artifact.exists() {
                    err.push_str(&format!("\n    {key} missing"));
                    continue;
                }
                let sha = Sha1Sum::from_reader(&mut File::open(artifact)?)?;
                if sha == Sha1Sum::try_from(value.clone())? {
                    println!("  Output: {key} Checksum Validated: {sha}");
                    continue;
                }
                err.push_str(&format!(
                    "\n    {key}\n      Expected: {value}\n      Actual:  {sha}"
                ));
                if fs::remove_file(&artifact).is_err() {
                    err.push_str(&format!("\n      Could not delete file"));
                }
            }
            if err.len() > 0 {
                return Err(Box::new(std::io::Error::new(
                    ErrorKind::Other,
                    format!("  Processor failed, invalid outputs: {err}"),
                )));
            }
        }
        Ok(())
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
                buf.push(value.chars().nth(char_index + 1).unwrap());
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
                        key.push(value.chars().nth(y + 1).unwrap());
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

    // fn replace_tokens(
    //     tokens: &HashMap<String, String>,
    //     value: &str,
    // ) -> Result<String, Box<dyn Error>> {

    //     // let mut buf = String::new();
    //     // let mut char_index = 0;

    //     // while char_index < value.len() {
    //     //     let ch = value.chars().nth(char_index).unwrap();

    //     //     if ch == '\\' {
    //     //         if char_index == value.len() - 1 {
    //     //             return Err(Box::new(std::io::Error::new(
    //     //                 ErrorKind::Other,
    //     //                 format!("Illegal pattern (Bad escape): {value}"),
    //     //             )));
    //     //         }
    //     //         buf.push(value.chars().nth(char_index + 1).unwrap());
    //     //         char_index += 2;
    //     //     } else if ch == '{' || ch == '\'' {
    //     //         let mut key = String::new();
    //     //         let mut y = char_index + 1;

    //     //         while y <= value.len() {
    //     //             let d = value.chars().nth(y).unwrap();

    //     //             if d == '\\' {
    //     //                 if y == value.len() - 1 {
    //     //                     return Err(Box::new(std::io::Error::new(
    //     //                         ErrorKind::Other,
    //     //                         format!("Illegal pattern (Bad escape): {value}"),
    //     //                     )));
    //     //                 }
    //     //                 key.push(value.chars().nth(y + 1).unwrap());
    //     //                 y += 2;
    //     //             } else {
    //     //                 if (ch == '{' && d == '}') || (ch == '\'' && d == '\'') {
    //     //                     char_index = y;
    //     //                     break;
    //     //                 }
    //     //                 key.push(d);
    //     //                 y += 1;
    //     //             }
    //     //         }

    //     //         if ch == '\'' {
    //     //             buf.push_str(&key);
    //     //         } else {
    //     //             if !tokens.contains_key(&key) {
    //     //                 return Err(Box::new(std::io::Error::new(
    //     //                     ErrorKind::Other,
    //     //                     format!("Illegal pattern: {} Missing Key: {}", value, key),
    //     //                 )));
    //     //             }
    //     //             buf.push_str(tokens.get(&key).unwrap());
    //     //         }
    //     //     } else {
    //     //         buf.push(ch);
    //     //         char_index += 1;
    //     //     }
    //     // }

    //     // Ok(buf)
    // }
}
