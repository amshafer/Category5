/// SDL2 backend platform
///
/// This handles all window systems using SDL2
use super::Platform;
use crate::dom;
use crate::dom::DakotaDOM;
use crate::utils::fdwatch::FdWatch;
use crate::{
    event::{AxisSource, EventSystem},
    DakotaError, Result,
};

extern crate sdl2;
use crate::input::*;
use sdl2::event::{Event, WindowEvent};

const SCROLL_SENSITIVITY: f64 = 32.0;

#[allow(dead_code)]
pub struct SDL2Plat {
    sdl: sdl2::Sdl,
    sdl_video_sys: sdl2::VideoSubsystem,
    sdl_window: sdl2::video::Window,
    sdl_event_pump: sdl2::EventPump,
    /// last known mouse
    ///
    /// Because the mouse may disappear off one edge of the SDL window
    /// and re-appear on another, we have to manually calculate
    /// relative mouse motions using the last known mouse location.
    sdl_mouse_pos: (f64, f64),
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
            sdl_mouse_pos: (0.0, 0.0),
        })
    }
}

#[cfg(feature = "sdl")]
impl Platform for SDL2Plat {
    fn get_th_surf_type<'a>(&mut self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::SDL2(&self.sdl_video_sys, &self.sdl_window))
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
        watch: Option<&mut FdWatch>,
    ) -> std::result::Result<bool, DakotaError> {
        let mut is_ood = false;
        let mut needs_redraw = false;

        // Returns Result<bool, DakotaError>, true if we should terminate
        //
        // We have to pass in the mouse position (aka sdl_mouse_pos) due
        // to the borrow checker. We are referencing other parts of self and using
        // self here will bind the entire self obj in this closure
        let mut handle_event = |raw_event, user_fd, mouse_pos: &mut (f64, f64)| {
            // raw_event will be Some if we have a valid SDL event
            if let Some(event) = raw_event {
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
                        evsys.add_event_key_down(key, mods);
                    }
                    Event::KeyUp {
                        keycode, keymod, ..
                    } => {
                        let key = convert_sdl_keycode_to_dakota(keycode.unwrap());
                        let mods = convert_sdl_mods_to_dakota(keymod);
                        evsys.add_event_key_up(key, mods);
                    }
                    // handle pointer inputs. This just looks like the above keyboard
                    Event::MouseButtonDown { mouse_btn, .. } => {
                        let button = convert_sdl_mouse_to_dakota(mouse_btn);
                        evsys.add_event_mouse_button_down(button);
                    }
                    Event::MouseButtonUp { mouse_btn, .. } => {
                        let button = convert_sdl_mouse_to_dakota(mouse_btn);
                        evsys.add_event_mouse_button_up(button);
                    }
                    Event::MouseWheel { x, y, .. } => evsys.add_event_scroll(
                        Some(x as f64 * SCROLL_SENSITIVITY),
                        Some(y as f64 * SCROLL_SENSITIVITY),
                        (0.0, 0.0), // v120 value unspecified
                        AxisSource::Wheel,
                    ),
                    Event::MouseMotion { x, y, .. } => {
                        evsys.add_event_scroll(
                            Some(x as f64 - mouse_pos.0),
                            Some(y as f64 - mouse_pos.1),
                            (0.0, 0.0),
                            AxisSource::Wheel,
                        );

                        // Update our mouse position
                        mouse_pos.0 = x as f64;
                        mouse_pos.1 = y as f64;
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
                        WindowEvent::Resized { .. } => is_ood = true,
                        WindowEvent::SizeChanged { .. } => is_ood = true,
                        WindowEvent::Exposed { .. } => needs_redraw = true,
                        _ => {}
                    },
                    _ => {}
                }
            }

            // If this function was called because of our FdWatch, add the
            // right event now
            if user_fd {
                evsys.add_event_user_fd();
            }

            Ok(())
        };

        // There are two modes we need to consider for polling for SDL events, since
        // it doesn't follow a unix style: 1) if we are waiting for just SDL, 2) if we
        // are waiting for SDL and some file descriptors
        //
        // In the first case we should use SDL's SDL_WaitEvent, since it will save
        // power by not busy looping. If we need to wait for some fds then we have no
        // choice but to busy loop ourselves since SDL doesn't have a good way for us
        // to deal with this. If this becomes a problem hopefuly SDL3 has a good way to
        // deal with it..
        if let Some(fds) = watch {
            loop {
                // Wait for the first readable fd
                if fds.wait_for_events(Some(0)) {
                    handle_event(None, true, &mut self.sdl_mouse_pos)?;
                    break;
                }

                // Or wait for the first SDL event
                if let Some(ev) = self.sdl_event_pump.poll_event() {
                    handle_event(Some(ev), false, &mut self.sdl_mouse_pos)?;
                    break;
                }

                // Don't waste all the CPU
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        } else {
            // First block for the next event
            handle_event(
                Some(match timeout {
                    // If we are waiting a certain amount of time, tell SDL. If
                    // it returns an event, great, handle it.
                    // If not, then just return without handling.
                    Some(timeout) => match self.sdl_event_pump.wait_event_timeout(timeout) {
                        Some(event) => event,
                        None => return Ok(needs_redraw),
                    },
                    // No timeout was given, so we wait indefinitely
                    None => self.sdl_event_pump.wait_event(),
                }),
                false,
                &mut self.sdl_mouse_pos,
            )?;
        }

        // Now drain the available events before returning
        // control to the main dakota dispatch loop.
        for event in self.sdl_event_pump.poll_iter() {
            handle_event(Some(event), false, &mut self.sdl_mouse_pos)?
        }

        match is_ood {
            true => Err(DakotaError::OUT_OF_DATE),
            false => Ok(needs_redraw),
        }
    }
}
