/// A XML dakota reader
///
/// This will turn a Dakota file into a tree of elements that can be
/// processed by the engine. This is basically the parsing step in a
/// compiler, where we turn XML into our IR (i.e. LayoutNodes)
///
/// Austin Shafer - 2023
extern crate quick_xml;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::utils::anyhow;
use crate::{dom, font};
use crate::{Context, DakotaId, DakotaObjectType, Result, Scene};

use std::collections::HashMap;
use std::io::BufRead;
use std::sync::Arc;
use utils::log;

/// XML parser transaction
///
/// This transaction allows the XML engine to have a consistent,
/// read-only view of the state while it is populating the sizes of
/// the LayoutNodes
///
/// These fields correspond to the identically named variants in Dakota.
pub(crate) struct ParserTransaction<'a> {
    pt_dev: &'a th::Device,
    pt_ecs_inst: ll::Instance,
    pt_resource_ecs_inst: ll::Instance,
    pt_node_types: ll::Snapshot<'a, DakotaObjectType>,
    pt_resource_hints: ll::Snapshot<'a, dom::Hints>,
    pt_resources: ll::Snapshot<'a, DakotaId>,
    pt_resource_thundr_image: ll::Snapshot<'a, th::Image>,
    pt_resource_color: ll::Snapshot<'a, dom::Color>,
    pt_fonts: ll::Snapshot<'a, dom::Font>,
    pt_text_font: ll::Snapshot<'a, DakotaId>,
    pt_texts: ll::Snapshot<'a, dom::Text>,
    pt_glyphs: ll::Snapshot<'a, font::Glyph>,
    pt_is_viewport: ll::Snapshot<'a, bool>,
    pt_contents: ll::Snapshot<'a, dom::Content>,
    pt_offsets: ll::Snapshot<'a, dom::RelativeOffset>,
    pt_widths: ll::Snapshot<'a, dom::Value>,
    pt_heights: ll::Snapshot<'a, dom::Value>,
    pt_children: ll::Snapshot<'a, Vec<DakotaId>>,
    pt_font_instances: &'a mut Vec<(dom::Font, font::FontInstance)>,
    pt_freetype: &'a ft::Library,
    pt_fontconfig: &'a fc::Fontconfig,
    pt_unbounded_subsurf: ll::Snapshot<'a, bool>,
    /// This maps the string names for resource found in the
    /// XML document to DakotaIds that represent those resources.
    ///
    /// We need this since the resource section may be processed
    /// after the elements for some reason. We need to have a way
    /// to translate from strings to ids so that we can set up
    /// all the elements to reference resources without holding
    /// a giant array of resources somewhere.
    pt_name_to_id_map: HashMap<String, DakotaId>,
    /// Similar motivation but for font definitions
    pt_font_name_to_id_map: HashMap<String, DakotaId>,
}

/// A list of element names
///
/// This allows us to set and compare the currently processed element
/// without having to do expensive string ops
#[derive(Debug)]
enum Element {
    El {
        x: Option<dom::Value>,
        y: Option<dom::Value>,
        width: Option<dom::Value>,
        height: Option<dom::Value>,
    },
    Text(Vec<dom::TextItem>, Option<String>),
    TextFont(Option<String>),
    PixelSize(Option<u32>),
    Window {
        title: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        events: dom::WindowEvents,
    },
    Dakota {
        version: Option<String>,
        window: Option<dom::Window>,
        root_element: Option<DakotaId>,
    },
    Version(Option<String>),
    Name(Option<String>),
    FontName(Option<String>),
    Title(Option<String>),
    Width(Option<dom::Value>),
    Height(Option<dom::Value>),
    WindowWidth(Option<u32>),
    WindowHeight(Option<u32>),
    Layout,
    Color {
        r: Option<f32>,
        g: Option<f32>,
        b: Option<f32>,
        a: Option<f32>,
    },
    X(Option<dom::Value>),
    Y(Option<dom::Value>),
    Relative(Option<f32>),
    Constant(Option<i32>),
    R(Option<f32>),
    G(Option<f32>),
    B(Option<f32>),
    A(Option<f32>),
    AbsPath(Option<String>),
    RelPath(Option<String>),
    Image(Option<dom::Format>, Option<dom::Data>),
    Format(Option<dom::Format>),
    Data(dom::Data),
    ResourceMap,
    Resource(Option<String>),
    FontDefinition(
        Option<String>,
        Option<String>,
        Option<String>,
        u32,
        Option<dom::Color>,
    ),
    ResourceDefinition {
        name: Option<String>,
        image: Option<dom::Image>,
        color: Option<dom::Color>,
        hints: Option<dom::Hints>,
    },
    Hints(dom::Hints),
    Static(bool),
    Size(Option<dom::Value>, Option<dom::Value>),
    Offset(Option<dom::Value>, Option<dom::Value>),
    P(Option<String>),
    Bold(Option<String>),
    Content(Option<DakotaId>),
    Event {
        groups: Vec<String>,
        id: Option<String>,
        args: Vec<String>,
    },
    Group(Option<String>),
    Id(Option<String>),
    Arg(Option<String>),
    WindowEvents(dom::WindowEvents),
    Resize(Option<dom::Event>),
    RedrawComplete(Option<dom::Event>),
    Closed(Option<dom::Event>),
    UnboundedSubsurface,
    Viewport,
}

