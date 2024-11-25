//! Dakota Scene
//!
//! This contains the Element tree that will be used for presentation
//! on an arbitrary output. Scene's are self-contained and contain all
//! layout information.
// Austin Shafer - 2024
extern crate utils;
use crate::font;
use crate::layout::LayoutNode;
use crate::{dom, DakotaId, DakotaObjectType, SubsurfaceOrder, VirtualOutput};
use th::{Damage, Dmabuf, Droppable};
use utils::log;
use utils::{anyhow, Context, Result};

use std::sync::Arc;

// Re-exmport our getters/setters
mod generated;

pub struct Scene {
    /// The default device to create resources with
    pub(crate) d_dev: Arc<th::Device>,
    /// This is one ECS that is composed of multiple tables
    pub d_ecs_inst: ll::Instance,
    /// This is all of the LayoutNodes in the system, each corresponding to
    /// an Element or a subcomponent of an Element. Indexed by DakotaId.
    pub(crate) d_layout_nodes: ll::Component<LayoutNode>,
    // NOTE: --------------------------------
    //
    // If you update the following you may have to edit the generated
    // getters/setters in generated.rs
    pub d_node_types: ll::Component<DakotaObjectType>,

    // Resource components
    // --------------------------------------------
    /// The resource info configured by the user
    pub d_resource_ecs_inst: ll::Instance,
    pub d_resource_hints: ll::Component<dom::Hints>,
    /// Thundr image backing this resource
    pub d_resource_thundr_image: ll::Component<th::Image>,
    /// Color to pass to Thundr for this resource
    pub d_resource_color: ll::Component<dom::Color>,

    // Element components
    // --------------------------------------------
    /// The resource currently assigned to this element
    pub d_resources: ll::Component<DakotaId>,
    pub d_offsets: ll::Component<dom::RelativeOffset>,
    pub d_widths: ll::Component<dom::Value>,
    pub d_heights: ll::Component<dom::Value>,
    pub d_fonts: ll::Component<dom::Font>,
    pub d_texts: ll::Component<dom::Text>,
    pub d_glyphs: ll::Component<font::Glyph>,
    /// points to an id with font instance
    pub d_text_font: ll::Component<DakotaId>,
    pub d_contents: ll::Component<dom::Content>,
    pub d_bounds: ll::Component<dom::Edges>,
    pub d_children: ll::Component<Vec<DakotaId>>,
    pub d_unbounded_subsurf: ll::Component<bool>,
    /// Is this element a viewport node. If so it will have a viewport
    /// boundary and scroll the content inside of it.
    pub d_is_viewport: ll::Component<bool>,
    /// Any viewports assigned after layout
    ///
    /// If this is a viewport boundary then this will be populated to
    /// control draw clipping
    pub d_viewports: ll::Component<th::Viewport>,

    // DOM components
    // --------------------------------------------
    pub d_dom: Option<dom::DakotaDOM>,

    /// This is the root node in the scene tree
    pub d_layout_tree_root: Option<DakotaId>,
    /// Our current resolution. This is inherited from Output during
    /// creation and will be updated every time the output is out of
    /// date (resized).
    pub d_window_dims: (u32, u32),
    /// Default Font instance
    pub d_default_font_inst: DakotaId,
    pub d_freetype: ft::Library,
    pub d_fontconfig: fc::Fontconfig,

    /// Font shaping information. This is held separately outside of our ECS tables
    /// since it is not threadsafe. This associates a Font with the corresponding
    /// instance containing the shaping information.
    pub d_font_instances: Vec<(dom::Font, font::FontInstance)>,
}

macro_rules! create_component_and_table {
    ($ecs:ident, $llty:ty, $name:ident) => {
        let $name: ll::Component<$llty> = $ecs.add_component();
    };
}

