use crate::dom;
use crate::{DakotaError, Result};

#[cfg(feature = "wayland")]
extern crate wayc;
#[cfg(feature = "wayland")]
use wayc::Wayc;

#[cfg(any(unix, macos))]
extern crate sdl2;
#[cfg(any(unix, macos))]
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode,
};

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;

    fn set_output_params(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()>;

    /// Returns true if we should terminate i.e. the window has been closed.
    fn run(&mut self, timeout: Option<u32>) -> std::result::Result<bool, DakotaError>;
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
    fn set_output_params(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()> {
        println!("set_output_params on wayland is unimplemented");
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

#[cfg(feature = "sdl")]
#[allow(dead_code)]
pub struct SDL2Plat {
    sdl: sdl2::Sdl,
    sdl_video_sys: sdl2::VideoSubsystem,
    sdl_window: sdl2::video::Window,
    sdl_event_pump: sdl2::EventPump,
}

#[cfg(feature = "sdl")]
impl SDL2Plat {
    pub fn new() -> Result<Self> {
        // SDL goodies
        let sdl_context = sdl2::init().unwrap();
        let video_subsystem = sdl_context.video().unwrap();
        let window = video_subsystem
            .window("dakota", 800, 600)
            .vulkan()
            .resizable()
            .position_centered()
            .build()?;
        let event_pump = sdl_context.event_pump().unwrap();
        Ok(Self {
            sdl: sdl_context,
            sdl_video_sys: video_subsystem,
            sdl_event_pump: event_pump,
            sdl_window: window,
        })
    }
}

#[cfg(feature = "sdl")]
impl Platform for SDL2Plat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::SDL2(&self.sdl_window))
    }

    fn set_output_params(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()> {
        let sdl_win = &mut self.sdl_window;
        sdl_win.set_title(&win.title)?;
        sdl_win.set_size(dims.0, dims.1)?;
        Ok(())
    }

    /// Block and handle all available events from SDL2. If timeout
    /// is specified it will be passed to SDL's wait_event_timeout function.
    fn run(&mut self, timeout: Option<u32>) -> std::result::Result<bool, DakotaError> {
        // Returns Result<bool, DakotaError>, true if we should terminate
        let handle_event = |event| {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(true),
                Event::Window {
                    timestamp: _,
                    window_id: _,
                    win_event,
                } => match win_event {
                    // check redraw requested?
                    WindowEvent::Resized { .. } => return Err(DakotaError::OUT_OF_DATE),
                    _ => {}
                },
                _ => {}
            };

            return Ok(false);
        };

        // First block for the next event
        if handle_event(match timeout {
            // If we are waiting a certain amount of time, tell SDL. If
            // it returns an event, great, handle it.
            // If not, then just return without handling.
            Some(timeout) => match self.sdl_event_pump.wait_event_timeout(timeout) {
                Some(event) => event,
                None => return Ok(false),
            },
            // No timeout was given, so we wait indefinitely
            None => self.sdl_event_pump.wait_event(),
        })? {
            return Ok(true);
        }

        // Now drain the available events before returning
        // control to the main dakota dispatch loop.
        for event in self.sdl_event_pump.poll_iter() {
            if handle_event(event)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
