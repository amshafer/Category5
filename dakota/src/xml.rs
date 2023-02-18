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

use crate::dom;
use crate::utils::anyhow;
use crate::{Context, Dakota, DakotaId, Result};

use std::collections::HashMap;
use std::io::BufRead;
use std::rc::Rc;
use utils::log;

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
    Text(Vec<dom::TextItem>),
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
    Title(Option<String>),
    Width(Option<dom::Value>),
    Height(Option<dom::Value>),
    WindowWidth(Option<u32>),
    WindowHeight(Option<u32>),
    Layout(Option<DakotaId>),
    Color {
        r: Option<f32>,
        g: Option<f32>,
        b: Option<f32>,
        a: Option<f32>,
    },
    X(Option<dom::Value>),
    Y(Option<dom::Value>),
    Relative(Option<f32>),
    Constant(Option<u32>),
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
    ResourceDefinition {
        name: Option<String>,
        image: Option<dom::Image>,
        color: Option<dom::Color>,
        hints: Option<dom::Hints>,
    },
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
            b"text" => Self::Text(Vec::new()),
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
            b"title" => Self::Title(None),
            b"width" => Self::Width(None),
            b"height" => Self::Height(None),
            b"relative" => Self::Relative(None),
            b"constant" => Self::Constant(None),
            b"x" => Self::X(None),
            b"y" => Self::Y(None),
            b"layout" => Self::Layout(None),
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
            b"define_resource" => Self::ResourceDefinition {
                name: None,
                image: None,
                color: None,
                hints: None,
            },
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
            _ => {
                return Err(anyhow!(
                    "Element name {} is not a valid element name",
                    std::str::from_utf8(bytes)?
                ))
            }
        };

        Ok(ret)
    }

    /// Returns true if this element type will have a DakotaId created for
    /// it. False if no.
    fn needs_new_id(&self) -> bool {
        match self {
            Self::ResourceDefinition {
                name: _,
                image: _,
                color: _,
                hints: _,
            } => true,
            Self::ResourceMap
            | Self::El {
                x: _,
                y: _,
                width: _,
                height: _,
            } => true,
            Self::Dakota {
                version: _,
                window: _,
                root_element: _,
            } => true,
            _ => false,
        }
    }

    fn convert_to_dom_value(&self) -> Result<dom::Value> {
        match self {
            Element::Relative(float) => Ok(dom::Value::Relative(dom::Relative::new(
                float.ok_or(anyhow!("No data provided to <relative> tag"))?,
            ))),
            Element::Constant(int) => Ok(dom::Value::Constant(dom::Constant::new(
                int.ok_or(anyhow!("No data provided to <constant> tag"))?,
            ))),
            e => return Err(anyhow!("Unexpected child element: {:?}", e)),
        }
    }

    fn get_dom_event(&self) -> Result<dom::Event> {
        match self {
            Element::Event { groups, id, args } => Ok(dom::Event {
                groups: groups.clone(),
                id: id.clone(),
                args: Rc::new(args.clone()),
            }),
            e => return Err(anyhow!("Unexpected child element: {:?}", e)),
        }
    }
}

/// Data for this round of parsing
///
/// This will be freed after the XML stream is processed
struct ParseData {
    /// This maps the string names for resource found in the
    /// XML document to DakotaIds that represent those resources.
    ///
    /// We need this since the resource section may be processed
    /// after the elements for some reason. We need to have a way
    /// to translate from strings to ids so that we can set up
    /// all the elements to reference resources without holding
    /// a giant array of resources somewhere.
    name_to_id_map: HashMap<String, DakotaId>,
}