impl Scene {
    pub(crate) fn new(dev: Arc<th::Device>, resolution: (u32, u32)) -> Result<Self> {
        let mut layout_ecs = ll::Instance::new();
        create_component_and_table!(layout_ecs, LayoutNode, layout_table);
        create_component_and_table!(layout_ecs, DakotaObjectType, types_table);
        create_component_and_table!(layout_ecs, DakotaId, resources_table);
        create_component_and_table!(layout_ecs, dom::RelativeOffset, offsets_table);
        create_component_and_table!(layout_ecs, dom::Value, width_table);
        create_component_and_table!(layout_ecs, dom::Value, height_table);
        create_component_and_table!(layout_ecs, dom::Text, texts_table);
        create_component_and_table!(layout_ecs, dom::Font, font_table);
        create_component_and_table!(layout_ecs, font::Glyph, glyph_table);
        create_component_and_table!(layout_ecs, DakotaId, text_font_table);
        create_component_and_table!(layout_ecs, dom::Content, content_table);
        create_component_and_table!(layout_ecs, dom::Edges, bounds_table);
        create_component_and_table!(layout_ecs, Vec<DakotaId>, children_table);
        create_component_and_table!(layout_ecs, bool, unbounded_subsurf_table);
        create_component_and_table!(layout_ecs, th::Viewport, viewports_table);
        create_component_and_table!(layout_ecs, bool, is_viewports_table);

        let mut resource_ecs = ll::Instance::new();
        create_component_and_table!(resource_ecs, dom::Hints, resource_hints_table);
        create_component_and_table!(resource_ecs, th::Image, resource_thundr_image_table);
        create_component_and_table!(resource_ecs, dom::Color, resource_color_table);

        // Create a default Font instance
        let default_inst = layout_ecs.add_entity();

        let mut ret = Self {
            d_dev: dev,
            d_resource_ecs_inst: resource_ecs,
            d_resource_hints: resource_hints_table,
            d_resource_thundr_image: resource_thundr_image_table,
            d_resource_color: resource_color_table,
            d_ecs_inst: layout_ecs,
            d_layout_nodes: layout_table,
            d_node_types: types_table,
            d_resources: resources_table,
            d_offsets: offsets_table,
            d_widths: width_table,
            d_heights: height_table,
            d_fonts: font_table,
            d_texts: texts_table,
            d_text_font: text_font_table,
            d_glyphs: glyph_table,
            d_contents: content_table,
            d_bounds: bounds_table,
            d_children: children_table,
            d_dom: None,
            d_unbounded_subsurf: unbounded_subsurf_table,
            d_is_viewport: is_viewports_table,
            d_viewports: viewports_table,
            d_layout_tree_root: None,
            d_window_dims: resolution,
            d_default_font_inst: default_inst.clone(),
            d_freetype: ft::Library::init().context(anyhow!("Could not get freetype library"))?,
            d_fontconfig: fc::Fontconfig::new()
                .context(anyhow!("Could not initialize fontconfig"))?,
            d_font_instances: Vec::new(),
        };

        // Define our default font
        ret.d_node_types.set(&default_inst, DakotaObjectType::Font);
        ret.define_font(
            &default_inst,
            dom::Font {
                name: "Default".to_string(),
                font_name: "JetBrainsMono".to_string(),
                pixel_size: 16,
                color: None,
            },
        );

        return Ok(ret);
    }

    /// Get the Lluvia ECS backing DakotaIds
    ///
    /// This allows for applications using this to create their
    /// own Components which are indexed by DakotaId.
    pub fn get_ecs_instance(&self) -> ll::Instance {
        self.d_ecs_inst.clone()
    }

    /// Get the Lluvia ECS backing DakotaIds for Resources
    ///
    /// This allows for applications using this to create their
    /// own Components which are indexed by Resource Ids.
    pub fn get_resource_ecs_instance(&self) -> ll::Instance {
        self.d_resource_ecs_inst.clone()
    }

    /// Do we need to refresh the layout tree and rerender
    pub fn needs_refresh(&self) -> bool {
        self.d_node_types.is_modified()
            || self.d_resource_hints.is_modified()
            || self.d_resource_thundr_image.is_modified()
            || self.d_resource_color.is_modified()
            || self.d_resources.is_modified()
            || self.d_offsets.is_modified()
            || self.d_widths.is_modified()
            || self.d_heights.is_modified()
            || self.d_fonts.is_modified()
            || self.d_texts.is_modified()
            || self.d_text_font.is_modified()
            || self.d_contents.is_modified()
            || self.d_bounds.is_modified()
            || self.d_children.is_modified()
            || self.d_unbounded_subsurf.is_modified()
    }

    fn clear_needs_refresh(&mut self) {
        self.d_node_types.clear_modified();
        self.d_resource_hints.clear_modified();
        self.d_resource_thundr_image.clear_modified();
        self.d_resource_color.clear_modified();
        self.d_resources.clear_modified();
        self.d_offsets.clear_modified();
        self.d_widths.clear_modified();
        self.d_heights.clear_modified();
        self.d_fonts.clear_modified();
        self.d_texts.clear_modified();
        self.d_text_font.clear_modified();
        self.d_contents.clear_modified();
        self.d_bounds.clear_modified();
        self.d_children.clear_modified();
        self.d_unbounded_subsurf.clear_modified();
    }

