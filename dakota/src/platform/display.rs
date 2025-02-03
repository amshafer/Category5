/// The direct to display backend
///
/// This will run Dakota on the main display without any window system
/// present. This is done with the `VK_KHR_Display` Vulkan surface type
/// and using libinput to get input events.
extern crate input;
use input::event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait};
use input::event::pointer;
use input::event::pointer::{ButtonState, PointerEvent, PointerScrollEvent};
use input::{Libinput, LibinputInterface};

extern crate xkbcommon;
use xkbcommon::xkb;

use super::{BackendType, OutputPlatform, Platform};
use crate::event::*;
use crate::input::{convert_libinput_mouse_to_dakota, convert_xkb_keycode_to_dakota, Mods};
use crate::OutputId;
use crate::*;
use utils::log;

use std::fs::{File, OpenOptions};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::OwnedFd;
use std::path::Path;

/// This is sort of like a private userdata struct which
/// is used as an interface to the systems devices
///
/// i.e. this could call consolekit to avoid having to
/// be a root user to get raw input.
struct Inkit {
    // For now we don't have anything special to do,
    // so we are just putting a phantom int here since
    // we need to have something.
    _inner: u32,
}

/// This is the interface that libinput uses to abstract away
/// consolekit and friends.
///
/// In our case we just pass the arguments through to `open`.
/// We need to use the unix open extensions so that we can pass
/// custom flags.
impl LibinputInterface for Inkit {
    // open a device
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        log::debug!(" Opening device {:?}", path);
        match OpenOptions::new()
            // the unix extension's custom_flag field below
            // masks out O_ACCMODE, i.e. read/write, so add
            // them back in
            .read(true)
            .write(true)
            // libinput wants to use O_NONBLOCK
            .custom_flags(flags)
            .open(path)
        {
            Ok(f) => {
                // this turns the File into an int, so we
                // don't need to worry about the File's
                // lifetime.
                let fd = f.into();
                log::error!("Returning raw fd {:?}", fd);
                Ok(fd)
            }
            Err(e) => {
                // leave this in, it gives great error msgs
                log::error!("Error on opening {:?}", e);
                Err(-1)
            }
        }
    }

    // close a device
    fn close_restricted(&mut self, fd: OwnedFd) {
        // this will close the file
        drop(File::from(fd));
    }
}

/// Baremetal platform.
///
/// This platform is Direct 2 Display, meaning that there is no
/// window server at all. This will use vulkan to present to a physical
/// display and libinput to collect raw input events.
pub struct LibinputPlat {
    dp_type: BackendType,
    /// libinput context
    dp_libin: Libinput,
    /// libxkbcommon context
    _dp_xkb_ctx: xkb::Context,
    _dp_xkb_keymap: xkb::Keymap,
    /// this is referenced by Seat, which needs to map and
    /// share it with the clients
    _dp_xkb_keymap_name: String,
    /// xkb state machine
    dp_xkb_state: xkb::State,
    /// The current modifier key state. This will be updated using
    /// xkb.
    dp_current_modifiers: Mods,
    /// Our private fd listener
    dp_fdwatch: FdWatch,
    /// This is the Id of the virtual output we are driving
    /// TODO: right now this does not ever free our VirtualOutput
    /// id, so we need to find a way to allow recreation of the
    /// VirtualOutput.
    dp_output_id: Option<OutputId>,
    /// Dummy timeout we use for triggering a draw on startup
    dp_cached_timeout: Option<usize>,
}

impl LibinputPlat {
    pub fn new(backend_type: BackendType) -> Result<Self> {
        let kit: Inkit = Inkit { _inner: 0 };
        let mut libin = Libinput::new_with_udev(kit);

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
        // we need to choose a "seat" for udev to listen on
        // the default seat is seat0, which is all input devs
        libin.udev_assign_seat("seat0").unwrap();

        let mut fdwatch = FdWatch::new();
        fdwatch.add_fd(libin.as_raw_fd());
        fdwatch.register_events();

        Ok(Self {
            dp_type: backend_type,
            dp_libin: libin,
            _dp_xkb_ctx: context,
            _dp_xkb_keymap: keymap,
            _dp_xkb_keymap_name: km_name,
            dp_xkb_state: state,
            dp_current_modifiers: Mods::NONE,
            dp_fdwatch: fdwatch,
            dp_output_id: None,
            dp_cached_timeout: Some(0),
        })
    }

