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
#[derive(Debug)]
pub(crate) struct LayoutNode {
    /// Is this element a glyph subsurface. If so it is one character
    /// in a block of text. This is really an index into the font.
    pub l_glyph_id: Option<u16>,
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
    fn new(glyph_id: Option<u16>, off: dom::Offset<i32>, size: dom::Size<i32>) -> Self {
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

impl Dakota {
    /// Calculate size and position of centered content.
    ///
    ///
    /// This box has centered content.
    /// We should either recurse the child box or calculate the
    /// size based on the centered resource.
    fn calculate_sizes_content(&mut self, el: &DakotaId, space: &LayoutSpace) -> Result<()> {
        log::debug!("Calculating content size");
        let child_id = self.d_contents.get(el).unwrap().el.clone();

        self.calculate_sizes(&child_id, Some(el), &space)?;
        let parent_size = self.d_layout_nodes.get(el).unwrap().l_size;

        {
            let mut child_size_raw = self.d_layout_nodes.get_mut(&child_id).unwrap();
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

        let mut node = self.d_layout_nodes.get_mut(el).unwrap();
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
            .d_children
            .get(el)
            .ok_or(anyhow!("Expected children"))?
            .len();

        for i in 0..child_count {
            let child_id = self
                .d_children
                .get(el)
                .ok_or(anyhow!("Expected children"))?[i]
                .clone();
            self.calculate_sizes(&child_id, Some(el), &space)?;

            // ----- adjust child position ----
            {
                let mut child_size = self.d_layout_nodes.get_mut(&child_id).unwrap();

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

            self.d_layout_nodes
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

        node.l_offset_specified = self.d_offsets.get(el).is_some();
        node.l_offset = self
            .get_final_offset(el, &space)
            .context("Failed to calculate offset size of Element")?
            .into();

        node.l_size = self.get_default_size(el, space)?.into();
        // Bounds will be checked in caller to see if the parent needs to be
        // marked as a viewport.

        log::debug!("Offset of element is {:?}", node.l_offset);
        log::debug!("Size of element is {:?}", node.l_size);
        self.d_layout_nodes.take(el);
        self.d_layout_nodes.set(el, node);
        Ok(())
    }

    /// Helper to get the Font Instance for a particular element
    ///
    /// This will choose the default font (including size) if none
    /// has been assigned.
    pub(crate) fn get_font_id_for_el(&self, el: &DakotaId) -> DakotaId {
        match self.d_text_font.get(el) {
            Some(f) => f.clone(),
            None => self.d_default_font_inst.clone(),
        }
    }

    /// Handles creating LayoutNodes for every glyph in a passage
    ///
    /// This is the handler for the text field in the dakota file
    fn calculate_sizes_text(&mut self, el: &DakotaId) -> Result<()> {
        let font_id = self.get_font_id_for_el(el);
        let font = self.d_fonts.get(&font_id).unwrap();
        let font_inst = &mut self
            .d_font_instances
            .iter_mut()
            .find(|(f, _)| *f == *font)
            .expect("Could not find FontInstance")
            .1;

        let mut text = self.d_texts.get_mut(el).unwrap();
        let line_space = font_inst.get_vertical_line_spacing();

        // This is how far we have advanced on a line
        // Go down by one line space before writing the first line. This deals
        // with the problem that ft/hb want to index by the bottom left corner
        // and all my stuff wants to index from the top left corner. Without this
        // text starts being written "above" the element it is assigned to.
        let mut cursor = {
            let node = self.d_layout_nodes.get(el).unwrap();
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
                    // We need to take references to everything at once before the closure
                    // so that the borrow checker can see we aren't trying to reference all
                    // of self
                    let layouts = &mut self.d_layout_nodes;
                    let text_fonts = &mut self.d_text_font;

                    if run.cache.is_none() {
                        // TODO: we can get the available height from above, pass it to a font instance
                        // and create layout nodes for all character surfaces.
                        let mut trim = regex_trim_excess_space(&run.value);
                        // TODO: Find a better way of adding space around itemized runs
                        trim.push_str(" ");

                        run.cache = Some(font_inst.initialize_cached_chars(
                            &mut self.d_thund,
                            &mut self.d_ecs_inst,
                            &trim,
                        ));
                    }

                    // Record text locations
                    // We will create a whole bunch of sub-nodes which will be assigned
                    // glyph ids. These ids will later be used to get surfaces for.
                    font_inst.layout_text(
                        &mut self.d_thund,
                        &mut cursor,
                        run.cache.as_ref().unwrap(),
                        &mut |inst: &mut FontInstance, thund, curse, ch| {
                            // --- calculate sizes for the character surfaces ---
                            let size = inst.get_glyph_thundr_size(thund, ch.glyph_id);

                            let child_size = LayoutNode::new(
                                Some(ch.glyph_id),
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
                                let mut node = layouts.get_mut(el).unwrap();
                                // What we have done here is create a "fake" element (fake since
                                // the user didn't specify it) that represents a glyph.
                                node.add_child(ch.node.clone());
                            }

                            layouts.take(&ch.node);
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
            let node = self.d_layout_nodes.get(el).unwrap();
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
        if self.d_texts.get(el).is_some() {
            self.calculate_sizes_text(el)?;
        }

        // if the box has children, then recurse through them and calculate our
        // box size based on the fill type.
        if self.d_children.get(el).is_some() && self.d_children.get(el).unwrap().len() > 0 {
            // ------------------------------------------
            // CHILDREN
            // ------------------------------------------
            //

            self.calculate_sizes_children(el, &mut child_space)
                .context("Layout Tree Calculation: processing children of element")?;
        }

        if self.d_contents.get(el).is_some() {
            // ------------------------------------------
            // CENTERED CONTENT
            // ------------------------------------------
            self.calculate_sizes_content(el, &child_space)
                .context("Layout Tree Calculation: processing centered content of element")?;
        }

        // Update the size of this element after calculating the content
        let final_size = self.get_final_size(el, space)?.into();
        self.d_layout_nodes.get_mut(el).unwrap().l_size = final_size;

        return Ok(());
    }
}