    /// Create a new Dakota Id
    ///
    /// The type of the new id must be specified. In Dakota, all objects are
    /// represented by an Id, the type of which is specified during creation.
    /// This type will assign the "role" of this id, and what data can be
    /// attached to it.
    pub(crate) fn create_new_id_common(
        ecs_inst: &mut ll::Instance,
        node_types: &mut ll::Snapshot<DakotaObjectType>,
        element_type: DakotaObjectType,
    ) -> Result<DakotaId> {
        let id = ecs_inst.add_entity();

        node_types.set(&id, element_type);
        return Ok(id);
    }

    /// Set the current Dakota DOM object
    pub fn set_dakota_dom(&mut self, dom: dom::DakotaDOM) {
        self.d_dom = Some(dom);
    }

    /// Create a new Dakota element
    pub fn create_element(&mut self) -> Result<DakotaId> {
        let mut node_types = self.d_node_types.snapshot();
        let res = Self::create_new_id_common(
            &mut self.d_ecs_inst,
            &mut node_types,
            DakotaObjectType::Element,
        );
        node_types.commit();
        return res;
    }

    /// Returns true if this element will have it's position chosen for it by
    /// Dakota's layout engine.
    pub fn child_uses_autolayout(&self, id: &DakotaId) -> bool {
        self.d_offsets.get(id).is_some()
    }

    /// Helper for populating an element with default formatting
    /// regular text. This saves the user from fully specifying the details
    /// of the text objects for this common operation.
    pub fn set_text_regular(&mut self, resource: &DakotaId, text: &str) {
        self.d_texts.set(
            resource,
            dom::Text {
                items: vec![dom::TextItem::p(dom::TextRun {
                    value: text.to_owned(),
                    cache: None,
                })],
            },
        );
    }

    /// Create a new Dakota resource
    pub fn create_resource(&mut self) -> Result<DakotaId> {
        Ok(self.d_resource_ecs_inst.add_entity())
    }

    pub(crate) fn define_resource_from_image_internal(
        dev: &th::Device,
        resource_thundr_image: &mut ll::Snapshot<th::Image>,
        resource_color: &ll::Snapshot<dom::Color>,
        res: &DakotaId,
        file_path: &std::path::Path,
        format: dom::Format,
    ) -> Result<()> {
        if Self::is_resource_defined_internal(resource_thundr_image, resource_color, res) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        // Create an in-memory representation of the image contents
        let resolution = image::image_dimensions(file_path)
            .context("Format of image could not be guessed correctly. Could not get resolution")?;
        let img = image::open(file_path)
            .context("Could not open image path")?
            .to_bgra8();
        let pixels: Vec<u8> = img.into_vec();

        Self::define_resource_from_bits_internal(
            dev,
            resource_thundr_image,
            resource_color,
            res,
            pixels.as_slice(),
            resolution.0,
            resolution.1,
            0,
            format,
        )
    }

    /// Define a resource's contents given a PNG image
    ///
    /// This will look up and open the image at `file_path`, and populate
    /// the resource `res`'s contents from it.
    pub fn define_resource_from_image(
        &mut self,
        res: &DakotaId,
        file_path: &std::path::Path,
        format: dom::Format,
    ) -> Result<()> {
        let mut images = self.d_resource_thundr_image.snapshot();
        let mut colors = self.d_resource_color.snapshot();
        let res = Self::define_resource_from_image_internal(
            &mut self.d_dev,
            &mut images,
            &colors,
            res,
            file_path,
            format,
        );
        images.precommit();
        colors.precommit();
        images.commit();
        colors.commit();
        res
    }

    /// Has this Resource been defined
    ///
    /// If a resource has been defined then it contains surface contents. This
    /// means an internal GPU resource has been allocated for it.
    pub fn is_resource_defined(&self, res: &DakotaId) -> bool {
        let mut images = self.d_resource_thundr_image.snapshot();
        let mut colors = self.d_resource_color.snapshot();
        let res = Self::is_resource_defined_internal(&mut images, &colors, res);
        images.precommit();
        colors.precommit();
        images.commit();
        colors.commit();
        res
    }

