/// Define a DOM heirarchy and dakota data file format
///
/// Austin Shafer - 2022
use crate::font::CachedChar;
use crate::utils::{anyhow, Result};
use crate::{Dakota, DakotaId, LayoutSpace};

use std::cmp::{Ord, PartialOrd};
use std::rc::Rc;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Format {
    ARGB8888,
}

impl Format {
    pub fn get_size(&self) -> usize {
        match self {
            Format::ARGB8888 => 4,
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
#[derive(Copy, Clone, Debug)]
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

#[derive(Debug)]
pub struct Content {
    pub el: DakotaId,
}

/// This is a relative value that modifies an element
/// by a percentage of the size of the available space.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone)]
pub struct Relative {
    val: f32,
}

impl Relative {
    pub fn new(v: f32) -> Self {
        assert!(v >= 0.0 && v <= 1.0);
        Self { val: v }
    }

    pub fn scale(&self, val: f32) -> Result<u32> {
        if !(self.val >= 0.0 && self.val <= 1.0) {
            return Err(anyhow!(
                "Element.relativeOffset should use values in the range (0.0, 1.0)"
            ));
        }
        Ok((val * self.val) as u32)
    }
}

#[derive(Debug, PartialEq, PartialOrd, Copy, Clone)]
pub struct Constant {
    val: i32,
}

impl Constant {
    pub fn new(val: i32) -> Self {
        Self { val: val }
    }
}

/// Represents a possibly relative value. This will
/// either be a f32 scaling value or a constant size
/// u32.
#[derive(Debug, PartialEq, PartialOrd, Copy, Clone)]
pub enum Value {
    Relative(Relative),
    Constant(Constant),
}

impl Value {
    pub fn get_value(&self, avail_space: f32) -> Result<i32> {
        Ok(match self {
            Self::Relative(r) => r.scale(avail_space)? as i32,
            Self::Constant(c) => c.val,
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

impl From<Offset<i32>> for Offset<f32> {
    fn from(item: Offset<i32>) -> Self {
        Self {
            x: item.x as f32,
            y: item.y as f32,
        }
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

impl From<Size<u32>> for Size<f32> {
    fn from(item: Size<u32>) -> Self {
        Self {
            width: item.width as f32,
            height: item.height as f32,
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
    pub args: Rc<Vec<String>>,
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
#[derive(Debug, Clone)]
pub struct Font {
    pub name: String,
    pub path: String,
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
#[derive(Debug)]
pub struct Text {
    pub items: Vec<TextItem>,
}

#[derive(Debug, Clone)]
pub struct Window {
    pub title: String,
    pub size: Option<(u32, u32)>,
    pub events: WindowEvents,
}

#[derive(Debug)]
pub struct DakotaDOM {
    pub version: String,
    pub window: Window,
    pub root_element: DakotaId,
}

impl Dakota {
    /// Get the final size to use as an offset into the
    /// parent space. This takes care of handling the relative
    /// proportional offset size
    pub fn get_final_offset(&self, el: &DakotaId, space: &LayoutSpace) -> Result<Offset<i32>> {
        if let Some(offset) = self.d_offsets.get(el) {
            Ok(Offset::new(
                offset.x.get_value(space.avail_width)?,
                offset.y.get_value(space.avail_height)?,
            ))
        } else {
            // If no offset was specified use (0, 0)
            let default_offset = Offset {
                x: Value::Constant(Constant { val: 0 }),
                y: Value::Constant(Constant { val: 0 }),
            };

            Ok(Offset::new(
                default_offset.x.get_value(space.avail_width)?,
                default_offset.y.get_value(space.avail_height)?,
            ))
        }
    }

    pub fn get_default_size_val(
        &self,
        avail_space: u32,
        resource_size: Option<u32>,
        val: Option<Value>,
    ) -> Result<u32> {
        if let Some(size) = val {
            Ok(size.get_value(avail_space as f32)? as u32)
        } else {
            // If no size was provided but an image resource has been assigned, then
            // size this element to the resource. Text resource sizing will be
            // handled in calculate_sizes_text.
            //
            // If there are children and no resource was provided, then we will
            // limit this node to the size of the children later after processing
            // all of them.
            //
            // TODO: use LayoutSpace for all sizing decisions, then calculate the
            // final element size here, sizing to children if needed?
            if let Some(size) = resource_size {
                return Ok(size);
            }

            // If no size was specified then this defaults to the size of its container
            Ok(avail_space)
        }
    }

    /// Get the default starting size to use within the parent space.
    ///
    /// This either returns the size set by the user, otherwise the size of the image
    /// resource assigned, otherwise the size of the parent space.
    pub fn get_default_size(&self, el: &DakotaId, space: &LayoutSpace) -> Result<Size<u32>> {
        let get_image_size = |is_width| match self.d_resources.get(el).as_deref().clone() {
            Some(res) => self
                .d_resource_thundr_image
                .get(&res)
                .map(|image| match is_width {
                    true => image.get_size().0,
                    false => image.get_size().1,
                }),
            None => None,
        };

        let width = self.get_default_size_val(
            space.avail_width as u32,
            get_image_size(true),
            self.d_widths.get(el).map(|val| *val),
        )?;
        let height = self.get_default_size_val(
            space.avail_height as u32,
            get_image_size(false),
            self.d_heights.get(el).map(|val| *val),
        )?;

        Ok(Size::new(width, height))
    }

    fn get_child_size(&self, el: &DakotaId, is_width: bool, size: u32) -> u32 {
        // First adjust by the size of this element
        let el_size = self.d_layout_nodes.get(&el).unwrap();
        size.max(match is_width {
            true => el_size.l_offset.x as u32 + el_size.l_size.width as u32,
            false => el_size.l_offset.y as u32 + el_size.l_size.height as u32,
        })
    }

    /// Get the final size to use within the parent space.
    ///
    /// This is the same as the (original) default size, unless the following conditions are
    /// met:
    /// - no size was set by the user
    /// - no image resource is assigned
    /// - element does not have any positioned content
    ///
    /// The above criterea are evaluated per-dimension with respect to width/height. It is
    /// possible that one dimension is grown and the other is not.
    ///
    /// If those conditions are met, then the element will be shrunk/grown to contain all
    /// child elements.
    pub fn get_final_size(&self, el: &DakotaId, space: &LayoutSpace) -> Result<Size<u32>> {
        let mut ret = self.get_default_size(el, space)?;
        let mut is_image_resource = false;
        if let Some(res) = self.d_resources.get(el) {
            if self.d_resource_thundr_image.get(&res).is_some() {
                is_image_resource = true;
            }
        }

        let needs_size_to_child = !self.d_layout_nodes.get(el).unwrap().l_is_viewport
            && !is_image_resource
            && self.d_layout_nodes.get(el).unwrap().l_children.len() > 0;

        // Does the content have a width/height assigned
        //
        // If one of these dimensions was assigned, then we do not want to shrink this element
        // by that amount since the alignment was based on the original size.
        let (content_has_width, content_has_height) = match self.d_contents.get(el) {
            Some(cont) => (
                self.d_widths.get(&cont.el).is_some(),
                self.d_heights.get(&cont.el).is_some(),
            ),
            None => (false, false),
        };

        // If no size was specified by the user and no image has been assigned then we
        // will limit this element to the size of its children if there are any
        if self.d_widths.get(el).is_none() && needs_size_to_child && !content_has_width {
            ret.width = 0;
            for i in 0..self.d_layout_nodes.get(el).unwrap().l_children.len() {
                let child_id = self.d_layout_nodes.get(el).unwrap().l_children[i].clone();

                ret.width = self.get_child_size(&child_id, true, ret.width);
            }
        }

        if self.d_heights.get(el).is_none() && needs_size_to_child && !content_has_height {
            ret.height = 0;
            for i in 0..self.d_layout_nodes.get(el).unwrap().l_children.len() {
                let child_id = self.d_layout_nodes.get(el).unwrap().l_children[i].clone();

                ret.height = self.get_child_size(&child_id, false, ret.height);
            }
        }

        return Ok(ret);
    }
}
