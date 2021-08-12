extern crate thundr as th;

extern crate utils;
pub use utils::{anyhow, Context, Result};

extern crate serde;

pub mod dom;
use dom::DakotaDOM;

mod platform;
use platform::Platform;

mod xml;

pub struct Dakota {
    d_plat: Box<dyn Platform>,
    d_thundr: th::Thundr,
    d_dom: Option<DakotaDOM>,
}

impl Dakota {
    pub fn new() -> Result<Self> {
        #[cfg(feature = "wayland")]
        let mut plat = platform::WLPlat::new()?;

        #[cfg(feature = "macos")]
        let mut plat = platform::MacosPlat::new()?;

        #[cfg(feature = "xcb")]
        let mut plat = platform::XCBPlat::new()?;

        let info = th::CreateInfo::builder()
            .surface_type(plat.get_th_surf_type()?)
            .build();

        let thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        Ok(Self {
            d_plat: Box::new(plat),
            d_thundr: thundr,
            d_dom: None,
        })
    }

    /// Completely flush the thundr surfaces/images and recreate the scene
    fn refresh_thundr(&mut self) -> Result<()> {
        let dom = match &mut self.d_dom {
            Some(dom) => dom,
            None => {
                return Err(anyhow!(
                    "A scene is not loaded in Dakota. Please load one from xml",
                ))
            }
        };

        for lay in dom.layout.elements.iter() {}

        return Ok(());
    }
}