    fn is_resource_defined_internal(
        resource_thundr_image: &ll::Snapshot<th::Image>,
        resource_color: &ll::Snapshot<dom::Color>,
        res: &DakotaId,
    ) -> bool {
        resource_thundr_image.get(res).is_some() || resource_color.get(res).is_some()
    }

    /// Define a resource's contents from an array
    ///
    /// This will initialize the resource's GPU image using the contents from
    /// the `data` slice. The `stride` and `format` arguments are used to correctly
    /// specify the layout of memory within `data`, a stride of zero implies that
    /// pixels are tightly packed.
    ///
    /// A stride of zero implies the pixels are tightly packed.
    pub fn define_resource_from_bits(
        &mut self,
        res: &DakotaId,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32, // TODO: Handle stride properly
        format: dom::Format,
    ) -> Result<()> {
        let mut images = &mut self.d_resource_thundr_image.snapshot();
        let mut colors = self.d_resource_color.snapshot();
        let res = Self::define_resource_from_bits_internal(
            &self.d_dev,
            &mut images,
            &colors,
            res,
            data,
            width,
            height,
            stride,
            format,
        );
        images.precommit();
        colors.precommit();
        images.commit();
        colors.commit();
        res
    }

    fn define_resource_from_bits_internal(
        dev: &th::Device,
        resource_thundr_image: &mut ll::Snapshot<th::Image>,
        resource_color: &ll::Snapshot<dom::Color>,
        res: &DakotaId,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32, // TODO: Handle stride properly
        format: dom::Format,
    ) -> Result<()> {
        if format != dom::Format::ARGB8888 {
            return Err(anyhow!("Invalid image format"));
        }

        if Self::is_resource_defined_internal(resource_thundr_image, resource_color, res) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        // create a thundr image for each resource
        let image = dev
            .create_image_from_bits(data, width, height, stride, None)
            .context("Could not create Image resources")?;

        resource_thundr_image.set(res, image);
        Ok(())
    }

    /// Update the resource contents from a damaged CPU buffer
    ///
    /// This allows for updating the contents of a resource according to
    /// the data provided, within the damage regions specified. This is
    /// useful for compositor users to update a local copy of a shm texture.
    /// This should *NOT* be used with dmabuf-backed textures.
    pub fn update_resource_from_bits(
        &mut self,
        res: &DakotaId,
        data: &[u8],
        width: u32,
        height: u32,
        stride: u32, // TODO: Handle stride properly
        format: dom::Format,
        damage: Option<Damage>,
    ) -> Result<()> {
        if !(format == dom::Format::ARGB8888 || format == dom::Format::XRGB8888) {
            return Err(anyhow!("Invalid image format"));
        }

        let image = self.d_resource_thundr_image.get_mut(res).ok_or(anyhow!(
            "Resource does not have a internal GPU resource defined"
        ))?;

        self.d_dev
            .update_image_from_bits(&image, data, width, height, stride, damage, None)
            .context("Could not update image with damaged region")?;

        Ok(())
    }

    /// Populate a resource by importing a dmabuf
    ///
    /// This allows for loading the `fd` specified into Dakota's internal
    /// renderer without any copies. `modifier` must be supported by the
    /// Dakota device in use.
    pub fn define_resource_from_dmabuf(
        &mut self,
        res: &DakotaId,
        dmabuf: &Dmabuf,
        release_info: Option<Box<dyn Droppable + Send + Sync>>,
    ) -> Result<()> {
        if Self::is_resource_defined_internal(
            &self.d_resource_thundr_image.snapshot(),
            &self.d_resource_color.snapshot(),
            res,
        ) {
            return Err(anyhow!("Cannot redefine Resource contents"));
        }

        let image = self
            .d_dev
            .create_image_from_dmabuf(dmabuf, release_info)
            .context("Could not create Image resources")?;

        self.d_resource_thundr_image.set(res, image);
        Ok(())
    }

    /// Create a new Dakota Font object
    ///
    /// This creates a new id representing the requested font.
    pub fn create_font(&mut self) -> Result<DakotaId> {
        let mut node_types = self.d_node_types.snapshot();
        let res = Self::create_new_id_common(
            &mut self.d_ecs_inst,
            &mut node_types,
            DakotaObjectType::Font,
        );
        node_types.commit();
        return res;
    }

