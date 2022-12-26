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

use crate::dom::*;
use crate::utils::anyhow;
use crate::{Context, Dakota, Result};

use std::io::BufRead;

impl<'a> Dakota<'a> {
    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// string of XML.
    pub fn load_xml_str(&mut self, xml: &str) -> Result<DakotaDOM> {
        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }

    /// Parse a string of Dakota XML
    ///
    /// This provides a way to initialize a full application view from a
    /// arbitrary reader type that serves XML.
    pub fn load_xml_reader<B: BufRead>(&mut self, reader: B) -> Result<DakotaDOM> {
        let mut reader = Reader::from_reader(reader);
        reader.trim_text(true);

        self.parse_xml(&mut reader)
            .context("Failed to parse XML dakota string")
    }

    /// Parse a quick_xml stream into a Dakota DOM tree
    ///
    /// This initializes our elements to be later processed into layout nodes.
    fn parse_xml<R: BufRead>(&mut self, reader: &mut Reader<R>) -> Result<DakotaDOM> {
        let mut buf = Vec::new();

        loop {
            match reader.read_event(&mut buf) {
                Err(e) => {
                    return Err(anyhow!(
                        "Error at position {}: {:?}",
                        reader.buffer_position(),
                        e
                    ))
                }
                Ok(Event::Eof) => break,
                Ok(e) => println!("XML EVENT: {:#?}", e),
            }

            // Clear the buffer we passed to quick_xml
            buf.clear();
        }

        unimplemented!();
    }
}