    /// Translate a libinput scroll event into a Dakota axis event
    ///
    /// This is dyn since it can handle both horizontal and vertical
    /// axis events.
    fn get_scroll_event(
        &self,
        evsys: &mut PlatformEventSystem,
        ev: &dyn pointer::PointerScrollEvent,
        source: AxisSource,
        v120: (f64, f64),
    ) {
        let mut horizontal = None;
        let mut vertical = None;

        // reverse the scroll directions
        if ev.has_axis(pointer::Axis::Horizontal) {
            horizontal = Some((ev.scroll_value(pointer::Axis::Horizontal) * -1.0) as i32);
        }
        if ev.has_axis(pointer::Axis::Vertical) {
            vertical = Some((ev.scroll_value(pointer::Axis::Vertical) * -1.0) as i32);
        }

        evsys.add_event_scroll(horizontal, vertical, v120, source);
    }

    /// Get the next available event from libinput
    ///
    /// Dispatch should be called before this so libinput can
    fn process_available(&mut self, platform_queues: &mut ll::Component<PlatformEventSystem>) {
        let mut evsys = platform_queues
            .get_mut(self.dp_output_id.as_ref().unwrap())
            .unwrap();

        while let Some(ev) = self.dp_libin.next() {
            match ev {
                input::event::Event::Pointer(PointerEvent::Motion(m)) => {
                    evsys.add_event_mouse_move(m.dx() as i32, m.dy() as i32);
                }
                // TODO: actually handle advanced scrolling/finger behavior
                // We should track ScrollWheel using the v120 api, and handle
                // high-res and wheel click behavior. For ScrollFinger we
                // should handle kinetic scrolling
                input::event::Event::Pointer(PointerEvent::ScrollFinger(sf)) => {
                    self.get_scroll_event(&mut evsys, &sf, AxisSource::Finger, (0.0, 0.0));
                }
                input::event::Event::Pointer(PointerEvent::ScrollWheel(sw)) => {
                    let mut v120 = (0.0, 0.0);

                    // Mouse wheels will be handled with the higher resolution
                    // v120 API for discrete scrolling
                    if sw.has_axis(pointer::Axis::Horizontal) {
                        v120.0 = sw.scroll_value_v120(pointer::Axis::Horizontal);
                    }
                    if sw.has_axis(pointer::Axis::Vertical) {
                        v120.1 = sw.scroll_value_v120(pointer::Axis::Vertical);
                    }

                    self.get_scroll_event(&mut evsys, &sw, AxisSource::Wheel, v120);
                }
                input::event::Event::Pointer(PointerEvent::Button(b)) => {
                    let button = convert_libinput_mouse_to_dakota(b.button());

                    if b.button_state() == ButtonState::Pressed {
                        evsys.add_event_mouse_button_down(button);
                    } else {
                        evsys.add_event_mouse_button_up(button);
                    }
                }
                input::event::Event::Keyboard(KeyboardEvent::Key(k)) => {
                    // let xkb keep track of the keyboard state
                    let changed = self.dp_xkb_state.update_key(
                        // add 8 to account for differences between evdev and x11
                        k.key() as u32 + 8,
                        match k.key_state() {
                            KeyState::Pressed => xkb::KeyDirection::Down,
                            KeyState::Released => xkb::KeyDirection::Up,
                        },
                    );

                    let keysym = self.dp_xkb_state.key_get_one_sym(k.key() + 8);
                    let key = convert_xkb_keycode_to_dakota(keysym);
                    let utf = self.dp_xkb_state.key_get_utf8(k.key() + 8);

                    // Update each modifier
                    if changed != 0 {
                        let mod_options = [
                            (xkb::MOD_NAME_ALT, Mods::LALT),
                            (xkb::MOD_NAME_NUM, Mods::NUM),
                            (xkb::MOD_NAME_CAPS, Mods::CAPS),
                            (xkb::MOD_NAME_CTRL, Mods::LCTRL),
                            (xkb::MOD_NAME_LOGO, Mods::LMETA),
                            (xkb::MOD_NAME_SHIFT, Mods::LCTRL),
                        ];

                        for opt in mod_options.iter() {
                            self.dp_current_modifiers |= if self
                                .dp_xkb_state
                                .mod_name_is_active(&opt.0, xkb::STATE_MODS_EFFECTIVE)
                            {
                                opt.1
                            } else {
                                Mods::NONE
                            };
                        }

                        // Add the modifier event with the latest mods
                        evsys.add_event_keyboard_modifiers(self.dp_current_modifiers);
                    }

                    if k.key_state() == KeyState::Pressed {
                        evsys.add_event_key_down(key, utf, RawKeycode::Linux(k.key()));
                    } else {
                        // Key up events do not generate utf characters
                        evsys.add_event_key_up(
                            key,
                            String::with_capacity(0),
                            RawKeycode::Linux(k.key()),
                        );
                    }
                }
                _e => log::debug!("Unhandled Input Event: {:?}", _e),
            };
        }
    }
}

