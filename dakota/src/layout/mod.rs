/// Dakota Layout engine
///
/// This transforms the Dakota Elements created by the user
/// into a representation suitable for drawing. Specifically,
/// this lowers the Elements into LayoutNodes, which are then
/// used to create Thundr Surfaces for drawing commands.
///
/// Austin Shafer - 2024
extern crate regex;
use regex::Regex;
use std::ops::DerefMut;

use crate::font::*;
use crate::{dom, Dakota, DakotaId, Result};
use utils::{anyhow, log, Context};

#[cfg(test)]
mod tests;

fn regex_trim_excess_space(str: &String) -> String {
    let re = Regex::new(r"\s+").unwrap();
    let trimmed = re.replace_all(str, " ");
    trimmed.to_string()
}

/// Used for tracking layout of children
struct TileInfo {
    /// The latest position we have marched horizontally
    /// while laying children.
    t_last_x: u32,
    /// Same as last width, but in the Y axis.
    t_last_y: u32,
    /// The last known greatest height. This is what the next
    /// height will be when a line overflows.
    t_greatest_y: u32,
}

/// This is the available space for a layout calculation.
/// this handles the number of children sharing the space, the
/// available size
#[derive(Debug, Clone)]
pub struct LayoutSpace {
    /// This is essentially the width of the parent container
    pub avail_width: i32,
    /// This is essentially the height of the parent container
    pub avail_height: i32,
}

/// The elements of the layout tree.
/// This will be constructed from the Elements in the DOM
#[derive(Debug, Clone)]
pub(crate) struct LayoutNode {
    /// Is this element a glyph subsurface. If so it is one character
    /// in a block of text. This is really an index into the font.
    pub l_glyph_id: Option<DakotaId>,
    /// True if the dakota file specified an offset for this el
    pub l_offset_specified: bool,
    pub l_offset: dom::Offset<i32>,
    pub l_size: dom::Size<i32>,
    /// Ids of the children that this layout node has
    pub l_children: Vec<DakotaId>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            l_glyph_id: None,
            l_offset_specified: false,
            l_offset: dom::Offset::new(0, 0),
            l_size: dom::Size::new(0, 0),
            l_children: Vec::with_capacity(0),
        }
    }
}

impl LayoutNode {
    fn new(glyph_id: Option<DakotaId>, off: dom::Offset<i32>, size: dom::Size<i32>) -> Self {
        Self {
            l_glyph_id: glyph_id,
            l_offset_specified: false,
            l_offset: off,
            l_size: size,
            l_children: Vec::with_capacity(0),
        }
    }

    fn add_child(&mut self, other: ll::Entity) {
        self.l_children.push(other);
    }
}

/// LayoutTransaction
///
/// This transaction allows the layout engine to have a consistent,
/// read-only view of the state while it is recalculating the sizes of
/// the LayoutNodes
///
/// These fields correspond to the identically named variants in Dakota.
pub(crate) struct LayoutTransaction<'a> {
    lt_ecs_inst: ll::Instance,
    lt_resources: ll::Snapshot<'a, DakotaId>,
    lt_resource_thundr_image: ll::Snapshot<'a, th::Image>,
    lt_resource_color: ll::Snapshot<'a, dom::Color>,
    lt_fonts: ll::Snapshot<'a, dom::Font>,
    lt_text_font: ll::Snapshot<'a, DakotaId>,
    lt_texts: ll::Snapshot<'a, dom::Text>,
    lt_default_font_inst: DakotaId,
    lt_glyphs: ll::Snapshot<'a, Glyph>,
    lt_viewports: ll::Snapshot<'a, th::Viewport>,
    lt_layout_nodes: ll::Snapshot<'a, LayoutNode>,
    lt_contents: ll::Snapshot<'a, dom::Content>,
    lt_offsets: ll::Snapshot<'a, dom::RelativeOffset>,
    lt_widths: ll::Snapshot<'a, dom::Value>,
    lt_heights: ll::Snapshot<'a, dom::Value>,
    lt_children: ll::Snapshot<'a, Vec<DakotaId>>,
    lt_font_instances: &'a mut Vec<(dom::Font, FontInstance)>,
    lt_thund: &'a mut th::Thundr,
}

