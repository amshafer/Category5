extern crate quick_xml;
use crate::dom::DakotaDOM;
use crate::{Context, Dakota, Result};

impl Dakota {
    pub fn load_xml_str(&mut self, xml: &str) -> Result<()> {
        let dom: DakotaDOM =
            quick_xml::de::from_str(xml).context("Failed to parse XML dakota string")?;

        self.d_dom = Some(dom);

        Ok(())
    }
}
