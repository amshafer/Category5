/// SDL2 backend platform
///
/// This handles all window systems using SDL2
use super::{OutputPlatform, Platform};
use crate::dom;
use crate::utils::{fdwatch::FdWatch, log};
use crate::{
    event::{AxisSource, GlobalEventSystem, OutputEventSystem, PlatformEventSystem, RawKeycode},
    OutputId, Result,
};

extern crate sdl2;
extern crate sdl2_sys;
use crate::input::*;
use sdl2::event::{Event, WindowEvent};

extern crate xkbcommon;
use std::os::fd::RawFd;
use std::sync::{Arc, RwLock};
use xkbcommon::xkb;

const SCROLL_SENSITIVITY: f64 = 32.0;

/// Common SDL2 dispatch backend
#[allow(dead_code)]
pub struct SDL2Plat {
    sdl: sdl2::Sdl,
    sdl_event_pump: sdl2::EventPump,
    /// last known mouse
    ///
    /// Because the mouse may disappear off one edge of the SDL window
    /// and re-appear on another, we have to manually calculate
    /// relative mouse motions using the last known mouse location.
    sdl_mouse_pos: (i32, i32),
    /// The current set of active modifiers
    sdl_mods: Mods,
    /// libxkbcommon context
    sdl_xkb_ctx: xkb::Context,
    sdl_xkb_keymap: xkb::Keymap,
    /// this is referenced by Seat, which needs to map and
    /// share it with the clients
    sdl_xkb_keymap_name: String,
    /// xkb state machine
    sdl_xkb_state: xkb::State,
    /// fds the user wants us to wake up on
    sdl_user_fds: Option<FdWatch>,
    /// This maps a SDL window_id to the OutputIds of our Output
    /// and VirtualOutput that events should be delivered one.
    /// The format is `(SDL window_id, Output, VirtualOutput)`.
    sdl_window_id_map: Arc<RwLock<Vec<(u32, OutputId, OutputId)>>>,
}

impl SDL2Plat {
    pub fn new() -> Result<Self> {
        // SDL goodies
        let sdl_context = sdl2::init().unwrap();
        let event_pump = sdl_context.event_pump().unwrap();
        // Create all the components for xkb
        // A description of this can be found in the xkb
        // section of wayland-book.com
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            &"",
            &"",
            &"",
            &"", // These should be env vars
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .expect("Could not initialize a xkb keymap");
        let km_name = keymap.get_as_string(xkb::KEYMAP_FORMAT_TEXT_V1);

        let state = xkb::State::new(&keymap);
        Ok(Self {
            sdl: sdl_context,
            sdl_event_pump: event_pump,
            sdl_mouse_pos: (0, 0),
            sdl_mods: Mods::NONE,
            sdl_xkb_ctx: context,
            sdl_xkb_keymap: keymap,
            sdl_xkb_keymap_name: km_name,
            sdl_xkb_state: state,
            sdl_user_fds: None,
            sdl_window_id_map: Arc::new(RwLock::new(Vec::with_capacity(1))),
        })
    }

    /// SDL hands us events that are identified by a window_id to tell us
    /// which SDL toplevel surface the event was delivered on. We need to
    /// turn this into our OutputId for the Output or VirtualOutput that
    /// we should queue this event up on.
    fn get_output_from_sdl_id(&self, window_id: u32) -> (u32, OutputId, OutputId) {
        self.sdl_window_id_map
            .read()
            .unwrap()
            .iter()
            .find(|e| e.0 == window_id)
            .expect(&format!(
                "Could not find output corresponding to SDL window id {}",
                window_id
            ))
            .clone()
    }