impl<'a> Dakota<'a> {
    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// string of XML.
    pub fn load_xml_str(&mut self, xml: &str) -> Result<DakotaId> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }

    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// arbitrary reader type that serves XML.
    pub fn load_xml_reader<B: BufRead>(&mut self, reader: B) -> Result<DakotaId> {
        let mut reader = Reader::from_reader(reader);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }

    fn get_id_for_name(&mut self, parse: &mut ParseData, name: &str) -> DakotaId {
        if !parse.name_to_id_map.contains_key(name) {
            parse
                .name_to_id_map
                .insert(name.to_string(), self.d_ecs_inst.add_entity());
        }

        parse.name_to_id_map.get(name).unwrap().clone()
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
        parse: &mut ParseData,
        id: &DakotaId,
        node: &mut Element,
        old_id: &DakotaId,
        old_node: &Element,
    ) -> Result<()> {
        match node {
            // Element
            // -------------------------------------------------------
            Element::El {
                x,
                y,
                width,
                height,
            } => match old_node {
                Element::Resource(name) => {
                    let resource_id = self.get_id_for_name(
                        parse,
                        name.as_ref()
                            .ok_or(anyhow!("Element was not assigned a resource"))?,
                    );
                    self.d_resources.set(id, resource_id)
                }
                Element::El {
                    x: _,
                    y: _,
                    width: _,
                    height: _,
                } => {
                    // Add old_id as a child element
                    if self.d_children.get_mut(id).is_none() {
                        self.d_children.set(id, Vec::new());
                    }

                    self.d_children.get_mut(id).unwrap().push(old_id.clone());
                }
                Element::X(val) => *x = *val,
                Element::Y(val) => *y = *val,
                Element::Width(val) => *width = *val,
                Element::Height(val) => *height = *val,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
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
                Element::Window {
                    title,
                    width,
                    height,
                    events,
                } => {
                    *window = Some(dom::Window {
                        title: title
                            .as_ref()
                            .ok_or(anyhow!("Window does not contain title field"))?
                            .clone(),
                        width: width
                            .ok_or(anyhow!("Window does not contain window_width field"))?,
                        height: height
                            .ok_or(anyhow!("Window does not contain window_height field"))?,
                        events: events.clone(),
                    })
                }
                Element::Layout(data) => *root_element = *data,
                e => return Err(anyhow!("Unexpected child element: {:?}", e)),
            },
            // -------------------------------------------------------
            Element::Text(data) => {}
            Element::Width(data) | Element::Height(data) => {
                *data = Some(old_node.convert_to_dom_value()?)
            }
            Element::Layout(data) => {}
            Element::Color { r, g, b, a } => {}
            Element::Image(format, data) => {}
            Element::Format(data) => {}
            Element::Data(data) => {}
            Element::ResourceMap => {}
            Element::ResourceDefinition {
                name,
                image,
                color,
                hints,
            } => {}
            Element::Size(width, height) => {}
            Element::Offset(x, y) => {}
            Element::Content(data) => {}
            Element::Event { groups, id, args } => {}
            Element::WindowEvents(events) => {}
            Element::Resize(data) => {}
            Element::RedrawComplete(data) => {}
            Element::Closed(data) => {}
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
                    | Element::Name(data) => *data = Some(text),
                    // float fields
                    Element::R(data)
                    | Element::G(data)
                    | Element::B(data)
                    | Element::A(data)
                    | Element::Relative(data) => *data = Some(text.parse::<f32>()?),
                    // unsigned int fields
                    Element::Constant(data)
                    | Element::WindowWidth(data)
                    | Element::WindowHeight(data) => *data = Some(text.parse::<u32>()?),
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
    fn parse_xml<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Result<DakotaId> {
        let mut buf = Vec::new();
        // Our parsing data
        let mut parse = ParseData {
            name_to_id_map: HashMap::new(),
        };

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

                    if ty.needs_new_id() {
                        id = Some(self.d_ecs_inst.add_entity());
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
                Ok(Event::End(e)) => {
                    log::verbose!("XML EVENT: {:#?}", e);
                    let old_id = id.clone();
                    let old_node = node;

                    // Pop our parent node info back into focus
                    match stack.pop() {
                        // If we have reached the end break from our loop
                        Some((None, None)) | None => break,
                        Some((i, n)) => {
                            id = i;
                            node = n;
                        }
                    }

                    // Validate old_node and add (old_id, old_node) as children of (id, node)
                    // We can expect id and node to be valid here since we just checked it
                    self.add_child(
                        &mut parse,
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

        match ret {
            Some(val) => Ok(val),
            None => Err(anyhow!("Error: no elements found in XML")),
        }
    }
}
