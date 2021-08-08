use crate::serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
enum Format {
    ARGB8888,
}

#[derive(Serialize, Deserialize, Debug)]
struct Image {
    format: Format,
}

#[derive(Serialize, Deserialize, Debug)]
struct Hints {
    constant: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct Data {
    relPath: Option<String>,
    absPath: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Resource {
    name: String,
    image: Option<Image>,
    data: Data,
    hints: Option<Hints>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ResourceMap {
    #[serde(rename = "resource", default)]
    resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Element {
    resource: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Layout {
    #[serde(rename = "el", default)]
    elements: Vec<Element>,
}

#[derive(Serialize, Deserialize, Debug)]
struct DakotaDOM {
    version: String,
    resourceMap: ResourceMap,
    layout: Layout,
}
