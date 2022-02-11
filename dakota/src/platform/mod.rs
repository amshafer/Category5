use crate::dom;
use crate::dom::DakotaDOM;
use crate::{event::EventSystem, DakotaError, Result};

#[cfg(feature = "wayland")]
extern crate wayc;
#[cfg(feature = "wayland")]
use wayc::Wayc;

#[cfg(any(unix, macos))]
extern crate sdl2;
use crate::input::*;
#[cfg(any(unix, macos))]
use sdl2::event::{Event, WindowEvent};

pub trait Platform {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType>;

    fn set_output_params(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()>;

    /// Returns true if we should terminate i.e. the window has been closed.
    fn run(
        &mut self,
        evsys: &mut EventSystem,
        dom: &DakotaDOM,
        timeout: Option<u32>,
    ) -> std::result::Result<(), DakotaError>;
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
    fn run(
        &mut self,
        evsys: &mut EventSystem,
        dom: &DakotaDOM,
        timeout: Option<u32>,
    ) -> std::result::Result<(), DakotaError> {
        // Returns Result<bool, DakotaError>, true if we should terminate
        let mut handle_event = |event| {
            match event {
                // Tell the window to exit if the user closed it
                Event::Quit { .. } => evsys.add_event_window_closed(dom),
                // Here we record events for our keystrokes
                //
                // This requires converting the raw keycodes from sdl2 into an
                // enum that we control. See input.rs for how this is done. We
                // also wrap the Keyboard Modifiercodes in a similar way
                Event::KeyDown {
                    keycode, keymod, ..
                } => {
                    let key = convert_sdl_keycode_to_dakota(keycode.unwrap());
                    let mods = convert_sdl_mods_to_dakota(keymod);
                    evsys.add_event_key_down(dom, key, mods);
                }
                Event::KeyUp {
                    keycode, keymod, ..
                } => {
                    let key = convert_sdl_keycode_to_dakota(keycode.unwrap());
                    let mods = convert_sdl_mods_to_dakota(keymod);
                    evsys.add_event_key_up(dom, key, mods);
                }
                // handle pointer inputs. This just looks like the above keyboard
                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } => {
                    let button = convert_sdl_mouse_to_dakota(mouse_btn);
                    evsys.add_event_mouse_button_down(dom, button, x, y);
                }
                Event::MouseButtonUp {
                    mouse_btn, x, y, ..
                } => {
                    let button = convert_sdl_mouse_to_dakota(mouse_btn);
                    evsys.add_event_mouse_button_up(dom, button, x, y);
                }

                // Now we have window events. There's really only one we need to
                // pay attention to here, and it's the resize event. Thundr is
                // going to check for OUT_OF_DATE, but it's possible that the toolkit
                // (SDL) might need refreshing while libvulkan doesn't yet know about
                // it.
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

            Ok(())
        };

        // First block for the next event
        handle_event(match timeout {
            // If we are waiting a certain amount of time, tell SDL. If
            // it returns an event, great, handle it.
            // If not, then just return without handling.
            Some(timeout) => match self.sdl_event_pump.wait_event_timeout(timeout) {
                Some(event) => event,
                None => return Ok(()),
            },
            // No timeout was given, so we wait indefinitely
            None => self.sdl_event_pump.wait_event(),
        })?;

        // Now drain the available events before returning
        // control to the main dakota dispatch loop.
        for event in self.sdl_event_pump.poll_iter() {
            handle_event(event)?
        }
        Ok(())
    }
}