impl Element {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let ret = match bytes {
            b"el" => Self::El {
                x: None,
                y: None,
                width: None,
                height: None,
            },
            b"text" => Self::Text(Vec::new(), None),
            b"font" => Self::TextFont(None),
            b"pixel_size" => Self::PixelSize(None),
            b"window" => Self::Window {
                title: None,
                width: None,
                height: None,
                events: dom::WindowEvents::default(),
            },
            b"dakota" => Self::Dakota {
                version: None,
                window: None,
                root_element: None,
            },
            b"version" => Self::Version(None),
            b"name" => Self::Name(None),
            b"font_name" => Self::FontName(None),
            b"title" => Self::Title(None),
            b"width" => Self::Width(None),
            b"height" => Self::Height(None),
            b"window_width" => Self::WindowWidth(None),
            b"window_height" => Self::WindowHeight(None),
            b"relative" => Self::Relative(None),
            b"constant" => Self::Constant(None),
            b"x" => Self::X(None),
            b"y" => Self::Y(None),
            b"layout" => Self::Layout,
            b"color" => Self::Color {
                r: None,
                g: None,
                b: None,
                a: None,
            },
            b"r" => Self::R(None),
            b"g" => Self::G(None),
            b"b" => Self::B(None),
            b"a" => Self::A(None),
            b"absPath" => Self::AbsPath(None),
            b"relPath" => Self::RelPath(None),
            b"image" => Self::Image(None, None),
            b"format" => Self::Format(None),
            b"data" => Self::Data(dom::Data {
                rel_path: None,
                abs_path: None,
            }),
            b"resourceMap" => Self::ResourceMap,
            b"resource" => Self::Resource(None),
            b"define_font" => Self::FontDefinition(None, None, None, 0, None),
            b"define_resource" => Self::ResourceDefinition {
                name: None,
                image: None,
                color: None,
                hints: None,
            },
            b"hints" => Self::Hints(dom::Hints::default()),
            b"static" => Self::Static(false),
            b"size" => Self::Size(None, None),
            b"p" => Self::P(None),
            b"bold" => Self::Bold(None),
            b"offset" => Self::Offset(None, None),
            b"content" => Self::Content(None),
            b"event" => Self::Event {
                groups: Vec::new(),
                id: None,
                args: Vec::new(),
            },
            b"group" => Self::Group(None),
            b"id" => Self::Id(None),
            b"arg" => Self::Arg(None),
            b"window_events" => Self::WindowEvents(dom::WindowEvents::default()),
            b"resize" => Self::Resize(None),
            b"redraw_complete" => Self::RedrawComplete(None),
            b"closed" => Self::Closed(None),
            b"unbounded_subsurface" => Self::UnboundedSubsurface,
            b"viewport" => Self::Viewport,
            _ => {
                return Err(anyhow!(
                    "Element name {} is not a valid element name",
                    std::str::from_utf8(bytes)?
                ))
            }
        };

