use crate::serde::{Deserialize, Serialize};
use crate::utils::{anyhow, Result};

use std::cmp::{Ord, PartialOrd};

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
pub struct Content {
    pub el: Option<std::boxed::Box<Element>>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Serialize, Deserialize)]
pub struct Offset {
    pub x: u32,
    pub y: u32,
}

impl Offset {
    fn union(&mut self, other: &Self) {
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
    fn union(&mut self, other: &Self) {
        self.width = std::cmp::max(self.width, other.width);
        self.height = std::cmp::max(self.height, other.height);
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
    pub size: Option<Size>,
    #[serde(rename = "scrolling", default)]
    pub bounds: Option<Edges>,
    #[serde(rename = "el", default)]
    pub children: Vec<Element>,
}

impl Element {
    /// Resize this element to contain all of its children.
    ///
    /// This can be used when the size of a box was not specified, and it
    /// should be grown to be able to hold all of the child boxes.
    ///
    /// We don't need to worry about bounding by an available size, this is
    /// to be used when there are no bounds (such as in a scrolling arena) and
    /// we just want to grow this element to fit everything.
    ///
    /// TODO: Needs to handle using the offset to stack many boxes.
    pub fn resize_to_children(&mut self) -> Result<()> {
        // This closure gets around some annoying borrow checker shortcomings
        //
        // Basically we want to run the following updating self.offset and self.size.
        // If we are iterating in loops and the like, then the borrow checker doesn't like
        // us passing an element owned by self into a function
        let resize_func =
            |parent_size: &mut Option<Size>, _parent_offset: &mut Option<Offset>, other: &Self| {
                let mut size = match other.size.as_ref() {
                    Some(s) => s.clone(),
                    None => return Err(anyhow!("Input element does not have a size")),
                };

                // We have a size, so we need to resize it. Otherwise this element gains
                // the size of the other one.
                if let Some(my_size) = parent_size.as_mut() {
                    // add any offsets to our size
                    if let Some(offset) = other.offset.as_ref() {
                        size.width += offset.x;
                        size.height += offset.y;
                    }
                    my_size.union(&size);
                } else {
                    *parent_size = Some(size);
                }

                Ok(())
            };

        for i in 0..self.children.len() {
            resize_func(&mut self.size, &mut self.offset, &self.children[i])?;
        }

        if let Some(content) = self.content.as_mut() {
            if let Some(child) = content.el.as_ref() {
                resize_func(&mut self.size, &mut self.offset, child)?;
            }
        }

        return Ok(());
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Layout {
    #[serde(rename = "el")]
    pub root_element: Element,
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
