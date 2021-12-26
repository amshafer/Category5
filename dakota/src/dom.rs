use crate::serde::{Deserialize, Serialize};
use crate::utils::{anyhow, Result};
use crate::LayoutSpace;

use std::cell::RefCell;
use std::cmp::{Ord, PartialOrd};
use std::rc::Rc;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum Format {
    ARGB8888,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Image {
    pub format: Format,
    pub data: Data,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Hints {
    pub constant: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data {
    #[serde(rename = "relPath", default)]
    pub rel_path: Option<String>,
    #[serde(rename = "abs_path", default)]
    pub abs_path: Option<String>,
}

impl Data {
    /// Get the filesystem path that this resource should be loaded from
    ///
    /// This is a helper, since there are multiple types of paths. It also
    /// does rule checking to ensure that only one is specified.
    pub fn get_fs_path<'a>(&'a self) -> Result<&'a String> {
        if self.rel_path.is_some() && self.abs_path.is_some() {
            return Err(anyhow!("Cannot specify both rel_path and abs_path"));
        }

        if let Some(path) = self.rel_path.as_ref() {
            return Ok(&path);
        } else if let Some(path) = self.abs_path.as_ref() {
            return Ok(&path);
        } else {
            return Err(anyhow!("No filesystem path was specified for this data."));
        }
    }
}

#[derive(Copy, Clone, Serialize, Deserialize, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Resource {
    pub name: String,
    pub image: Option<Image>,
    pub color: Option<Color>,
    pub hints: Option<Hints>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ResourceMap {
    #[serde(rename = "resource", default)]
    pub resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Content {
    pub el: Option<Rc<RefCell<Element>>>,
}

/// This is a relative offset that offsets an element
/// by a percentage of the size of the available space.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone, Serialize, Deserialize)]
pub struct RelativeOffset {
    pub x: f32,
    pub y: f32,
}

impl RelativeOffset {
    pub fn new(w: f32, h: f32) -> Self {
        assert!((w >= 0.0 && w < 1.0) && (h >= 0.0 && h < 1.0));
        Self { x: w, y: h }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Serialize, Deserialize)]
pub struct Offset {
    pub x: u32,
    pub y: u32,
}

impl Offset {
    pub fn new(w: u32, h: u32) -> Self {
        Self { x: w, y: h }
    }

    #[allow(dead_code)]
    pub fn union(&mut self, other: &Self) {
        self.x = std::cmp::max(self.x, other.x);
        self.y = std::cmp::max(self.y, other.y);
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            width: w,
            height: h,
        }
    }
    pub fn union(&mut self, other: &Self) {
        self.width = std::cmp::max(self.width, other.width);
        self.height = std::cmp::max(self.height, other.height);
    }
}

/// This is a relative size that sizes an element
/// by a percentage of the size of the available space.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone, Serialize, Deserialize)]
pub struct RelativeSize {
    pub width: f32,
    pub height: f32,
}

impl RelativeSize {
    pub fn new(w: f32, h: f32) -> Self {
        assert!((w >= 0.0 && w < 1.0) && (h >= 0.0 && h < 1.0));
        Self {
            width: w,
            height: h,
        }
    }
}

/// The boundary behavior of the edges of a box. True
/// if scrolling is allowed on that axis in this box.
#[derive(Serialize, Deserialize, Debug)]
pub struct Edges {
    pub horizontal: Option<bool>,
    pub vertical: Option<bool>,
}

impl Default for Edges {
    fn default() -> Self {
        Self {
            horizontal: None,
            vertical: Some(true),
        }
    }
}

/// Only one of content or children may be defined,
/// they are mutually exclusive.
///
/// Element layout will:
///   a) expand horizontally to fit their container
///   b) expand vertically to fit their container
///   c) a element's content is scaled to fit the element.
///   d) default behavior is only vertical scrolling allowed for
///      when the element's content is longer than the element's height.
///      d.1) if the user does not specify a vertical/horizontal scrolling,
///           then that edge of the element is static. It is basically
///           a window, and scrolling may occur within that element in
///           whatever dimensions were not marked as scrolling.
///           (e.g. default behavior is a horizontal scrolling = false
///            and vertical scrolling = true)
///   e) a-b may be limited by dimensions specified by the user.
///      the dimensions are not specified, then the resource's
///      default size is used.
///   f) regarding (e), if the element's size does not fill the container,
///      then:
///      f.1) the elementes will be laid out horizontally first,
///      f.2) with vertical wrapping if there is not enough room.
///           
#[derive(Serialize, Deserialize, Debug)]
pub struct Element {
    pub resource: Option<String>,
    pub content: Option<Content>,
    pub offset: Option<Offset>,
    #[serde(rename = "relativeOffset", default)]
    pub rel_offset: Option<RelativeOffset>,
    pub size: Option<Size>,
    #[serde(rename = "relativeSize", default)]
    pub rel_size: Option<RelativeSize>,
    #[serde(rename = "scrolling", default)]
    pub bounds: Option<Edges>,
    #[serde(rename = "el", default)]
    pub children: Vec<Rc<RefCell<Element>>>,
}

impl Element {
    /// Get the final size to use as an offset into the
    /// parent space. This takes care of handling the relative
    /// proportional offset size
    pub fn get_final_offset(&self, space: &LayoutSpace) -> Result<Option<Offset>> {
        if self.offset.is_some() && self.rel_offset.is_some() {
            return Err(anyhow!(
                "Element.offset and Element.relativeOffset cannot both be defined"
            ));
        }

        if let Some(rel) = self.rel_offset.as_ref() {
            if !((rel.x >= 0.0 && rel.x < 1.0) && (rel.y >= 0.0 && rel.y < 1.0)) {
                return Err(anyhow!(
                    "Element.relativeOffset should use values in the range (0.0, 1.0)"
                ));
            }
            return Ok(Some(Offset::new(
                (space.avail_width as f32 * rel.x) as u32,
                (space.avail_height as f32 * rel.y) as u32,
            )));
        }

        Ok(self.offset)
    }

    /// Get the final size to use within the parent space.
    /// This takes care of handling the relative
    /// proportional size.
    pub fn get_final_size(&self, space: &LayoutSpace) -> Result<Option<Size>> {
        if self.size.is_some() && self.rel_size.is_some() {
            return Err(anyhow!(
                "Element.size and Element.relativeSize cannot both be defined"
            ));
        }

        if let Some(rel) = self.rel_size.as_ref() {
            if !((rel.width >= 0.0 && rel.width < 1.0) && (rel.height >= 0.0 && rel.height < 1.0)) {
                return Err(anyhow!(
                    "Element.relativeSize should use values in the range (0.0, 1.0)"
                ));
            }
            return Ok(Some(Size::new(
                (space.avail_width as f32 * rel.width) as u32,
                (space.avail_height as f32 * rel.height) as u32,
            )));
        }

        Ok(self.size)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Layout {
    #[serde(rename = "el")]
    pub root_element: Rc<RefCell<Element>>,
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
