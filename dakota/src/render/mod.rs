use crate::font::Glyph;
use crate::layout::LayoutNode;
use crate::{dom, DakotaId, Output, Scene};

/// Dakota Drawing logic
///
/// This splits out the rendering layouut of Dakota, which uses
/// Thundr to draw 2D elements on the surface. This consumes the
/// LayoutNode tree computed ut of Dakota, which uses
/// Thundr to draw 2D elements on the surface. This consumes the
/// LayoutNode tree computed by the layout layer and turns it
/// into Thundr Surfaces, dispatching the draw calls.
use thundr as th;

/// RenderTransaction
///
/// This transaction allows the rendering part of the code to have a consistent,
/// read-only view of the state while it is performing drawing commands.
///
/// These fields correspond to the identically named variants in Dakota.
pub(crate) struct RenderTransaction<'a> {
    rt_resources: ll::Snapshot<'a, DakotaId>,
    rt_resource_thundr_image: ll::Snapshot<'a, th::Image>,
    rt_resource_color: ll::Snapshot<'a, dom::Color>,
    rt_fonts: ll::Snapshot<'a, dom::Font>,
    rt_text_font: ll::Snapshot<'a, DakotaId>,
    rt_default_font_inst: DakotaId,
    rt_glyphs: ll::Snapshot<'a, Glyph>,
    rt_viewports: ll::Snapshot<'a, th::Viewport>,
    rt_layout_nodes: ll::Snapshot<'a, LayoutNode>,
}

impl<'a> RenderTransaction<'a> {
    /// Commit this transaction
    fn commit(&mut self) {
        self.rt_resources.precommit();
        self.rt_resource_thundr_image.precommit();
        self.rt_resource_color.precommit();
        self.rt_fonts.precommit();
        self.rt_text_font.precommit();
        self.rt_glyphs.precommit();
        self.rt_viewports.precommit();
        self.rt_layout_nodes.precommit();

        // Now do actual commit to WAR ids being dropped
        self.rt_resources.commit();
        self.rt_resource_thundr_image.commit();
        self.rt_resource_color.commit();
        self.rt_fonts.commit();
        self.rt_text_font.commit();
        self.rt_glyphs.commit();
        self.rt_viewports.commit();
        self.rt_layout_nodes.commit();
    }

    /// Helper to get a display surface for a glyph.
    pub fn get_thundr_surf_for_glyph(
        &self,
        node: &DakotaId,
        glyph: &Glyph,
        pos: &(i32, i32),
    ) -> th::Surface {
        let mut surf = th::Surface::new(
            th::Rect::new(pos.0, pos.1, glyph.g_bitmap_size.0, glyph.g_bitmap_size.1),
            None,
        );

        let font_id = match self.rt_text_font.get(node) {
            Some(f) => f,
            None => &self.rt_default_font_inst,
        };
        let font = self.rt_fonts.get(&font_id).unwrap();
        if let Some(color) = font.color.as_ref() {
            surf.set_color((color.r, color.g, color.b, color.a));
        }

        return surf;
    }

    /// Populate a display surface with this nodes dimensions and content
    ///
    /// This accepts a base offset to handle child element positioning
    fn get_thundr_surf_for_el(&self, node: &DakotaId, base: (i32, i32)) -> th::Result<th::Surface> {
        let layout = self.rt_layout_nodes.get(node).unwrap();
        let offset = (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);

        // Image/color content will be set later
        let mut surf = if let Some(glyph_id) = layout.l_glyph_id.as_ref() {
            // If this path is hit, then this layout node is really a glyph in a
            // larger block of text. It has been created as a child, and isn't
            // a real element. We ask the font code to give us a surface for
            // it that we can display.
            let glyph = self.rt_glyphs.get(glyph_id).unwrap();
            self.get_thundr_surf_for_glyph(node, glyph, &offset)
        } else {
            th::Surface::new(
                th::Rect::new(
                    offset.0,
                    offset.1,
                    layout.l_size.width,
                    layout.l_size.height,
                ),
                None, // color
            )
        };

        // Handle binding images
        // We need to get the resource's content from our resource map, get
        // the display image for it, and bind it to our new surface.
        if let Some(resource_id) = self.rt_resources.get(node) {
            // Assert that only one content type is set
            let mut content_num = 0;

            if self.rt_resource_thundr_image.get(&resource_id).is_some() {
                content_num += 1;
            }
            if let Some(color) = self.rt_resource_color.get(&resource_id) {
                surf.set_color((color.r, color.g, color.b, color.a));
                content_num += 1;
            }

            assert!(content_num == 1);
        }

        return Ok(surf);
    }