        Ok(ret)
    }

    fn convert_to_dom_value(&self) -> Result<dom::Value> {
        match self {
            Element::Relative(float) => Ok(dom::Value::Relative(
                float.ok_or(anyhow!("No data provided to <relative> tag"))?,
            )),
            Element::Constant(int) => Ok(dom::Value::Constant(
                int.ok_or(anyhow!("No data provided to <constant> tag"))?,
            )),
            e => return Err(anyhow!("Unexpected child element: {:?}", e)),
        }
    }

    fn get_dom_event(&self) -> Result<dom::Event> {
        match self {
            Element::Event { groups, id, args } => Ok(dom::Event {
                groups: groups.clone(),
                id: id.clone(),
                args: Arc::new(args.clone()),
            }),
            e => return Err(anyhow!("Unexpected child element: {:?}", e)),
        }
    }
}

impl<'a> Drop for ParserTransaction<'a> {
    fn drop(&mut self) {
        // Unlock all of our snapshots
        // This helps prevent deadlocking when we are returning
        // early due to an error. In that case we have snapshots
        // containing the final reference to an id, but can't
        // drop it because the locks are held
        self.precommit();
    }
}

impl<'a> ParserTransaction<'a> {
    /// Commit this transaction
    fn precommit(&mut self) {
        // drop all locks to avoid deadlocking for ids that
        // get invalidated and try to free their data
        self.pt_node_types.precommit();
        self.pt_resource_hints.precommit();
        self.pt_resources.precommit();
        self.pt_resource_thundr_image.precommit();
        self.pt_resource_color.precommit();
        self.pt_fonts.precommit();
        self.pt_text_font.precommit();
        self.pt_texts.precommit();
        self.pt_glyphs.precommit();
        self.pt_is_viewport.precommit();
        self.pt_contents.precommit();
        self.pt_widths.precommit();
        self.pt_heights.precommit();
        self.pt_offsets.precommit();
        self.pt_children.precommit();
        self.pt_unbounded_subsurf.precommit();
    }

    /// Commit this transaction
    fn commit(&mut self) {
        self.precommit();

        // Now we can commit
        self.pt_node_types.commit();
        self.pt_resource_hints.commit();
        self.pt_resources.commit();
        self.pt_resource_thundr_image.commit();
        self.pt_resource_color.commit();
        self.pt_fonts.commit();
        self.pt_text_font.commit();
        self.pt_texts.commit();
        self.pt_glyphs.commit();
        self.pt_is_viewport.commit();
        self.pt_contents.commit();
        self.pt_widths.commit();
        self.pt_heights.commit();
        self.pt_offsets.commit();
        self.pt_children.commit();
        self.pt_unbounded_subsurf.commit();
    }

    // Similar to main Dakota functions. These here hook into common creation logic
    // --------------------------------------------------------------------------
    fn create_element(&mut self) -> Result<DakotaId> {
        Scene::create_new_id_common(
            &mut self.pt_ecs_inst,
            &mut self.pt_node_types,
            DakotaObjectType::Element,
        )
    }

    fn create_font(&mut self) -> Result<DakotaId> {
        Scene::create_new_id_common(
            &mut self.pt_ecs_inst,
            &mut self.pt_node_types,
            DakotaObjectType::Font,
        )
    }

    fn add_child_to_element(&mut self, parent: &DakotaId, child: DakotaId) {
        Scene::add_child_to_element_internal(&mut self.pt_children, parent, child);
    }

    fn define_font(&mut self, id: &DakotaId, font: dom::Font) {
        Scene::define_font_internal(
            &mut self.pt_font_instances,
            &mut self.pt_fonts,
            &self.pt_freetype,
            &self.pt_fontconfig,
            id,
            font,
        );
    }

    fn define_resource_from_image(
        &mut self,
        res: &DakotaId,
        file_path: &std::path::Path,
        format: dom::Format,
    ) -> Result<()> {
        Scene::define_resource_from_image_internal(
            &mut self.pt_dev,
            &mut self.pt_resource_thundr_image,
            &self.pt_resource_color,
            res,
            file_path,
            format,
        )
    }

    // --------------------------------------------------------------------------

