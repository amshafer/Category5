use crate::dom;
use crate::{Context, Result};

#[cfg(feature = "wayland")]
extern crate wayc;
#[cfg(feature = "wayland")]
use wayc::Wayc;

#[cfg(any(unix, macos))]
extern crate sdl2;
#[cfg(any(unix, macos))]
use sdl2::{event::Event, keyboard::Keycode};

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;

    fn set_output_params(&mut self, win: &dom::Window) -> Result<()>;

    /// Returns true if we should terminate i.e. the window has been closed.
    fn run<F: FnMut()>(&mut self, func: F) -> Result<bool>;
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
    fn set_output_params(&mut self, win: &dom::Window) -> Result<()> {
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
pub struct SDL2Plat {
    sdl: sdl2::Sdl,
    sdl_video_sys: sdl2::VideoSubsystem,
    sdl_canvas: sdl2::render::Canvas<sdl2::video::Window>,
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
            .position_centered()
            .build()?;
        let mut event_pump = sdl_context.event_pump().unwrap();
        let mut canvas = window.into_canvas().build().unwrap();
        Ok(Self {
            sdl: sdl_context,
            sdl_video_sys: video_subsystem,
            sdl_event_pump: event_pump,
            sdl_canvas: canvas,
        })
    }
}

#[cfg(feature = "sdl")]
impl Platform for SDL2Plat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::SDL2(self.sdl_canvas.window()))
    }

    fn set_output_params(&mut self, win: &dom::Window) -> Result<()> {
        let mut sdl_win = self.sdl_canvas.window_mut();
        sdl_win.set_title(&win.title);
        sdl_win.set_size(win.width, win.height);
        Ok(())
    }

    fn run<F>(&mut self, mut func: F) -> Result<bool>
    where
        F: FnMut(),
    {
        for event in self.sdl_event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => return Ok(true),
                _ => {}
            }
        }
        Ok(false)
    }
}
