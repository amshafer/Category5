/// Dakota Drawing logic
///
/// This splits out the rendering layouut of Dakota, which uses
/// Thundr to draw 2D elements on the surface. This consumes the
/// LayoutNode tree computed ut of Dakota, which uses
/// Thundr to draw 2D elements on the surface. This consumes the
/// LayoutNode tree computed by the layout layer and turns it
/// into Thundr Surfaces, dispatching the draw calls.
use crate::{dom, Dakota, DakotaId};

impl Dakota {
    /// Populate a thundr surface with this nodes dimensions and content
    ///
    /// This accepts a base offset to handle child element positioning
    fn get_thundr_surf_for_el(
        &mut self,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<th::Surface> {
        let layout = self.d_layout_nodes.get(node).unwrap();

        // If this node is a viewport then ignore its offset, setting the viewport
        // will take care of positioning it.
        let offset = match self.d_viewports.get(node).is_some() {
            true => (0, 0),
            false => (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y),
        };

        // Image/color content will be set later
        let mut surf = if let Some(glyph_id) = layout.l_glyph_id {
            let font_id = self.get_font_id_for_el(node);
            let font = self.d_fonts.get(&font_id).unwrap();
            let font_inst = &mut self
                .d_font_instances
                .iter_mut()
                .find(|(f, _)| *f == *font)
                .expect("Could not find FontInstance")
                .1;
            // If this path is hit, then this layout node is really a glyph in a
            // larger block of text. It has been created as a child, and isn't
            // a real element. We ask the font code to give us a surface for
            // it that we can display.
            font_inst.get_thundr_surf_for_glyph(&mut self.d_thund, glyph_id, &offset)
        } else {
            th::Surface::new(
                th::Rect::new(
                    offset.0,
                    offset.1,
                    layout.l_size.width,
                    layout.l_size.height,
                ),
                None, // image
                None, // color
            )
        };

        // Handle binding images
        // We need to get the resource's content from our resource map, get
        // the thundr image for it, and bind it to our new surface.
        if let Some(resource_id) = self.d_resources.get(node) {
            // Assert that only one content type is set
            let mut content_num = 0;

            if let Some(image) = self.d_resource_thundr_image.get(&resource_id) {
                surf.bind_image(image.clone());
                content_num += 1;
            }
            if let Some(color) = self.d_resource_color.get(&resource_id) {
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
    fn get_thundr_viewport(
        &self,
        parent: &th::Viewport,
        node: &DakotaId, // child viewport
        base: (i32, i32),
    ) -> Option<th::Viewport> {
        let layout = self.d_layout_nodes.get(node)?;
        let viewport = self.d_viewports.get(node)?;

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

    /// Helper for drawing a single element
    ///
    /// This does not recurse. Will skip drawing this node if it is out of the bounds of
    /// its viewport.
    fn draw_node(
        &mut self,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        {
            let layout = self.d_layout_nodes.get(node).unwrap();

            // Test that this child is visible before drawing it
            let offset = dom::Offset::new(base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);
            if (offset.x > viewport.size.0
                    && offset.y > viewport.size.1 )
                    // Have we scrolled past this horizontally
                    || (offset.x < 0 && offset.x * -1 > layout.l_size.width)
                    // Have we scrolled past this vertically
                    || (offset.y < 0 && offset.y * -1 > layout.l_size.height)
            {
                return Ok(());
            }
        }

        let surf = self.get_thundr_surf_for_el(node, base)?;

        self.d_thund.draw_surface(&surf)
    }

    /// Recursively draw node and all of its children
    ///
    /// This does not cross viewport boundaries
    fn draw_node_recurse(
        &mut self,
        viewport: &th::Viewport,
        node: &DakotaId,
        base: (i32, i32),
    ) -> th::Result<()> {
        // If this node is a viewport then update our thundr viewport
        let new_th_viewport = match self.d_viewports.get(node).is_some() {
            true => {
                // Set Thundr's currently in use viewport
                let th_viewport = self.get_thundr_viewport(viewport, node, base).unwrap();
                self.d_thund.set_viewport(&th_viewport)?;

                Some(th_viewport)
            }
            false => None,
        };

        let new_viewport = match self.d_viewports.get(node).is_some() {
            true => new_th_viewport.as_ref().unwrap(),
            false => viewport,
        };

        // Start by drawing ourselves
        self.draw_node(new_viewport, node, base)?;

        // Help borrow checker. We don't do any modification to this while
        // drawing so it's fine.
        let layout_nodes = self.d_layout_nodes.clone();
        let layout = layout_nodes.get(node).unwrap();

        // Update our subsurf offset
        // If this node is a viewport then the base offset needs to be reset
        let new_base = match self.d_viewports.get(node) {
            // do scrolling here to allow us to test things that are off screen?
            // By putting the offset here all children will be offset by it
            Some(vp) => (vp.scroll_offset.0, vp.scroll_offset.1),
            None => (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y),
        };

        // Now draw each of our children
        for child in layout.l_children.iter() {
            self.draw_node_recurse(new_viewport, child, new_base)?;
        }

        // If this node was a viewport then restore our old viewport
        if new_th_viewport.is_some() {
            self.d_thund.set_viewport(viewport)?;
        }

        Ok(())
    }

    /// Draw the entire scene
    ///
    /// This starts at the root viewport and draws all child viewports
    pub(crate) fn draw_surfacelists(&mut self) -> th::Result<()> {
        let root_node = self.d_layout_tree_root.clone().unwrap();
        let root_viewport = self.d_viewports.get_clone(&root_node).unwrap();

        self.d_thund.begin_recording()?;
        self.draw_node_recurse(&root_viewport, &root_node, (0, 0))?;
        self.d_thund.end_recording()?;

        Ok(())
    }
}