impl<'a> LayoutTransaction<'a> {
    /// Commit this transaction
    fn commit(&mut self) {
        self.lt_resources.precommit();
        self.lt_resource_thundr_image.precommit();
        self.lt_resource_color.precommit();
        self.lt_fonts.precommit();
        self.lt_text_font.precommit();
        self.lt_texts.precommit();
        self.lt_glyphs.precommit();
        self.lt_viewports.precommit();
        self.lt_layout_nodes.precommit();
        self.lt_contents.precommit();
        self.lt_widths.precommit();
        self.lt_heights.precommit();
        self.lt_offsets.precommit();
        self.lt_children.precommit();

        // Now do the actual commit
        self.lt_resources.commit();
        self.lt_resource_thundr_image.commit();
        self.lt_resource_color.commit();
        self.lt_fonts.commit();
        self.lt_text_font.commit();
        self.lt_texts.commit();
        self.lt_glyphs.commit();
        self.lt_viewports.commit();
        self.lt_layout_nodes.commit();
        self.lt_contents.commit();
        self.lt_widths.commit();
        self.lt_heights.commit();
        self.lt_offsets.commit();
        self.lt_children.commit();
    }

    /// Helper to get the Font Instance for a particular element
    ///
    /// This will choose the default font (including size) if none
    /// has been assigned.
    pub(crate) fn get_font_id_for_el(&self, el: &DakotaId) -> DakotaId {
        match self.lt_text_font.get(el) {
            Some(f) => f.clone(),
            None => self.lt_default_font_inst.clone(),
        }
    }

    /// Get the final size to use as an offset into the
    /// parent space. This takes care of handling the relative
    /// proportional offset size
    pub fn get_final_offset(&self, el: &DakotaId, space: &LayoutSpace) -> Result<dom::Offset<i32>> {
        if let Some(offset) = self.lt_offsets.get(el) {
            Ok(dom::Offset::new(
                offset.x.get_value(space.avail_width)?,
                offset.y.get_value(space.avail_height)?,
            ))
        } else {
            // If no offset was specified use (0, 0)
            let default_offset = dom::Offset {
                x: dom::Value::Constant(0),
                y: dom::Value::Constant(0),
            };

            Ok(dom::Offset::new(
                default_offset.x.get_value(space.avail_width)?,
                default_offset.y.get_value(space.avail_height)?,
            ))
        }
    }

    pub fn get_default_size_val(
        &self,
        avail_space: i32,
        resource_size: Option<u32>,
        val: Option<dom::Value>,
    ) -> Result<u32> {
        if let Some(size) = val {
            Ok(size.get_value(avail_space)? as u32)
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
            Ok(avail_space as u32)
        }
    }

    /// Get the default starting size to use within the parent space.
    ///
    /// This either returns the size set by the user, otherwise the size of the image
    /// resource assigned, otherwise the size of the parent space.
    pub fn get_default_size(&self, el: &DakotaId, space: &LayoutSpace) -> Result<dom::Size<u32>> {
        let get_image_size = |is_width| match self.lt_resources.get(el).as_deref().clone() {
            Some(res) => self
                .lt_resource_thundr_image
                .get(&res)
                .map(|image| match is_width {
                    true => image.get_size().0,
                    false => image.get_size().1,
                }),
            None => None,
        };

        let width = self.get_default_size_val(
            space.avail_width,
            get_image_size(true),
            self.lt_widths.get(el).map(|val| *val),
        )?;
        let height = self.get_default_size_val(
            space.avail_height,
            get_image_size(false),
            self.lt_heights.get(el).map(|val| *val),
        )?;

        Ok(dom::Size::new(width, height))
    }

