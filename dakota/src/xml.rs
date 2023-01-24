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

use std::io::BufRead;
use utils::log;

/// A list of element names
///
/// This allows us to set and compare the currently processed element
/// without having to do expensive string ops
enum Element {
    El,
    Text(Vec<dom::TextItem>),
    Window {
        title: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        events: dom::WindowEvents,
        root_element: Option<DakotaId>,
    },
    Dakota {
        version: Option<String>,
        window: Option<dom::Window>,
    },
    Version(Option<String>),
    Name(Option<String>),
    Width(Option<u32>),
    Height(Option<u32>),
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
    Constant(Option<u32>),
    R(Option<f32>),
    G(Option<f32>),
    B(Option<f32>),
    A(Option<f32>),
    AbsPath(Option<String>),
    RelPath(Option<String>),
    Image(Option<dom::Format>, Option<dom::Data>),
    Format(Option<dom::Format>),
    Data(Option<dom::Data>),
    ResourceMap,
    Resource {
        name: Option<String>,
        image: Option<dom::Image>,
        color: Option<dom::Color>,
        hints: Option<dom::Hints>,
    },
    Size(Option<dom::Value>, Option<dom::Value>),
    Offset(Option<dom::Value>, Option<dom::Value>),
    P(Option<String>),
    Content,
    Event(Vec<String>, Option<String>, Vec<String>),
    Group(Option<String>),
    Arg(Option<String>),
    WindowEvents {
        resize: Option<dom::Event>,
        redraw_complete: Option<dom::Event>,
        closed: Option<dom::Event>,
    },
    Resize(Option<dom::Event>),
    RedrawComplete(Option<dom::Event>),
    Closed(Option<dom::Event>),
}

impl Element {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let ret = match bytes {
            b"el" => Self::El,
            b"text" => Self::Text(Vec::new()),
            b"window" => Self::Window {
                title: None,
                width: None,
                height: None,
                events: dom::WindowEvents::default(),
                root_element: None,
            },
            b"dakota" => Self::Dakota {
                version: None,
                window: None,
            },
            b"version" => Self::Version(None),
            b"name" => Self::Name(None),
            b"width" => Self::Width(None),
            b"height" => Self::Height(None),
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
            b"data" => Self::Data(None),
            b"resourceMap" => Self::ResourceMap,
            b"resource" => Self::Resource {
                name: None,
                image: None,
                color: None,
                hints: None,
            },
            b"size" => Self::Size(None, None),
            b"p" => Self::P(None),
            b"offset" => Self::Offset(None, None),
            b"content" => Self::Content,
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
            Self::Resource {
                name: _,
                image: _,
                color: _,
                hints: _,
            } => true,
            Self::ResourceMap | Self::El => true,
            Self::Dakota {
                version: _,
                window: _,
            } => true,
            _ => false,
        }
    }
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

    /// Parse a quick_xml stream into a Dakota DOM tree
    ///
    /// This initializes our elements to be later processed into layout nodes.
    fn parse_xml<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Result<DakotaId> {
        let mut buf = Vec::new();

        // The DakotaId we are currently populating
        let mut id = None;
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
                }
                Ok(Event::Eof) => break,
                // Unknown events and errors just get debug prints for now
                Ok(e) => {
                    log::verbose!("XML EVENT: {:#?}", e)
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

        unimplemented!();
    }
}