    /// Returns Result<bool, DakotaError>, true if we should terminate
    fn handle_event(
        &mut self,
        global_evsys: &mut GlobalEventSystem,
        output_queues: &mut ll::Component<OutputEventSystem>,
        platform_queues: &mut ll::Component<PlatformEventSystem>,
        raw_event: Option<Event>,
    ) -> Result<()> {
        // raw_event will be Some if we have a valid SDL event
        if let Some(event) = raw_event {
            // First get the event queues for the window reported by this event
            let (mut output_evsys, mut platform_evsys) = match event {
                Event::KeyDown { window_id, .. }
                | Event::KeyUp { window_id, .. }
                | Event::MouseButtonDown { window_id, .. }
                | Event::MouseButtonUp { window_id, .. }
                | Event::MouseWheel { window_id, .. }
                | Event::MouseMotion { window_id, .. }
                | Event::Window { window_id, .. } => {
                    // A window ID of zero is invalid in SDL, we should log this event
                    // and skip it
                    // https://wiki.libsdl.org/SDL3/SDL_GetWindowID
                    if window_id == 0 {
                        log::error!("SDL Event with invalid window_id {:?}", event);
                        return Ok(());
                    }

                    let (_, output_id, virtual_id) = self.get_output_from_sdl_id(window_id);

                    (
                        Some(output_queues.get_mut(&output_id).unwrap()),
                        Some(platform_queues.get_mut(&virtual_id).unwrap()),
                    )
                }
                _ => (None, None),
            };

            // Now we can handle the event and record its events in its queue
            match event {
                // Tell the window to exit if the user closed it
                Event::Quit { .. } => global_evsys.add_event_quit(),
                // Here we record events for our keystrokes
                //
                // This requires converting the raw keycodes from sdl2 into an
                // enum that we control. See input.rs for how this is done. We
                // also wrap the Keyboard Modifiercodes in a similar way
                Event::KeyDown {
                    keycode,
                    keymod,
                    scancode,
                    ..
                } => {
                    let key = convert_sdl_keycode_to_dakota(keycode.unwrap());
                    let mods = convert_sdl_mods_to_dakota(keymod);
                    self.update_xkb_from_scancode(scancode.unwrap(), xkb::KeyDirection::Down);
                    let (raw, utf) = self.get_utf8_from_key(scancode.unwrap());

                    platform_evsys.as_mut().unwrap().add_event_key_down(
                        key,
                        utf,
                        RawKeycode::Linux(raw),
                    );

                    if mods != self.sdl_mods {
                        self.sdl_mods = mods;
                        platform_evsys
                            .as_mut()
                            .unwrap()
                            .add_event_keyboard_modifiers(mods);
                    }
                }
                Event::KeyUp {
                    keycode,
                    keymod,
                    scancode,
                    ..
                } => {
                    let key = convert_sdl_keycode_to_dakota(keycode.unwrap());
                    let mods = convert_sdl_mods_to_dakota(keymod);
                    self.update_xkb_from_scancode(scancode.unwrap(), xkb::KeyDirection::Up);
                    let (raw, _) = self.get_utf8_from_key(scancode.unwrap());

                    platform_evsys.as_mut().unwrap().add_event_key_up(
                        key,
                        String::with_capacity(0), // no utf8 characters are generated for lifting a key
                        RawKeycode::Linux(raw),
                    );

                    if mods != self.sdl_mods {
                        self.sdl_mods = mods;
                        platform_evsys
                            .as_mut()
                            .unwrap()
                            .add_event_keyboard_modifiers(mods);
                    }
                }
                // handle pointer inputs. This just looks like the above keyboard
                Event::MouseButtonDown { mouse_btn, .. } => {
                    let button = convert_sdl_mouse_to_dakota(mouse_btn);
                    platform_evsys
                        .as_mut()
                        .unwrap()
                        .add_event_mouse_button_down(button);
                }
                Event::MouseButtonUp { mouse_btn, .. } => {
                    let button = convert_sdl_mouse_to_dakota(mouse_btn);
                    platform_evsys
                        .as_mut()
                        .unwrap()
                        .add_event_mouse_button_up(button);
                }
                Event::MouseWheel { x, y, .. } => {
                    platform_evsys.as_mut().unwrap().add_event_scroll(
                        // reverse the scroll direction
                        Some((x as f64 * SCROLL_SENSITIVITY * -1.0) as i32),
                        Some((y as f64 * SCROLL_SENSITIVITY * -1.0) as i32),
                        (0.0, 0.0), // v120 value unspecified
                        AxisSource::Wheel,
                    )
                }
                Event::MouseMotion { x, y, .. } => {
                    platform_evsys
                        .as_mut()
                        .unwrap()
                        .add_event_mouse_move(x - self.sdl_mouse_pos.0, y - self.sdl_mouse_pos.1);

                    // Update our mouse position
                    self.sdl_mouse_pos.0 = x;
                    self.sdl_mouse_pos.1 = y;
                }

                // Now we have window events. There's really only one we need to
                // pay attention to here, and it's the resize event. Thundr is
                // going to check for OUT_OF_DATE, but it's possible that the toolkit
                // (SDL) might need refreshing while libvulkan doesn't yet know about
                // it.
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Close => output_evsys.as_mut().unwrap().add_event_destroyed(),
                    WindowEvent::Resized { .. } | WindowEvent::SizeChanged { .. } => {
                        output_evsys.as_mut().unwrap().add_event_resized();
                    }
                    WindowEvent::Exposed { .. } => {
                        output_evsys.as_mut().unwrap().add_event_redraw();
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Ok(())
    }

    /// Update this platform's internal xkbcommon state representing that
    /// a keystroke has taken place.
    fn update_xkb_from_scancode(
        &mut self,
        code: sdl2::keyboard::Scancode,
        direction: xkb::KeyDirection,
    ) {
        let key = convert_sdl_scancode_to_linux(code);

        // let xkb keep track of the keyboard state
        self.sdl_xkb_state.update_key(
            // add 8 to account for differences between evdev and x11
            key as u32 + 8,
            direction,
        );
    }

    /// Convert a sdl2 keycode into a utf8 character
    ///
    /// This handles upper and lowercase which SDL doesn't do for us by using the
    /// provided modifiers. It also handles generating utf8 characters using
    /// xkbcommon.
    ///
    /// Returns an empty array of chars if no utf8 characters were generated by this
    /// keystroke. Also returns the raw Linux keycode, this is used by system users
    /// such as Category5.
    fn get_utf8_from_key(&mut self, code: sdl2::keyboard::Scancode) -> (u32, String) {
        let raw = convert_sdl_scancode_to_linux(code);

        // add 8 to account for differences between evdev and x11
        return (raw, self.sdl_xkb_state.key_get_utf8(raw + 8));
    }
}

impl Platform for SDL2Plat {
    /// Create a window
    ///
    /// This creates a new window output with our winsys, we can
    /// then use this with a Thundr `Display`.
    fn create_output(
        &mut self,
        id: OutputId,
        virtual_output_id: OutputId,
    ) -> Result<Box<dyn OutputPlatform>> {
        let video_subsystem = self.sdl.video().unwrap();
        let window = video_subsystem
            .window("dakota", 640, 480)
            .vulkan()
            .resizable()
            .position_centered()
            .build()?;

        // Record this in our map
        log::debug!("adding SDL window id {}", window.id());
        self.sdl_window_id_map
            .write()
            .unwrap()
            .push((window.id(), id, virtual_output_id));

        Ok(Box::new(SDL2Window {
            sdl_video_sys: video_subsystem,
            sdl_window: window,
            sdl_window_id_map: self.sdl_window_id_map.clone(),
        }))
    }

    /// Create a new virtual window
    ///
    /// This may fail if the platform only supports one virtual surface
    fn create_virtual_output(&mut self) -> bool {
        true
    }

    /// Add a watch descriptor to our list. This will cause the platform's
    /// event loop to wake when this fd is readable and queue the UserFd
    /// event.
    fn add_watch_fd(&mut self, fd: RawFd) {
        if self.sdl_user_fds.is_none() {
            self.sdl_user_fds = Some(FdWatch::new());
        }

        let watch = self.sdl_user_fds.as_mut().unwrap();
        watch.add_fd(fd);
        watch.register_events();
    }

    /// Run the event loop for this platform
    ///
    /// Block and handle all available events from SDL2. If timeout
    /// is specified it will be passed to SDL's wait_event_timeout function.
    fn run(
        &mut self,
        global_evsys: &mut GlobalEventSystem,
        output_evsys: &mut ll::Component<OutputEventSystem>,
        platform_evsys: &mut ll::Component<PlatformEventSystem>,
        timeout: Option<usize>,
    ) -> Result<()> {
        // There are two modes we need to consider for polling for SDL events, since
        // it doesn't follow a unix style: 1) if we are waiting for just SDL, 2) if we
        // are waiting for SDL and some file descriptors
        //
        // In the first case we should use SDL's SDL_WaitEvent, since it will save
        // power by not busy looping. If we need to wait for some fds then we have no
        // choice but to busy loop ourselves since SDL doesn't have a good way for us
        // to deal with this. If this becomes a problem hopefuly SDL3 has a good way to
        // deal with it..
        if let Some(fds) = self.sdl_user_fds.as_mut() {
            loop {
                // Wait for the first readable fd
                if fds.wait_for_events(Some(1)) {
                    global_evsys.add_event_user_fd();
                    break;
                }

                // Or wait for the first SDL event
                let ev = self.sdl_event_pump.poll_event();
                if let Some(ev) = ev {
                    self.handle_event(global_evsys, output_evsys, platform_evsys, Some(ev))?;
                    break;
                }

                // Don't waste all the CPU
                std::thread::sleep(std::time::Duration::from_millis(8));
            }
        } else {
            // First block for the next event
            let ev = match timeout {
                // If we are waiting a certain amount of time, tell SDL. If
                // it returns an event, great, handle it.
                // If not, then just return without handling.
                Some(timeout) => match self.sdl_event_pump.wait_event_timeout(timeout as u32) {
                    Some(event) => event,
                    None => return Ok(()),
                },
                // No timeout was given, so we wait indefinitely
                None => self.sdl_event_pump.wait_event(),
            };
            self.handle_event(global_evsys, output_evsys, platform_evsys, Some(ev))?;
        }

        // Now drain the available events before returning
        // control to the main dakota dispatch loop.
        let mut events: Vec<_> = self.sdl_event_pump.poll_iter().collect();
        for event in events.drain(..) {
            self.handle_event(global_evsys, output_evsys, platform_evsys, Some(event))?;
        }

        return Ok(());
    }

    fn get_th_surf_type<'a>(&self) -> Result<th::SurfaceType> {
        Ok(th::SurfaceType::SDL2)
    }
}

/// Single SDL2 window
pub struct SDL2Window {
    sdl_video_sys: sdl2::VideoSubsystem,
    sdl_window: sdl2::video::Window,
    /// This maps a SDL window_id to the OutputIds of our Output
    /// and VirtualOutput that events should be delivered one.
    /// The format is `(SDL window_id, Output, VirtualOutput)`.
    sdl_window_id_map: Arc<RwLock<Vec<(u32, OutputId, OutputId)>>>,
}

impl Drop for SDL2Window {
    fn drop(&mut self) {
        // Remove this window from our tracking list
        let window_id = self.sdl_window.id();
        log::debug!("removing SDL window id {}", window_id);
        self.sdl_window_id_map
            .write()
            .unwrap()
            .retain(|e| e.0 != window_id);
    }
}

impl OutputPlatform for SDL2Window {
    /// Get the thundr winsys info that this platform should use.
    ///
    /// This is where we share our window system object pointers that
    /// Thundr will consume when it creates a `Dispaly` that draws to
    /// this output.
    fn get_th_window_info<'a>(&self) -> Result<th::WindowInfo> {
        Ok(th::WindowInfo::SDL2(&self.sdl_video_sys, &self.sdl_window))
    }

    /// Set the dimensions of this window
    fn set_geometry(&mut self, win: &dom::Window, dims: (u32, u32)) -> Result<()> {
        self.sdl_window.set_title(&win.title)?;
        self.sdl_window.set_size(dims.0, dims.1)?;
        Ok(())
    }
}