    pub(crate) fn define_font_internal(
        font_instances: &mut Vec<(dom::Font, font::FontInstance)>,
        fonts: &mut ll::Snapshot<dom::Font>,
        freetype: &ft::Library,
        fontconfig: &fc::Fontconfig,
        id: &DakotaId,
        font: dom::Font,
    ) {
        let font_path = fontconfig.find(&font.font_name, None).unwrap();

        if font_instances.iter().find(|(f, _)| *f == font).is_none() {
            font_instances.push((
                font.clone(),
                font::FontInstance::new(
                    freetype,
                    font_path.path.to_str().unwrap(),
                    font.pixel_size,
                ),
            ));
        }

        fonts.set(id, font);
    }

    /// Define a Font for text rendering
    ///
    /// This accepts a definition of a Font, including the name and location
    /// of the font file. This is then loaded into Dakota and text rendering
    /// is allowed with the font.
    pub fn define_font(&mut self, id: &DakotaId, font: dom::Font) {
        let mut fonts = self.d_fonts.snapshot();
        Self::define_font_internal(
            &mut self.d_font_instances,
            &mut fonts,
            &self.d_freetype,
            &self.d_fontconfig,
            id,
            font,
        );
        fonts.commit();
    }

    pub(crate) fn add_child_to_element_internal(
        children: &mut ll::Snapshot<Vec<DakotaId>>,
        parent: &DakotaId,
        child: DakotaId,
    ) {
        // Add old_id as a child element
        if children.get_mut(parent).is_none() {
            children.set(parent, Vec::new());
        }
        let child_vec = children.get_mut(parent).unwrap();

        if child_vec
            .iter()
            .find(|c| c.get_raw_id() == child.get_raw_id())
            .is_none()
        {
            child_vec.push(child);
        }
    }

    /// Add `child` as a child element to `parent`.
    ///
    /// This operation on makes sense for Dakota objects with the `Element` object
    /// type. Will only add `child` if it is not already a child of `parent`.
    pub fn add_child_to_element(&mut self, parent: &DakotaId, child: DakotaId) {
        let mut children = self.d_children.snapshot();
        Self::add_child_to_element_internal(&mut children, parent, child);
        children.commit();
    }

    /// Remove `child` as a child element of `parent`.
    ///
    /// This operation on makes sense for Dakota objects with the `Element` object
    /// type. This does nothing if `child` is not a child of `parent`.
    pub fn remove_child_from_element(&mut self, parent: &DakotaId, child: &DakotaId) -> Result<()> {
        let mut children = match self.d_children.get_mut(parent) {
            Some(children) => children,
            None => return Ok(()),
        };

        // Get the indices of our two children
        if let Some(pos) = children
            .iter()
            .position(|c| c.get_raw_id() == child.get_raw_id())
        {
            children.remove(pos);
        }

        Ok(())
    }

    /// Reorder two elements that are children of parent
    ///
    /// Depending on the value of `order`, this will insert child A above or below
    /// child B in the element list.
    ///
    /// This is best used for when you need to bring an element to the front or back
    /// of a child list without regenerating the entire thing. This is particularly
    /// useful for category5, which orders elements for wayland subsurfaces
    pub fn reorder_children_element(
        &mut self,
        parent: &DakotaId,
        order: SubsurfaceOrder,
        a: &DakotaId,
        b: &DakotaId,
    ) -> Result<()> {
        let mut children = self
            .d_children
            .get_mut(parent)
            .context("Parent does not have any children, cannot reorder")?;

        // Get the indices of our two children
        let pos_a = children
            .iter()
            .position(|c| c.get_raw_id() == a.get_raw_id())
            .context("Could not find Child A in Parent's children")?;
        let pos_b = children
            .iter()
            .position(|c| c.get_raw_id() == b.get_raw_id())
            .context("Could not find Child B in Parent's children")?;

        // Remove child A and insert it above or below child B
        children.remove(pos_a);
        children.insert(
            match order {
                SubsurfaceOrder::Above => pos_b + 1,
                SubsurfaceOrder::Below => pos_b,
            },
            a.clone(),
        );

        Ok(())
    }

    /// Move child to front of children in parent
    ///
    /// This is used for bringing an element into "focus", and placing it as
    /// the foremost child.
    pub fn move_child_to_front(&mut self, parent: &DakotaId, child: &DakotaId) -> Result<()> {
        let mut children = self
            .d_children
            .get_mut(parent)
            .context("Parent does not have any children, cannot reorder")?;

        // Get the indices of our two children
        let pos = children
            .iter()
            .position(|c| c.get_raw_id() == child.get_raw_id())
            .context("Could not find Child A in Parent's children")?;

        // Remove child A and insert it above or below child B
        children.remove(pos);
        children.push(child.clone());

        Ok(())
    }

