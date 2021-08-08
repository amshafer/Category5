extern crate thundr as th;

extern crate utils;
pub use utils::{Context, Result};

extern crate serde;

mod dom;
use dom::DakotaDOM;

mod platform;
use platform::Platform;

mod xml;

struct Dakota {
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
}
