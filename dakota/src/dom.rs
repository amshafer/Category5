use crate::serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum Format {
    ARGB8888,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Image {
    format: Format,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Hints {
    constant: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data {
    relPath: Option<String>,
    absPath: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Resource {
    name: String,
    image: Option<Image>,
    data: Data,
    hints: Option<Hints>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResourceMap {
    #[serde(rename = "resource", default)]
    resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Element {
    resource: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Layout {
    #[serde(rename = "el", default)]
    elements: Vec<Element>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DakotaDOM {
    version: String,
    resourceMap: ResourceMap,
    layout: Layout,
}