impl Platform for LibinputPlat {
    /// Create a window
    ///
    /// This creates a new window output with our winsys, we can
    /// then use this with a Thundr `Display`.
    fn create_output(
        &mut self,
        _id: OutputId,
        _virtual_output_id: OutputId,
    ) -> Result<Box<dyn OutputPlatform>> {
        Ok(Box::new(LibinputOutput {
            lo_type: self.dp_type,
        }))
    }

    /// Create a new virtual window
    ///
    /// This may fail if the platform only supports one virtual surface
    fn create_virtual_output(&mut self, output_ecs: &ll::Instance) -> Result<OutputId> {
        if self.dp_output_id.is_some() {
            return Err(anyhow!(
                "Libinput platform supports only one VirtualOutput at a time"
            ));
        }

        let ret = output_ecs.add_entity();
        self.dp_output_id = Some(ret.clone());
        Ok(ret)
    }

    /// Add a watch descriptor to our list. This will cause the platform's
    /// event loop to wake when this fd is readable and queue the UserFd
    /// event.
    fn add_watch_fd(&mut self, fd: RawFd) {
        self.dp_fdwatch.add_fd(fd);
        self.dp_fdwatch.register_events();
    }

    /// Run the event loop for this platform
    ///
    /// This will dispatch winsys handling and will wait for user
    /// input.
    ///
    /// Returns true if we should redraw the app due to an out of
    /// date swapchain.
    fn run(
        &mut self,
        _global_evsys: &mut GlobalEventSystem,
        _output_queues: &mut ll::Component<OutputEventSystem>,
        platform_queues: &mut ll::Component<PlatformEventSystem>,
        mut timeout: Option<usize>,
    ) -> Result<()> {
        if self.dp_cached_timeout.is_some() && timeout.is_none() {
            timeout = self.dp_cached_timeout.take();
        }

        self.dp_fdwatch.wait_for_events(timeout);
        // TODO: return UserFdReadable?

        self.dp_libin.dispatch().unwrap();
        self.process_available(platform_queues);

        Ok(())
    }

    fn get_th_surf_type<'a>(&self) -> Result<th::SurfaceType> {
        Ok(match self.dp_type {
            #[cfg(feature = "drm")]
            BackendType::Drm => th::SurfaceType::Drm,
            BackendType::VkD2d => th::SurfaceType::Display,
        })
    }
}

/// Libinput output
///
/// This doesn't hold as much state as other backends as there isn't
/// a window system in play here.
pub struct LibinputOutput {
    lo_type: BackendType,
}

impl OutputPlatform for LibinputOutput {
    fn get_th_window_info<'a>(&self) -> Result<th::WindowInfo> {
        Ok(match self.lo_type {
            #[cfg(feature = "drm")]
            BackendType::Drm => th::WindowInfo::Drm,
            BackendType::VkD2d => th::WindowInfo::Display,
        })
    }

    /// This doesn't make sense to implement, since the final size is just whatever
    /// the size of the screen is.
    fn set_geometry(&mut self, _win: &dom::Window, _dims: (u32, u32)) -> Result<()> {
        log::error!("set_output_params on direct backends is unimplemented");
        Ok(())
    }
}
