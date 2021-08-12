use crate::serde::{Deserialize, Serialize};
use crate::utils::{anyhow, Result};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum Format {
    ARGB8888,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Image {
    pub format: Format,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Hints {
    pub constant: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data {
    pub relPath: Option<String>,
    pub absPath: Option<String>,
}

impl Data {
    /// Get the filesystem path that this resource should be loaded from
    ///
    /// This is a helper, since there are multiple types of paths. It also
    /// does rule checking to ensure that only one is specified.
    pub fn get_fs_path<'a>(&'a self) -> Result<&'a String> {
        if self.relPath.is_some() && self.absPath.is_some() {
            return Err(anyhow!("Cannot specify both relPath and absPath"));
        }

        if let Some(path) = self.relPath.as_ref() {
            return Ok(&path);
        } else if let Some(path) = self.absPath.as_ref() {
            return Ok(&path);
        } else {
            return Err(anyhow!("No filesystem path was specified for this data."));
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Resource {
    pub name: String,
    pub image: Option<Image>,
    pub data: Data,
    pub hints: Option<Hints>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResourceMap {
    #[serde(rename = "resource", default)]
    pub resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Element {
    pub resource: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Layout {
    #[serde(rename = "el", default)]
    pub elements: Vec<Element>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DakotaDOM {
    pub version: String,
    #[serde(rename = "resourceMap")]
    pub resource_map: ResourceMap,
    pub window: Window,
    pub layout: Layout,
}