    /// This refreshes the entire scene, and regenerates
    /// the Thundr surface list.
    pub fn recompile(&mut self, virtual_output: &VirtualOutput) -> Result<()> {
        log::verbose!("Dakota: Refreshing element tree");
        let root_node_id = {
            let dom = self
                .d_dom
                .as_ref()
                .ok_or(anyhow!("Only DOM objects can be refreshed"))?;

            dom.root_element.clone()
        };

        // Update our cached output size. This gets consumed by the layout engine
        self.d_window_dims = virtual_output.get_size();

        // Set the size of our root node. We need to assign this a size manually so
        // that it doesn't default and size itself to its children, causing the viewport
        // scroll region calculation to go wrong.
        self.d_widths.set(
            &root_node_id,
            dom::Value::Constant(self.d_window_dims.0 as i32),
        );
        self.d_heights.set(
            &root_node_id,
            dom::Value::Constant(self.d_window_dims.1 as i32),
        );

        // Reset our old tree
        self.d_layout_tree_root = None;

        // Manually mark the root node as a viewport node. It always is, and it will
        // always have the root viewport.
        self.d_is_viewport.set(&root_node_id, true);

        // construct layout tree with sizes of all boxes
        self.layout(&root_node_id)?;

        // Perform the Thundr pass
        //
        self.d_layout_tree_root = Some(root_node_id);

        self.clear_needs_refresh();

        Ok(())
    }

    /// Returns true if the node is of a type that guarantees it cannot have
    /// child elements.
    ///
    /// This most notably happens with text elements.
    fn node_can_have_children(&self, texts: &ll::Snapshot<dom::Text>, id: &DakotaId) -> bool {
        !texts.get(id).is_some()
    }

    fn viewport_at_pos_recursive(
        &self,
        layout_nodes: &ll::Snapshot<LayoutNode>,
        viewports: &ll::Snapshot<th::Viewport>,
        texts: &ll::Snapshot<dom::Text>,
        id: &DakotaId,
        base: (i32, i32),
        x: i32,
        y: i32,
    ) -> Option<DakotaId> {
        let layout = layout_nodes.get(id).unwrap();
        let offset = (base.0 + layout.l_offset.x, base.1 + layout.l_offset.y);

        // If this node is of a type where we know it has a lot of children but none of them
        // could possibly be a viewport, take an early exit.
        // This most notably happens in the case of text nodes, which have a large number of
        // virtual children.
        if !self.node_can_have_children(texts, id) && viewports.get(id).is_none() {
            return None;
        }

        // Since the tree is back to front, process the children first. If one of them is a match,
        // it is the top-most viewport and we should return it. Otherwise we can test if this node
        // matches.  If this is a new viewport boundary then add its scroll offset to our children
        let mut child_offset = offset;
        if let Some(vp) = viewports.get(id) {
            child_offset.0 += vp.offset.0 + vp.scroll_offset.0;
            child_offset.1 += vp.offset.1 + vp.scroll_offset.1;
        }
        for child in layout.l_children.iter() {
            if let Some(ret) = self.viewport_at_pos_recursive(
                layout_nodes,
                viewports,
                texts,
                child,
                child_offset,
                x,
                y,
            ) {
                return Some(ret);
            }
        }

        // If this node is not a viewport return nothing
        if viewports.get(id).is_none() {
            return None;
        }

        let x_range = offset.0..(offset.0 + layout.l_size.width);
        let y_range = offset.1..(offset.1 + layout.l_size.height);

        if x_range.contains(&x) && y_range.contains(&y) {
            return Some(id.clone());
        }

        None
    }

    /// Walks the viewport tree and returns the ECS id of the
    /// viewport at this location. Note there will always be a viewport
    /// because the entire window surface is at the very least, the root viewport
    pub fn get_viewport_at_position(&self, x: i32, y: i32) -> DakotaId {
        assert!(self.d_layout_tree_root.is_some());
        let root_node = self.d_layout_tree_root.as_ref().unwrap();

        // use some snapshots here to hold the read locks open
        let layout_nodes = self.d_layout_nodes.snapshot();
        let viewports = self.d_viewports.snapshot();
        let texts = self.d_texts.snapshot();
        assert!(viewports.get(root_node).is_some());

        self.viewport_at_pos_recursive(&layout_nodes, &viewports, &texts, root_node, (0, 0), x, y)
            .unwrap()
    }
}
