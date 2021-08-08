extern crate wayc;
use wayc::Wayc;

extern crate thundr as th;

extern crate anyhow;
pub use anyhow::{Context, Result};

extern crate serde;

mod dom;
use dom::DakotaDOM;

mod xml;

struct Dakota {
    d_wayc: Wayc,
    d_thundr: th::Thundr,
    d_dom: Option<DakotaDOM>,
}

impl Dakota {
    pub fn new() -> Result<Self> {
        let mut wayc = Wayc::new().context("Failed to initialize wayland")?;
        let wl_surf = wayc
            .create_surface()
            .context("Failed to create wayland surface")?;

        let info = th::CreateInfo::builder()
            .surface_type(th::SurfaceType::Wayland(
                wayc.get_wl_display(),
                wl_surf.borrow().get_wl_surface().detach(),
            ))
            .build();

        let thundr = th::Thundr::new(&info).context("Failed to initialize Thundr")?;

        Ok(Self {
            d_wayc: wayc,
            d_thundr: thundr,
            d_dom: None,
        })
    }
}