    /// Returns a new id if this element type will have a DakotaId created for
    /// it. None if no
    fn needs_new_id(&mut self, node: &Element) -> Result<Option<DakotaId>> {
        match node {
            Element::FontDefinition(_, _, _, _, _) => Ok(Some(self.create_font()?)),
            Element::El { .. }
            | Element::Layout
            // create a dummy element Id for the toplevel dakota object to help
            // keep the parsing logic easy. It will not be used.
            | Element::Dakota {..} => Ok(Some(self.create_element()?)),
            _ => Ok(None),
        }
    }

    /// Look up this resource's DakotaId in our name -> id mapping
    ///
    /// This is used to get an id for a resource even if it has not yet
    /// been created
    fn get_id_for_name(&mut self, is_font: bool, name: &str) -> Result<DakotaId> {
        let name_to_id_map = match is_font {
            true => &mut self.pt_font_name_to_id_map,
            false => &mut self.pt_name_to_id_map,
        };

        if !name_to_id_map.contains_key(name) {
            let res = match is_font {
                true => Scene::create_new_id_common(
                    &mut self.pt_ecs_inst,
                    &mut self.pt_node_types,
                    DakotaObjectType::Font,
                )
                .context("Creating DakotaId for Resource Definition")?,
                false => self.pt_resource_ecs_inst.add_entity(),
            };
            name_to_id_map.insert(name.to_string(), res);
        }

        Ok(name_to_id_map.get(name).unwrap().clone())
    }