    /// Create a Thundr viewport struct from our dakota Viewport
    ///
    /// This would be straightforward except that we have to clip our viewport
    /// to the size of the parent viewport. This keeps child elements within the
    /// bounds of the parent.
    fn get_display_viewport(
        &self,
        parent: &th::Viewport,
        node: &DakotaId, // child viewport
        base: (i32, i32),
    ) -> Option<th::Viewport> {
        let layout = self.rt_layout_nodes.get(node)?;
        let viewport = self.rt_viewports.get(node)?;

        // We will copy the viewport, and then return a clamped version of it to
        // draw with.
        let mut ret = viewport.clone();

        // If the child is partially scrolled past, then update its offset to
        // zero and limit the size by that amount
        let clamp_to_parent_base = |child_original_size,
                                    child_offset: &mut i32,
                                    child_size: &mut i32,
                                    parent_offset: i32,
                                    parent_size: i32| {
            // The child size is either size reduced by the amount this
            // child is behind the parent, or the size reduced by the amount
            // this child exceeds the parent, or the size
            *child_size = if *child_offset < parent_offset {
                child_original_size - (parent_offset - *child_offset).abs()
            } else if *child_offset + child_original_size > parent_offset + parent_size {
                (parent_offset + parent_size) - *child_offset
            } else {
                child_original_size
            };
            // Now clamp it to the parent's region
            *child_offset = (*child_offset).clamp(parent_offset, parent_offset + parent_size);
        };

        // Update the starting dimensions of the returned viewport
        ret.offset = (
            base.0 as i32 + layout.l_offset.x as i32,
            base.1 as i32 + layout.l_offset.y as i32,
        );
        ret.size = (layout.l_size.width as i32, layout.l_size.height as i32);

        // Clamp it to the parent
        clamp_to_parent_base(
            layout.l_size.width as i32,
            &mut ret.offset.0,
            &mut ret.size.0,
            parent.offset.0,
            parent.size.0,
        );
        clamp_to_parent_base(
            layout.l_size.height as i32,
            &mut ret.offset.1,
            &mut ret.size.1,
            parent.offset.1,
            parent.size.1,
        );

        return Some(ret);
    }

    /// Test if we should skip drawing this node because it is offscreen
    fn is_node_visible(&self, viewport: &th::Viewport, node: &DakotaId, base: (i32, i32)) -> bool {
        let layout = self.rt_layout_nodes.get(node).unwrap();

        // Test that this child is visible before drawing it
        let offset = dom::Offset::new(base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);
        !(offset.x > viewport.offset.0 + viewport.size.0
                    || offset.y > viewport.offset.1 + viewport.size.1
                    // Have we scrolled past this horizontally
                    || (offset.x < 0 && offset.x * -1 > layout.l_size.width)
                    // Have we scrolled past this vertically
                    || (offset.y < 0 && offset.y * -1 > layout.l_size.height))
    }

    /// Test if we should skip drawing this viewport because it is offscreen
    fn is_nodes_viewport_visible(
        &self,
        viewport: &th::Viewport,
        child_viewport: &th::Viewport,
        base: (i32, i32),
    ) -> bool {
        let offset = dom::Offset::new(
            base.0 + child_viewport.offset.0,
            base.1 + child_viewport.offset.1,
        );
        !(offset.x > viewport.offset.0 + viewport.size.0
                    || offset.y > viewport.offset.1 + viewport.size.1
                    // Have we scrolled past this horizontally
                    || (offset.x + child_viewport.size.0 < viewport.offset.0)
                    // Have we scrolled past this vertically
                    || (offset.x + child_viewport.size.1 < viewport.offset.1))
    }

