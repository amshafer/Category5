/// Define a DOM heirarchy and dakota data file format
///
/// Austin Shafer - 2022
use crate::font::CachedChar;
use crate::utils::{anyhow, Result};
use crate::DakotaId;

use std::cmp::{Ord, PartialOrd};
use std::sync::Arc;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Format {
    ARGB8888,
    XRGB8888,
}

impl Format {
    pub fn get_size(&self) -> usize {
        match self {
            Format::XRGB8888 | Format::ARGB8888 => 4,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Image {
    pub format: Format,
    pub data: Data,
}

#[derive(Default, Clone, Debug)]
pub struct Hints {
    pub constant: bool,
}

#[derive(Debug, Clone)]
pub struct Data {
    pub rel_path: Option<String>,
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

/// Color definition for a resource
///
/// Resources that are not defined by images may instead be defined
/// by a color. Values are in the range [0.0, 1.0].
#[derive(Copy, PartialEq, Clone, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// Create a new color from values [0.0, 1.0]
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug)]
pub struct Resource {
    pub name: String,
    pub image: Option<Image>,
    pub color: Option<Color>,
    pub hints: Option<Hints>,
}

#[derive(Debug, Clone)]
pub struct Content {
    pub el: DakotaId,
}

impl Content {
    pub fn new(el: DakotaId) -> Self {
        Self { el: el }
    }
}

/// Represents a possibly relative value. This will
/// either be a f32 scaling value or a constant size
/// u32.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone)]
pub enum Value {
    /// This is a relative value that modifies an element
    /// by a percentage of the size of the available space.
    Relative(f32),
    Constant(i32),
}

impl Value {
    fn scale(current: f32, val: f32) -> Result<i32> {
        if !(current >= 0.0 && current <= 1.0) {
            return Err(anyhow!(
                "Element.relativeOffset should use values in the range (0.0, 1.0)"
            ));
        }
        Ok((current * val) as i32)
    }

    pub fn get_value(&self, avail_space: i32) -> Result<i32> {
        Ok(match *self {
            Self::Relative(r) => Self::scale(r, avail_space as f32)? as i32,
            Self::Constant(c) => c,
        })
    }
}

/// This is a relative offset that offsets an element
/// by a percentage of the size of the available space.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone)]
pub struct RelativeOffset {
    pub x: Value,
    pub y: Value,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Copy, Clone)]
pub struct Offset<T: Copy> {
    pub x: T,
    pub y: T,
}

impl<T: PartialOrd + Copy> Offset<T> {
    pub fn new(w: T, h: T) -> Self {
        Self { x: w, y: h }
    }

    #[allow(dead_code)]
    pub fn union(&mut self, other: &Self) {
        self.x = utils::partial_max(self.x, other.x);
        self.y = utils::partial_max(self.y, other.y);
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct Size<T: Copy> {
    pub width: T,
    pub height: T,
}

impl<T: PartialOrd + Copy> Size<T> {
    pub fn new(w: T, h: T) -> Self {
        Self {
            width: w,
            height: h,
        }
    }
    pub fn union(&mut self, other: &Self) {
        self.width = utils::partial_max(self.width, other.width);
        self.height = utils::partial_max(self.height, other.height);
    }
}

impl From<Size<u32>> for Size<i32> {
    fn from(item: Size<u32>) -> Self {
        Self {
            width: item.width as i32,
            height: item.height as i32,
        }
    }
}

/// The boundary behavior of the edges of a box. True
/// if scrolling is allowed on that axis in this box.
#[derive(Debug)]
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

/// This DOM node defines a named EventHandler
/// to call, along with a set of arguments to pass
/// to the handler when it is run. This is a generic
/// callback definition, and may be attached to many
/// locations in a scene.
///
/// The name field references the named callback that
/// the application will define. The application creates
/// a list of name/EventHandler pairs that it hands to Dakota
/// during initialization that will have their `handle` methods
/// called when the event's condition is met.
///
/// This node is really just a instance of an event handler.
/// It describes what handler to call and a set of arguments
/// to pass.
#[derive(Debug, Clone)]
pub struct Event {
    pub groups: Vec<String>,
    pub id: Option<String>,
    pub args: Arc<Vec<String>>,
}

/// These are global window events that will be defined once. Events
/// taking places on Elements may have Element granularity, but this
/// set of events handles global changes like window resizing, redraw,
/// fullscreen, etc.
#[derive(Default, Debug, Clone)]
pub struct WindowEvents {
    pub resize: Option<Event>,
    pub redraw_complete: Option<Event>,
    pub closed: Option<Event>,
}

/// A description of the typeface and size of the
/// font to use for this text block
#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    /// This is a unique name for this font definition
    pub name: String,
    /// This is the name of the font to use (for example Inconsolata)
    pub font_name: String,
    pub pixel_size: u32,
    pub color: Option<Color>,
}

/// A run of characters of the same format type
#[derive(Debug, Clone)]
pub struct TextRun {
    pub value: String,
    pub cache: Option<Vec<CachedChar>>,
}

/// Represents a contiguous run of similarly formatted text.
///
/// An item is something like a paragraph, or a sentence that is bolded. It will
/// consist of a run of characters that share this format.
#[derive(Debug, Clone)]
#[allow(non_camel_case_types)]
pub enum TextItem {
    p(TextRun),
    b(TextRun),
}

/// Represnts a collection of text items
///
/// Items are assembled here into paragraphs of mixed fonts and formats. This
/// tracks one big "block" of text.
#[derive(Debug, Clone)]
pub struct Text {
    pub items: Vec<TextItem>,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub title: String,
    pub size: Option<(u32, u32)>,
    pub events: WindowEvents,
}

#[derive(Debug, Clone)]
pub struct DakotaDOM {
    pub version: String,
    pub window: Window,
    pub root_element: DakotaId,
}