    /// Helper function for turning a string into a DOM object
    fn get_text_run(&self, s: &Option<String>) -> Result<dom::TextRun> {
        Ok(dom::TextRun {
            value: s
                .as_ref()
                .ok_or(anyhow!("No text inside tag that expected text data"))?
                .clone(),
            cache: None,
        })
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

    /// We need to check that all required fields were specified in the
    /// node we are finishing, otherwise we need to throw a parsing error.
    ///
    /// This is also where we pass the data specified in our child node
    /// up to its parent. At this point old_id should have all
    /// of its ECS data populated/validated, and old_node will tell us what
    /// kind it is. Depending on the type of child node we will add
    /// old_id to one of node's fields. We may also add it directly to id,
    /// such as adding a child id to our children list.
    fn add_child(
        &mut self,
        id: &DakotaId,
        node: &mut Element,
        old_id: &DakotaId,
        old_node: &Element,
    ) -> Result<()> {
        // [node/id] is the current element that we are modifying
        // old_[node/id] is the child XML element that just had its end tag
        // complete. We are propogating its data up the tree to [node/id]

        match node {
            // Element
            // -------------------------------------------------------
            Element::El {
                x,
                y,
                width,
                height,
            } => {
                match old_node {
                    Element::Resource(name) => {
                        let resource_id = self
                            .get_id_for_name(
                                false,
                                name.as_ref()
                                    .ok_or(anyhow!("Element was not assigned a resource"))?,
                            )
                            .context("Getting resource reference for element")?;
                        self.pt_resources.set(id, resource_id)
                    }
                    Element::Viewport => self.pt_is_viewport.set(id, true),
                    Element::UnboundedSubsurface => self.pt_unbounded_subsurf.set(id, true),
                    Element::El {
                        x: _,
                        y: _,
                        width: _,
                        height: _,
                    } => {
                        if !self.node_can_have_children(&self.pt_texts, id) {
                            return Err(anyhow!("Element cannot have children due to other properties defined for it"));
                        }

                        self.add_child_to_element(id, old_id.clone())
                    }
                    Element::X(val) => *x = *val,
                    Element::Y(val) => *y = *val,
                    Element::Width(val) => *width = *val,
                    Element::Height(val) => *height = *val,
                    Element::Text(data, font) => {
                        if self.pt_children.get(id).is_some() {
                            return Err(anyhow!("Text Elements cannot have children"));
                        }

                        self.pt_texts.set(
                            id,
                            dom::Text {
                                items: data.clone(),
                            },
                        );
                        // font is optional
                        if let Some(name) = font {
                            let resource_id = self
                                .get_id_for_name(true, name)
                                .context("Getting resource reference for element")?;
                            self.pt_text_font.set(id, resource_id);
                        }
                    }
                    Element::Content(data) => self.pt_contents.set(
                        id,
                        dom::Content {
                            el: data
                                .clone()
                                .ok_or(anyhow!("Content does not contain an element"))?,
                        },
                    ),
                    Element::Size(width, height) => {
                        // Widths and heights are optional
                        if let Some(width) = width {
                            self.pt_widths.set(id, *width);
                        }
                        if let Some(height) = height {
                            self.pt_heights.set(id, *height);
                        }
                    }
                    Element::Offset(x, y) => self.pt_offsets.set(
                        id,
                        dom::RelativeOffset {
                            x: x.clone()
                                .ok_or(anyhow!("Content does not contain an element"))?,
                            y: y.clone()
                                .ok_or(anyhow!("Content does not contain an element"))?,
                        },
                    ),
                    e => {
                        return Err(anyhow!("Unexpected child element: {:?}", e)
                            .context("While processing children for Dakota Element"))
                    }
                }
            }
            // -------------------------------------------------------
            Element::Window {
                title,
                width,
                height,
                events,
            } => match old_node {
                Element::Title(data) => *title = data.clone(),
                Element::WindowWidth(data) => *width = *data,
                Element::WindowHeight(data) => *height = *data,
                Element::WindowEvents(data) => *events = data.clone(),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Dakota {
                version,
                window,
                root_element,
            } => match old_node {
                Element::Version(data) => *version = data.clone(),
                Element::ResourceMap => {}
                Element::Window {
                    title,
                    width,
                    height,
                    events,
                } => {
                    let mut size = None;

                    if let Some(w) = width {
                        size = Some((*w, 0));
                    }
                    if let Some(h) = height {
                        size.as_mut()
                            .ok_or(anyhow!(
                                "Must specify both width and height of Window or none at all"
                            ))?
                            .1 = *h;
                    }

                    *window = Some(dom::Window {
                        title: title
                            .as_ref()
                            .ok_or(anyhow!("Window does not contain title field"))?
                            .clone(),
                        size: size,
                        events: events.clone(),
                    })
                }
                Element::Layout => *root_element = Some(old_id.clone()),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Text(data, font) => match old_node {
                Element::P(s) => data.push(dom::TextItem::p(self.get_text_run(s)?)),
                Element::Bold(s) => data.push(dom::TextItem::b(self.get_text_run(s)?)),
                Element::TextFont(name) => {
                    *font = Some(name.clone().context("Font name not specified")?)
                }
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::Width(data) | Element::Height(data) | Element::X(data) | Element::Y(data) => {
                *data = Some(old_node.convert_to_dom_value()?)
            }
            Element::Layout => self.add_child_to_element(id, old_id.clone()),
            Element::Color { r, g, b, a } => match old_node {
                Element::R(data) => *r = *data,
                Element::G(data) => *g = *data,
                Element::B(data) => *b = *data,
                Element::A(data) => *a = *data,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Image(format, data) => match old_node {
                Element::Format(f) => *format = f.clone(),
                Element::Data(d) => *data = Some(d.clone()),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::Data(data) => match old_node {
                Element::RelPath(path) => {
                    data.rel_path = Some(path.clone().ok_or(anyhow!(
                        "No path provided in element that expects path value"
                    ))?)
                }
                Element::AbsPath(path) => {
                    data.abs_path = Some(path.clone().ok_or(anyhow!(
                        "No path provided in element that expects path value"
                    ))?)
                }
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::ResourceMap => match old_node {
                Element::FontDefinition(name, font_name, _path, size, color) => {
                    let resource_id = self
                        .get_id_for_name(
                            true,
                            name.as_ref()
                                .ok_or(anyhow!("Font definition does not have a name"))?,
                        )
                        .context("Getting resource id for font definition")?;

                    self.define_font(
                        &resource_id,
                        dom::Font {
                            name: name
                                .clone()
                                .ok_or(anyhow!("Font definition does not have a name"))?,
                            font_name: font_name
                                .clone()
                                .ok_or(anyhow!("Font definition does not specify a font name"))?,
                            pixel_size: *size,
                            color: *color,
                        },
                    );
                }
                Element::ResourceDefinition {
                    name,
                    image,
                    color,
                    hints,
                } => {
                    // Look up this resource's id
                    let resource_id = self
                        .get_id_for_name(
                            false,
                            name.as_ref()
                                .ok_or(anyhow!("Resource definition does not have a name"))?,
                        )
                        .context("Getting resource id for resource definition")?;

                    if let Some(h) = hints.clone() {
                        self.pt_resource_hints.set(&resource_id, h);
                    }

                    // If this resource is backed by an image, populate it
                    if let Some(i) = image.as_ref() {
                        let file_path = std::path::Path::new(i.data.get_fs_path()?);
                        self.define_resource_from_image(&resource_id, &file_path, i.format)?;
                    } else if let Some(c) = color.as_ref() {
                        self.pt_resource_color.set(&resource_id, *c);
                    }
                }
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::FontDefinition(name, font_name, path, size, color) => match old_node {
                Element::Name(n) => *name = n.clone(),
                Element::FontName(n) => *font_name = n.clone(),
                Element::AbsPath(p) | Element::RelPath(p) => *path = p.clone(),
                Element::PixelSize(s) => *size = s.context("PixelSize was not populated")?,
                Element::Color { r, g, b, a } => {
                    *color = Some(dom::Color {
                        r: r.clone().ok_or(anyhow!("Color value R not specified"))?,
                        g: g.clone().ok_or(anyhow!("Color value G not specified"))?,
                        b: b.clone().ok_or(anyhow!("Color value B not specified"))?,
                        a: a.clone().ok_or(anyhow!("Color value A not specified"))?,
                    })
                }
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::ResourceDefinition {
                name,
                image,
                color,
                hints,
            } => match old_node {
                Element::Name(n) => *name = n.clone(),
                Element::Image(format, data) => {
                    *image = Some(dom::Image {
                        format: format
                            .clone()
                            .ok_or(anyhow!("Format not specified for image"))?,
                        data: data
                            .clone()
                            .ok_or(anyhow!("Format not specified for image"))?,
                    })
                }
                Element::Color { r, g, b, a } => {
                    *color = Some(dom::Color {
                        r: r.clone().ok_or(anyhow!("Color value R not specified"))?,
                        g: g.clone().ok_or(anyhow!("Color value G not specified"))?,
                        b: b.clone().ok_or(anyhow!("Color value B not specified"))?,
                        a: a.clone().ok_or(anyhow!("Color value A not specified"))?,
                    })
                }
                Element::Hints(data) => *hints = Some(data.clone()),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::Hints(data) => match old_node {
                Element::Static(val) => data.constant = *val,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Size(width, height) => match old_node {
                Element::Width(data) => *width = *data,
                Element::Height(data) => *height = *data,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::Offset(x, y) => match old_node {
                Element::X(data) => *x = *data,
                Element::Y(data) => *y = *data,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Content(data) => match old_node {
                Element::El {
                    x: _,
                    y: _,
                    width: _,
                    height: _,
                } => *data = Some(old_id.clone()),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Event { groups, id, args } => match old_node {
                Element::Group(data) => groups.push(
                    data.clone()
                        .ok_or(anyhow!("Event group text not specified"))?,
                ),
                Element::Id(data) => *id = data.clone(),
                Element::Arg(data) => args.push(
                    data.clone()
                        .ok_or(anyhow!("Event argument text not specified"))?,
                ),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            Element::Resize(data) | Element::RedrawComplete(data) | Element::Closed(data) => {
                *data = Some(old_node.get_dom_event()?)
            }
            Element::WindowEvents(events) => match old_node {
                Element::Resize(data) => events.resize = data.clone(),
                Element::RedrawComplete(data) => events.redraw_complete = data.clone(),
                Element::Closed(data) => events.closed = data.clone(),
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            _ => {
                // If this was hit we have a parsing issue. The XML stream specified
                // data in an element type that does not have any data (such as ResourceMap)
                return Err(anyhow!(
                    "Element type {:?} does not expect child elements",
                    node
                ));
            }
        }

        Ok(())
    }

    /// Here we are going to update our node info with what
    /// is specified in the XML stream
    fn handle_xml_text(&mut self, node: &mut Element, event: &Event) -> Result<()> {
        match event {
            Event::Text(text_bytes) => {
                // Get a rust String from our raw utf8 in the XML stream
                let text = std::str::from_utf8(&text_bytes)
                    .context("Creating string from utf8 bytes in XML element")?
                    .to_string();

                // Now add this text to the node's private data, if appropriate
                match node {
                    // string fields
                    Element::Version(data)
                    | Element::AbsPath(data)
                    | Element::RelPath(data)
                    | Element::P(data)
                    | Element::Bold(data)
                    | Element::Group(data)
                    | Element::Id(data)
                    | Element::Arg(data)
                    | Element::Title(data)
                    | Element::Resource(data)
                    | Element::TextFont(data)
                    | Element::FontName(data)
                    | Element::Name(data) => *data = Some(text),
                    // float fields
                    Element::R(data)
                    | Element::G(data)
                    | Element::B(data)
                    | Element::A(data)
                    | Element::Relative(data) => {
                        *data = Some(
                            text.parse::<f32>()
                                .context("Could not parse float value for text in element")?,
                        )
                    }
                    Element::Constant(data) => {
                        *data =
                            Some(text.parse::<i32>().context(
                                "Could not parse unsigned int value for text in element",
                            )?)
                    }
                    // unsigned int fields
                    Element::WindowWidth(data)
                    | Element::PixelSize(data)
                    | Element::WindowHeight(data) => {
                        *data =
                            Some(text.parse::<u32>().context(
                                "Could not parse unsigned int value for text in element",
                            )?)
                    }
                    Element::Static(data) => {
                        *data = match text.as_str() {
                            "true" => true,
                            "false" => false,
                            fmt => return Err(anyhow!("Unknown resource hint {:?}", fmt)),
                        }
                    }
                    Element::Format(data) => {
                        *data = match text.as_str() {
                            "ARGB8888" => Some(dom::Format::ARGB8888),
                            fmt => return Err(anyhow!("Unknown image format {:?}", fmt)),
                        }
                    }
                    _ => {
                        // If this was hit we have a parsing issue. The XML stream specified
                        // data in an element type that does not have any data (such as ResourceMap)
                        return Err(anyhow!(
                            "Element type {:?} does not expect text inside its XML tag",
                            node
                        ));
                    }
                }
            }
            e => {
                return Err(anyhow!(
                    "XML Event of incorrect type. Expected text inside this element, found: {:?}",
                    e
                ))
            }
        }

        Ok(())
    }

    /// Parse a quick_xml stream into a Dakota DOM tree
    ///
    /// This initializes our elements to be later processed into layout nodes.
    fn parse_xml<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Result<dom::DakotaDOM> {
        let mut buf = Vec::new();

        // The DakotaId we are currently populating
        let mut id = None;
        let mut ret = None;
        // The node type (Element) of the current XML node
        let mut node = None;
        let mut stack = Vec::new();

        loop {
            match reader.read_event(&mut buf) {
                Ok(Event::Start(e)) => {
                    log::verbose!("XML EVENT: {:#?}", e);
                    // We are entering a new tag, push the old one
                    stack.push((id.clone(), node));

                    // extract our element type from the XML tag name
                    let ty = Element::from_bytes(e.name().as_ref())?;

                    if let Some(new_id) = self.needs_new_id(&ty)? {
                        id = Some(new_id);
                        // Stash the first id we allocate, this will be the root id
                        // for what the XML stream specifies that we will return.
                        //
                        // This means we require a "root" node in each XML stream parsed
                        if ret.is_none() {
                            ret = id.clone();
                        }
                    }
                    node = Some(ty);
                }
                Ok(Event::End(_e)) => {
                    log::verbose!("XML EVENT: {:#?}", _e);
                    // Save a copy of the XML element that just ended
                    let old_id = id.clone();
                    let old_node = node;

                    // Pop our parent node info back into focus
                    match stack.pop() {
                        // If we have finished the dakota element then we can return
                        Some((None, None)) | None => {
                            // The Dakota object is our toplevel object. Since we are
                            // done processing here the old_* variables will be our DOM,
                            match old_node {
                                Some(Element::Dakota {
                                    version,
                                    window,
                                    root_element,
                                }) => {
                                    // Return a DOM object
                                    return Ok(dom::DakotaDOM {
                                        version: version
                                            .clone()
                                            .ok_or(anyhow!("Dakota missing field version"))?,
                                        window: window
                                            .clone()
                                            .ok_or(anyhow!("Dakota missing field version"))?,
                                        root_element: root_element
                                            .clone()
                                            .ok_or(anyhow!("Dakota missing field version"))?,
                                    });
                                }
                                _ => {
                                    return Err(anyhow!(
                                        "Toplevel XML tag is not the Dakota object"
                                    ))
                                }
                            };
                        }
                        // Update our current node with the popped value
                        Some((i, n)) => {
                            id = i;
                            node = n;
                        }
                    }

                    // Validate old_node and add (old_id, old_node) as children of (id, node)
                    // We can expect id and node to be valid here since we just checked it
                    self.add_child(
                        id.as_ref().unwrap(),
                        node.as_mut().unwrap(),
                        old_id.as_ref().unwrap(),
                        old_node.as_ref().unwrap(),
                    )
                    .context(format!("Error at position {}:", reader.buffer_position(),))?;
                }
                Ok(Event::Eof) => break,
                // Unknown events and errors just get debug prints for now
                Ok(e) => {
                    log::verbose!("XML EVENT: {:#?}", e);
                    // We can expect id and node to be valid here otherwise it is
                    // an implementation error
                    self.handle_xml_text(node.as_mut().unwrap(), &e)
                        .context(format!(
                            "Error at position {} while processing element {:?}",
                            reader.buffer_position(),
                            node
                        ))?;
                }
                Err(e) => {
                    return Err(anyhow!(
                        "Error at position {}: {:?}",
                        reader.buffer_position(),
                        e
                    ))
                }
            }

            // Clear the buffer we passed to quick_xml
            buf.clear();
        }

        return Err(anyhow!("Error: no elements found in XML"));
    }
}

impl Scene {
    /// Parse a quick_xml stream into a Dakota DOM tree
    ///
    /// This initializes our elements to be later processed into layout nodes.
    fn parse_xml<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Result<()> {
        let mut trans = ParserTransaction {
            pt_dev: &self.d_dev,
            pt_ecs_inst: self.d_ecs_inst.clone(),
            pt_resource_ecs_inst: self.d_resource_ecs_inst.clone(),
            pt_node_types: self.d_node_types.snapshot(),
            pt_resources: self.d_resources.snapshot(),
            pt_resource_hints: self.d_resource_hints.snapshot(),
            pt_resource_thundr_image: self.d_resource_thundr_image.snapshot(),
            pt_resource_color: self.d_resource_color.snapshot(),
            pt_fonts: self.d_fonts.snapshot(),
            pt_text_font: self.d_text_font.snapshot(),
            pt_texts: self.d_texts.snapshot(),
            pt_glyphs: self.d_glyphs.snapshot(),
            pt_is_viewport: self.d_is_viewport.snapshot(),
            pt_contents: self.d_contents.snapshot(),
            pt_widths: self.d_widths.snapshot(),
            pt_heights: self.d_heights.snapshot(),
            pt_offsets: self.d_offsets.snapshot(),
            pt_children: self.d_children.snapshot(),
            pt_font_instances: &mut self.d_font_instances,
            pt_name_to_id_map: HashMap::new(),
            pt_font_name_to_id_map: HashMap::new(),
            pt_freetype: &self.d_freetype,
            pt_fontconfig: &self.d_fontconfig,
            pt_unbounded_subsurf: self.d_unbounded_subsurf.snapshot(),
        };

        self.d_dom = Some(trans.parse_xml(reader)?);
        trans.precommit();
        trans.commit();

        Ok(())
    }

    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// string of XML.
    pub fn load_xml_str(&mut self, xml: &str) -> Result<()> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }

    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// arbitrary reader type that serves XML.
    pub fn load_xml_reader<B: BufRead>(&mut self, reader: B) -> Result<()> {
        let mut reader = Reader::from_reader(reader);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }
}