    /// Helper for drawing a single element
    ///
    /// This does not recurse. Will skip drawing this node if it is out of the bounds of
    /// its viewport.
    fn draw_node(
        &self,
        frame: &mut th::FrameRenderer<'a>,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        let surf = self.get_thundr_surf_for_el(node, base)?;

        if !self.is_node_visible(viewport, node, base) {
            return Ok(());
        }

        // Get the image to use for this surface, if we have one
        // This is done separately so that we can avoid cloning the image
        // id. The atomic inc/dec to do this shows up in profiling
        let layout = self.rt_layout_nodes.get(node).unwrap();
        let mut image = None;

        if let Some(glyph_id) = layout.l_glyph_id.as_ref() {
            let glyph = self.rt_glyphs.get(glyph_id).unwrap();
            image = glyph.g_image.as_ref();
        } else if let Some(resource_id) = self.rt_resources.get(node) {
            if let Some(res) = self.rt_resource_thundr_image.get(&resource_id) {
                image = Some(res)
            }
        }

        frame.draw_surface(&surf, image)
    }

    /// Recursively draw node and all of its children
    ///
    /// This does not cross viewport boundaries
    fn draw_node_recurse(
        &self,
        frame: &mut th::FrameRenderer<'a>,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        // If this node is a viewport then update our display viewport
        let new_th_viewport = match self.rt_viewports.get(node).is_some() {
            true => {
                let child_viewport = self.rt_viewports.get(node).unwrap();
                // If this node its viewport is not visible then we know
                // we can skip it and all children as they must be clipped within
                if !self.is_node_visible(viewport, node, base)
                    || !self.is_nodes_viewport_visible(viewport, child_viewport, base)
                {
                    return Ok(());
                }

                // Set Thundr's currently in use viewport
                let th_viewport = self.get_display_viewport(viewport, node, base).unwrap();
                frame.set_viewport(&th_viewport)?;

                Some(th_viewport)
            }
            false => None,
        };

        let new_viewport = match self.rt_viewports.get(node).is_some() {
            true => new_th_viewport.as_ref().unwrap(),
            false => viewport,
        };

        // Start by drawing ourselves
        self.draw_node(frame, new_viewport, node, base)?;

        let layout = self.rt_layout_nodes.get(node).unwrap();

        // Update our subsurf offset
        let mut new_base = (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);
        // If this is a viewport boundary also add our scrolling offset
        if self.rt_viewports.get(node).is_some() {
            new_base.0 += new_viewport.scroll_offset.0;
            new_base.1 += new_viewport.scroll_offset.1;
        }

        // Now draw each of our children
        for child in layout.l_children.iter() {
            self.draw_node_recurse(frame, new_viewport, child, new_base)?;
        }

        // If this node was a viewport then restore our old viewport
        if new_th_viewport.is_some() {
            frame.set_viewport(viewport)?;
        }

        Ok(())
    }

    /// Draw a scene using the provided renderer and transaction view.
    pub(crate) fn draw_surfacelists(
        &self,
        frame: &mut th::FrameRenderer<'a>,
        root_viewport: &th::Viewport,
        root_node: DakotaId,
    ) -> th::Result<()> {
        self.draw_node_recurse(frame, &root_viewport, &root_node, (0, 0))
    }
}

impl Output {
    /// Draw the entire scene
    ///
    /// This starts at the root viewport and draws all child viewports
    /// present in the specified scene object.
    pub(crate) fn draw_surfacelists(&mut self, scene: &Scene) -> th::Result<()> {
        let root_node = scene
            .d_layout_tree_root
            .clone()
            .expect("No compiled layout found, need to compile this Scene before using it");
        let root_viewport = scene.d_viewports.get_clone(&root_node).unwrap();

        let mut frame = self.d_display.acquire_next_frame()?;
        let mut trans = RenderTransaction {
            rt_resources: scene.d_resources.snapshot(),
            rt_resource_thundr_image: scene.d_resource_thundr_image.snapshot(),
            rt_resource_color: scene.d_resource_color.snapshot(),
            rt_fonts: scene.d_fonts.snapshot(),
            rt_text_font: scene.d_text_font.snapshot(),
            rt_default_font_inst: scene.d_default_font_inst.clone(),
            rt_glyphs: scene.d_glyphs.snapshot(),
            rt_viewports: scene.d_viewports.snapshot(),
            rt_layout_nodes: scene.d_layout_nodes.snapshot(),
        };
        trans.draw_surfacelists(&mut frame, &root_viewport, root_node)?;
        trans.commit();
        frame.present()
    }
}
