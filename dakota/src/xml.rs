extern crate quick_xml;
use crate::dom::DakotaDOM;
use crate::{Context, Dakota, Result};

use std::io::BufRead;

impl Dakota {
    pub fn load_xml_str(&mut self, xml: &str) -> Result<DakotaDOM> {
        let dom: DakotaDOM =
            quick_xml::de::from_str(xml).context("Failed to parse XML dakota string")?;

        Ok(dom)
    }

    pub fn load_xml_reader<B: BufRead>(&mut self, reader: B) -> Result<DakotaDOM> {
        let dom: DakotaDOM =
            quick_xml::de::from_reader(reader).context("Failed to parse XML dakota string")?;

        Ok(dom)
    }
}
