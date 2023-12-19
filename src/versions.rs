// use std::{
//     collections::HashMap,
//     fmt,
//     fs::{self, File},
//     path::PathBuf,
// };

// use serde::{Deserialize, Serialize};

// #[derive(Debug, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct CompleteVersion {
//     #[serde(default)]
//     pub inherits_from: Option<String>,
//     #[serde(default)]
//     pub id: Option<String>,
//     #[serde(default)]
//     pub time: Option<String>,
//     #[serde(default)]
//     pub release_time: Option<String>,
//     #[serde(default, rename = "type")]
//     pub release_type: Option<String>,
//     #[serde(default)]
//     pub minecraft_arguments: Option<String>,
//     #[serde(default)]
//     pub libraries: Vec<Library>,
//     pub main_class: String,
//     pub minimum_launcher_version: i32,
//     #[serde(default)]
//     pub incompatibility_reason: Option<String>,
//     pub assets: String,
//     #[serde(default)]
//     pub compatibility_rules: Option<CompatibilityRule>,
//     #[serde(default)]
//     pub jar: Option<String>,
// }

// pub trait Version {
//     fn get_id(&self) -> String;
//     fn get_type(&self) -> String;
//     fn get_updated_time(&self) -> String;
//     fn get_release_time(&self) -> String;
// }

// // Release types
// pub trait ReleaseType {
//     fn get_name(&self) -> &str;
// }

// #[derive(Deserialize)]
// pub enum MinecraftReleaseType {
//     Snapshot,
//     Release,
//     Custom,
//     OldBeta,
//     OldAlpha,
// }

// impl MinecraftReleaseType {
//     pub fn values() -> Vec<MinecraftReleaseType> {
//         vec![
//             MinecraftReleaseType::Snapshot,
//             MinecraftReleaseType::Release,
//             MinecraftReleaseType::Custom,
//             MinecraftReleaseType::OldBeta,
//             MinecraftReleaseType::OldAlpha,
//         ]
//     }

//     fn get_description(&self) -> &str {
//         match self {
//             MinecraftReleaseType::Snapshot => {
//                 "Enable experimental development versions (\"snapshots\")"
//             }
//             MinecraftReleaseType::Release => "",
//             MinecraftReleaseType::Custom => "Enable versions with custom release types",
//             MinecraftReleaseType::OldBeta => {
//                 "Allow use of old \"Beta\" Minecraft versions (From 2010-2011)"
//             }
//             MinecraftReleaseType::OldAlpha => {
//                 "Allow use of old \"Alpha\" Minecraft versions (From 2010)"
//             }
//         }
//     }
// }

// impl ReleaseType for MinecraftReleaseType {
//     fn get_name(&self) -> &str {
//         match self {
//             MinecraftReleaseType::Snapshot => "snapshot",
//             MinecraftReleaseType::Release => "release",
//             MinecraftReleaseType::Custom => "custom",
//             MinecraftReleaseType::OldBeta => "old_beta",
//             MinecraftReleaseType::OldAlpha => "old_alpha",
//         }
//     }
// }

// pub trait VersionList {
//     fn get_version(&self, version_id: &str) -> Option<&CompleteVersion>;
//     fn add_version(&mut self, version: CompleteVersion) -> Result<(), Box<dyn std::error::Error>>;
//     fn refresh_versions(&mut self) -> Result<(), Box<dyn std::error::Error>>;
// }

// pub struct LocalVersionList {
//     version_dir: PathBuf,
//     versions: HashMap<String, CompleteVersion>,
// }

// impl VersionList for LocalVersionList {
//     fn get_version(&self, version_id: &str) -> Option<&CompleteVersion> {
//         self.versions.get(version_id)
//     }

//     fn add_version(&mut self, version: CompleteVersion) -> Result<(), Box<dyn std::error::Error>> {
//         let id = version
//             .id
//             .ok_or(StringError(format!("Cannot add blank version (id is null)")).to_box())?;
//         if self.get_version(&id).is_some() {
//             Err(StringError(format!("Version '{}' is already tracked", &id)).to_box())
//         } else {
//             self.versions.insert(id, version);
//             Ok(())
//         }
//     }

//     fn refresh_versions(&mut self) -> Result<(), Box<dyn std::error::Error>> {
//         let version_dirs = fs::read_dir(&self.version_dir)?
//             .flatten()
//             .collect::<Vec<_>>();
//         for version_dir in version_dirs {
//             let version_dir = version_dir.path();
//             let version_id = version_dir.file_name().unwrap().to_string_lossy();
//             let version_json = version_dir.join(format!("{}.json", &version_id));
//             if !version_dir.is_dir() {
//                 continue;
//             }

//             let json_path = format!("versions/{0}/{0}.json", &version_id);
//             let complete_version: CompleteVersion =
//                 serde_json::from_reader(File::open(&version_json)?)?;
//             if complete_version.release_type.is_none() {
//                 // continue;
//                 return Err(StringError(format!(
//                     "Ignoring: {json_path}; it has an invalid version specified (type is null)",
//                 ))
//                 .to_box());
//             }

//             if complete_version
//                 .id
//                 .map(|id| id == version_id)
//                 .unwrap_or(false)
//             {
//                 self.add_version(complete_version);
//             } else {
//                 println!(
//                     "Ignoring: {json_path}; it contains id: '{}' expected '{version_id}'",
//                     complete_version.id.unwrap_or("null".to_string())
//                 );
//             }
//         }
//         Ok(())
//     }
// }

// pub struct RemoteVersionList {
//     manifest_url: Url,
//     versions: Vec<PartialVersion>,
// }

// impl VersionList for RemoteVersionList {
//     fn get_version(&self, version_id: &str) -> Option<&CompleteVersion> {}
// }

// #[derive(Debug)]
// pub struct StringError(String);

// impl StringError {
//     pub fn to_box(self) -> Box<dyn std::error::Error> {
//         Box::new(self)
//     }
// }

// impl fmt::Display for StringError {
//     fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
//         write!(f, "{}", self.0)
//     }
// }

// impl std::error::Error for StringError {}