    fn get_child_size(&self, el: &DakotaId, is_width: bool, size: u32) -> u32 {
        // First adjust by the size of this element
        let el_size = self.lt_layout_nodes.get(&el).unwrap();
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
    pub fn get_final_size(&self, el: &DakotaId, space: &LayoutSpace) -> Result<dom::Size<u32>> {
        let mut ret = self.get_default_size(el, space)?;
        let mut is_image_resource = false;
        if let Some(res) = self.lt_resources.get(el) {
            if self.lt_resource_thundr_image.get(&res).is_some() {
                is_image_resource = true;
            }
        }

        let needs_size_to_child = !self.lt_viewports.get(el).is_some()
            && !is_image_resource
            && self.lt_layout_nodes.get(el).unwrap().l_children.len() > 0;

        // Does the content have a width/height assigned
        //
        // If one of these dimensions was assigned, then we do not want to shrink this element
        // by that amount since the alignment was based on the original size.
        let (content_has_width, content_has_height) = match self.lt_contents.get(el) {
            Some(cont) => (
                self.lt_widths.get(&cont.el).is_some(),
                self.lt_heights.get(&cont.el).is_some(),
            ),
            None => (false, false),
        };

        // If no size was specified by the user and no image has been assigned then we
        // will limit this element to the size of its children if there are any
        if self.lt_widths.get(el).is_none() && needs_size_to_child && !content_has_width {
            ret.width = 0;
            for i in 0..self.lt_layout_nodes.get(el).unwrap().l_children.len() {
                let child_id = self.lt_layout_nodes.get(el).unwrap().l_children[i].clone();

                ret.width = self.get_child_size(&child_id, true, ret.width);
            }
        }

        if self.lt_heights.get(el).is_none() && needs_size_to_child && !content_has_height {
            ret.height = 0;
            for i in 0..self.lt_layout_nodes.get(el).unwrap().l_children.len() {
                let child_id = self.lt_layout_nodes.get(el).unwrap().l_children[i].clone();

                ret.height = self.get_child_size(&child_id, false, ret.height);
            }
        }

        return Ok(ret);
    }

    /// Calculate size and position of centered content.
    ///
    ///
    /// This box has centered content.
    /// We should either recurse the child box or calculate the
    /// size based on the centered resource.
    fn calculate_sizes_content(&mut self, el: &DakotaId, space: &LayoutSpace) -> Result<()> {
        log::debug!("Calculating content size");
        let child_id = self.lt_contents.get(el).unwrap().el.clone();

        self.calculate_sizes(&child_id, Some(el), &space)?;
        let parent_size = self.lt_layout_nodes.get(el).unwrap().l_size;

        {
            let mut child_size_raw = self.lt_layout_nodes.get_mut(&child_id).unwrap();
            let child_size = child_size_raw.deref_mut();
            // At this point the size of the is calculated
            // and we can determine the offset. We want to center the
            // box, so that's the center point of the parent minus
            // half the size of the child.
            //
            // The child size should have already been clipped to the available space
            child_size.l_offset.x =
                utils::partial_max((parent_size.width / 2) - (child_size.l_size.width / 2), 0);
            child_size.l_offset.y =
                utils::partial_max((parent_size.height / 2) - (child_size.l_size.height / 2), 0);
        }

        let node = self.lt_layout_nodes.get_mut(el).unwrap();
        node.add_child(child_id.clone());
        Ok(())
    }

    /// Recursively calls calculate_sizes on all children of el
    ///
    /// This does all the work to get information about children for a particular
    /// element. After having the children calculate their sizes, it will assign
    /// them layout positions within el. This will fill from left to right by
    /// default, wrapping below if necessary.
    ///
    /// `grandparent` is avialable when appropriate and allows children to
    /// reference two levels above, for use when not bounding size by the
    /// current element.
    fn calculate_sizes_children(&mut self, el: &DakotaId, space: &mut LayoutSpace) -> Result<()> {
        log::debug!("Calculating children size");
        // TODO: do vertical wrapping too
        let mut tile_info = TileInfo {
            t_last_x: 0,
            t_last_y: 0,
            t_greatest_y: 0,
        };

        let child_count = self
            .lt_children
            .get(el)
            .ok_or(anyhow!("Expected children"))?
            .len();

        for i in 0..child_count {
            let child_id = self
                .lt_children
                .get(el)
                .ok_or(anyhow!("Expected children"))?[i]
                .clone();
            self.calculate_sizes(&child_id, Some(el), &space)?;

            // ----- adjust child position ----
            {
                let child_size = self.lt_layout_nodes.get_mut(&child_id).unwrap();

                // now the child size has been made, but it still needs to find
                // the proper position inside the parent container. If the child
                // already had an offset specified, it is "out of the loop", and
                // doesn't get used for pretty formatting, it just gets placed
                // wherever.
                if !child_size.l_offset_specified {
                    // if this element exceeds the horizontal or vertical space, set it on a
                    // new line
                    if tile_info.t_last_x as i32 + child_size.l_size.width > space.avail_width
                        || tile_info.t_last_y as i32 + child_size.l_size.height > space.avail_height
                    {
                        tile_info.t_last_x = 0;
                        tile_info.t_last_y = tile_info.t_greatest_y;
                    }

                    child_size.l_offset = dom::Offset {
                        x: tile_info.t_last_x as i32,
                        y: tile_info.t_last_y as i32,
                    };

                    // now we need to update the space that we have seen children
                    // occupy, so we know where to place the next children in the
                    // tiling formation.
                    tile_info.t_last_x += child_size.l_size.width as u32;
                    tile_info.t_greatest_y = std::cmp::max(
                        tile_info.t_greatest_y,
                        tile_info.t_last_y + child_size.l_size.height as u32,
                    );
                }
            }

            self.lt_layout_nodes
                .get_mut(el)
                .unwrap()
                .add_child(child_id.clone());
        }

        Ok(())
    }

    /// Calculate the sizes and handle the current element
    ///
    /// 1. If it has a size assigned, that is the final size, all children
    /// will be clipped or scrolled inside that window.
    /// 2. If no size is assigned, and we are limited in the amount of space
    /// we have, then the size is available_space
    /// 3. No size and no bounds means we are inside of a scrolling arena, and
    /// we should grow this box to hold all of its children.
    ///
    /// The final size of this element will be amended after all child content
    /// has been calculated.
    fn calculate_sizes_el(
        &mut self,
        el: &DakotaId,
        _parent: Option<&DakotaId>,
        space: &LayoutSpace,
    ) -> Result<()> {
        let mut node = LayoutNode::new(None, dom::Offset::new(0, 0), dom::Size::new(0, 0));

        node.l_offset_specified = self.lt_offsets.get(el).is_some();
        node.l_offset = self
            .get_final_offset(el, &space)
            .context("Failed to calculate offset size of Element")?
            .into();

        node.l_size = self.get_default_size(el, space)?.into();
        // Bounds will be checked in caller to see if the parent needs to be
        // marked as a viewport.

        log::debug!("Offset of element is {:?}", node.l_offset);
        log::debug!("Size of element is {:?}", node.l_size);
        self.lt_layout_nodes.take(el);
        self.lt_layout_nodes.set(el, node);
        Ok(())
    }

    /// Returns true if the node is of a type that guarantees it cannot have
    /// child elements.
    ///
    /// This most notably happens with text elements. Should match Dakota's
    /// version of this
    pub(crate) fn node_can_have_children(
        &self,
        texts: &ll::Snapshot<dom::Text>,
        id: &DakotaId,
    ) -> bool {
        !texts.get(id).is_some()
    }

    /// Handles creating LayoutNodes for every glyph in a passage
    ///
    /// This is the handler for the text field in the dakota file
    fn calculate_sizes_text(&mut self, el: &DakotaId) -> Result<()> {
        if !self.node_can_have_children(&self.lt_texts, el) && self.lt_children.get(el).is_some() {
            return Err(anyhow!("Text Elements cannot have children"));
        }

        let font_id = self.get_font_id_for_el(el);
        let font = self.lt_fonts.get(&font_id).unwrap();
        let font_inst = &mut self
            .lt_font_instances
            .iter_mut()
            .find(|(f, _)| *f == *font)
            .expect("Could not find FontInstance")
            .1;

        let text = self.lt_texts.get_mut(el).unwrap();
        let line_space = font_inst.get_vertical_line_spacing();

        // This is how far we have advanced on a line
        // Go down by one line space before writing the first line. This deals
        // with the problem that ft/hb want to index by the bottom left corner
        // and all my stuff wants to index from the top left corner. Without this
        // text starts being written "above" the element it is assigned to.
        let mut cursor = {
            let node = self.lt_layout_nodes.get(el).unwrap();
            Cursor {
                c_i: 0,
                c_x: 0,
                c_y: line_space,
                c_min: node.l_offset.x,
                c_max: node.l_offset.x + node.l_size.width,
            }
        };

        log::debug!("Calculating text size");
        log::debug!("{:?}", cursor);

        // Trim out newlines and tabs. Styling is done with entries in the DOM, not
        // through text formatting in the dakota file.
        for item in text.items.iter_mut() {
            match item {
                dom::TextItem::p(run) | dom::TextItem::b(run) => {
                    if run.cache.is_none() {
                        // TODO: we can get the available height from above, pass it to a font instance
                        // and create layout nodes for all character surfaces.
                        let mut trim = regex_trim_excess_space(&run.value);
                        // TODO: Find a better way of adding space around itemized runs
                        trim.push_str(" ");

                        // This must be called to initialize the glyphs before we do
                        // the layout and line splitting.
                        run.cache = Some(font_inst.initialize_cached_chars(
                            &mut self.lt_thund,
                            &mut self.lt_ecs_inst,
                            &mut self.lt_glyphs,
                            &trim,
                        ));
                    }

                    // We need to take references to everything at once before the closure
                    // so that the borrow checker can see we aren't trying to reference all
                    // of self
                    let layouts = &mut self.lt_layout_nodes;
                    let text_fonts = &mut self.lt_text_font;
                    let glyphs = &mut self.lt_glyphs;

                    // Record text locations
                    // We will create a whole bunch of sub-nodes which will be assigned
                    // glyph ids. These ids will later be used to get surfaces for.
                    font_inst.layout_text(
                        &mut self.lt_thund,
                        &mut cursor,
                        run.cache.as_ref().unwrap(),
                        &mut |_inst: &mut FontInstance, _thund, curse, ch| {
                            // --- calculate sizes for the character surfaces ---
                            let size = glyphs.get(&ch.glyph_id).unwrap().g_bitmap_size;

                            let child_size = LayoutNode::new(
                                Some(ch.glyph_id.clone()),
                                dom::Offset {
                                    x: curse.c_x + ch.offset.0,
                                    y: curse.c_y + ch.offset.1,
                                },
                                dom::Size {
                                    width: size.0,
                                    height: size.1,
                                },
                            );
                            log::info!("Character size is {:?}", size);

                            {
                                let node = layouts.get_mut(el).unwrap();
                                // What we have done here is create a "fake" element (fake since
                                // the user didn't specify it) that represents a glyph.
                                node.add_child(ch.node.clone());
                            }

                            layouts.set(&ch.node, child_size);
                            // We need to assign a font here or else later when we
                            // create thundr surfaces for these glyphs we will index
                            // the wrong font using this glyph_id
                            text_fonts.set(&ch.node, font_id.clone());
                        },
                    );
                }
            }
        }

        Ok(())
    }

    /// Create a layout tree of boxes.
    ///
    /// This gives all the layout information for where we should place
    /// thundr surfaces.
    ///
    /// This will add boxes to the box array, but will also return the
    /// box signifying the final size. By handing the size up the recursion
    /// chain, each box can see the sizes of its children as they are
    /// created, and can set its final size accordingly. This should prevent
    /// us from having to do more recursion later since everything is calculated
    /// now.
    pub(crate) fn calculate_sizes(
        &mut self,
        el: &DakotaId,
        parent: Option<&DakotaId>,
        space: &LayoutSpace,
    ) -> Result<()> {
        // ------------------------------------------
        // HANDLE THIS ELEMENT
        // ------------------------------------------
        // Must be done before anything referencing the size of this element
        self.calculate_sizes_el(el, parent, space)
            .context("Layout Tree Calculation: processing element")?;

        // This space is what the children/content will use
        // it is restricted in size to this element (their parent)
        let mut child_space = {
            let node = self.lt_layout_nodes.get(el).unwrap();
            LayoutSpace {
                avail_width: node.l_size.width,
                avail_height: node.l_size.height,
            }
        };

        // ------------------------------------------
        // HANDLE TEXT
        // ------------------------------------------
        // We do this after handling the size of the current element so that we
        // can know what width we have available to fill in with text.
        if self.lt_texts.get(el).is_some() {
            self.calculate_sizes_text(el)?;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if self.lt_children.get(el).is_some() && self.lt_children.get(el).unwrap().len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //

            self.calculate_sizes_children(el, &mut child_space)
                .context("Layout Tree Calculation: processing children of element")?;
        }

        if self.lt_contents.get(el).is_some() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            self.calculate_sizes_content(el, &child_space)
                .context("Layout Tree Calculation: processing centered content of element")?;
        }

        // Update the size of this element after calculating the content
        let final_size = self.get_final_size(el, space)?.into();
        self.lt_layout_nodes.get_mut(el).unwrap().l_size = final_size;

        return Ok(());
    }
}

