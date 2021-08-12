use crate::serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
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
pub struct DakotaDOM {
    pub version: String,
    pub resourceMap: ResourceMap,
    pub layout: Layout,
}
