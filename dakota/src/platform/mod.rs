use crate::{Context, Result};

#[cfg(feature = "wayland")]
extern crate wayc;
#[cfg(feature = "wayland")]
use wayc::Wayc;

#[cfg(feature = "macos")]
extern crate winit;
#[cfg(feature = "macos")]
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;
}

#[cfg(feature = "wayland")]
pub struct WLPlat {
    wp_wayc: Wayc,
}

#[cfg(feature = "wayland")]
impl WLPlat {
    fn new() -> Result<Self> {
        let mut wayc = Wayc::new().context("Failed to initialize wayland")?;
        let wl_surf = wayc
            .create_surface()
            .context("Failed to create wayland surface")?;

        Self {
            wp_wayc: wayc,
            wp_surf: wl_surf,
        }
    }
}

#[cfg(feature = "wayland")]
impl Platform for WLPlat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::Wayland(
            self.wp_wayc.get_wl_display(),
            self.wp_surf.borrow().get_wl_surface().detach(),
        ))
    }
}

#[cfg(feature = "macos")]
pub struct MacosPlat {
    mp_event_loop: EventLoop<()>,
    mp_window: winit::window::Window,
}

#[cfg(feature = "macos")]
impl MacosPlat {
    pub fn new() -> Result<Self> {
        let event_loop = EventLoop::new();
        let window = WindowBuilder::new()
            .build(&event_loop)
            .context("Could not create window with winit")?;

        Ok(Self {
            mp_event_loop: event_loop,
            mp_window: window,
        })
    }
}

#[cfg(feature = "macos")]
impl Platform for MacosPlat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::MacOS(&self.mp_window))
    }
}