impl Dakota {
    /// Draw the entire scene
    ///
    /// This starts at the root viewport and draws all child viewports
    pub(crate) fn layout(&mut self, root_node: &DakotaId) -> Result<()> {
        let mut trans = LayoutTransaction {
            lt_ecs_inst: self.d_ecs_inst.clone(),
            lt_resources: self.d_resources.snapshot(),
            lt_resource_thundr_image: self.d_resource_thundr_image.snapshot(),
            lt_resource_color: self.d_resource_color.snapshot(),
            lt_fonts: self.d_fonts.snapshot(),
            lt_text_font: self.d_text_font.snapshot(),
            lt_texts: self.d_texts.snapshot(),
            lt_default_font_inst: self.d_default_font_inst.clone(),
            lt_glyphs: self.d_glyphs.snapshot(),
            lt_viewports: self.d_viewports.snapshot(),
            lt_layout_nodes: self.d_layout_nodes.snapshot(),
            lt_contents: self.d_contents.snapshot(),
            lt_widths: self.d_widths.snapshot(),
            lt_heights: self.d_heights.snapshot(),
            lt_offsets: self.d_offsets.snapshot(),
            lt_children: self.d_children.snapshot(),
            lt_font_instances: &mut self.d_font_instances,
            lt_thund: &mut self.d_thund,
        };

        trans.calculate_sizes(
            root_node,
            None, // no parent since we are the root node
            &LayoutSpace {
                avail_width: self.d_window_dims.unwrap().0 as i32, // available width
                avail_height: self.d_window_dims.unwrap().1 as i32, // available height
            },
        )?;
        trans.commit();

        Ok(())
    }
}
