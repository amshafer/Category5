//! Event Handling
//!
// Austin Shafer - 2022

use crate::input::{Keycode, Mods, MouseButton};
use std::collections::VecDeque;

/// Global Dakota Event Queue
pub struct GlobalEventSystem {
    /// The global event queue
    es_event_queue: VecDeque<GlobalEvent>,
}

impl GlobalEventSystem {
    pub fn new() -> Self {
        Self {
            es_event_queue: VecDeque::new(),
        }
    }
}

/// Dakota Global Events
///
/// These events are independent of any particular output, and may notify
/// the user of polled file descriptors or timers.
#[derive(Debug, Clone)]
pub enum GlobalEvent {
    /// Indicates that one of the fds that the application provided
    /// for the event loop is readable. This can be used to have
    /// dakota `select()` a set of fds and wake the application up
    /// when they are ready.
    UserFdReadable,
    /// Dakota is quitting, the app should terminate
    Quit,
}

impl GlobalEventSystem {
    pub fn add_event_user_fd(&mut self) {
        self.es_event_queue.push_back(GlobalEvent::UserFdReadable);
    }

    /// Notify the app that a window was closed
    ///
    /// This is not an optional event. It will always be sent. It is
    /// optional in the element tree however.
    pub fn add_event_quit(&mut self) {
        self.es_event_queue.push_back(GlobalEvent::Quit);
    }

    /// Drain the queue of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn drain_events<'a>(&'a mut self) -> std::collections::vec_deque::Drain<'a, GlobalEvent> {
        self.es_event_queue.drain(0..)
    }
}

/// Output Event Queue
pub struct OutputEventSystem {
    /// The event queue itself
    es_event_queue: VecDeque<OutputEvent>,
}

/// Dakota Output Events
///
/// These events come from a couple possible sources, the most important of
/// which is the Redraw event. These are specific to a Dakota Output.
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// The window size has been changed, normally by the user.
    Resized,
    /// The output window has been closed
    Destroyed,
    /// The platform has lost the current output and we need to re-present
    /// to update the display.
    ///
    /// This happens on window systems, when the window needs redrawn.
    Redraw,
}

impl OutputEventSystem {
    pub fn new() -> Self {
        Self {
            es_event_queue: VecDeque::new(),
        }
    }
}

impl OutputEventSystem {
    /// Add a window resize event
    ///
    /// This signifies that a window was resized, and is triggered
    /// anytime OOD is returned from thundr.
    pub fn add_event_resized(&mut self) {
        self.es_event_queue.push_back(OutputEvent::Resized);
    }

    /// Add notice that the window needs redrawing.
    ///
    /// The platform has lost the current output and we need to re-present
    /// to update the display.
    ///
    /// This happens on window systems, when the window needs redrawn.
    pub fn add_event_redraw(&mut self) {
        self.es_event_queue.push_back(OutputEvent::Redraw);
    }

    /// Notify the app that a window was closed
    ///
    /// This is not an optional event. It will always be sent. It is
    /// optional in the element tree however.
    pub fn add_event_destroyed(&mut self) {
        self.es_event_queue.push_back(OutputEvent::Destroyed);
    }

    /// Get the next event
    ///
    /// The app should do this in its main loop after dispatching.
    pub fn pop_event(&mut self) -> Option<OutputEvent> {
        self.es_event_queue.pop_front()
    }
}

/// Platform Event Queue
pub struct PlatformEventSystem {
    /// The event queue itself
    es_event_queue: VecDeque<PlatformEvent>,
    /// Our current mouse position
    ///
    /// The individual backends report relative mouse position changes,
    /// but in the case of button presses we need to report the absolute
    /// location. This adds a place to cache this. The platforms will
    /// report relative mouse changes and we will update this here.
    es_mouse_pos: (i32, i32),
}

impl PlatformEventSystem {
    pub fn new() -> Self {
        Self {
            es_event_queue: VecDeque::new(),
            es_mouse_pos: (0, 0),
        }
    }
}

/// Source axis for scrolling operations
///
/// This distinguishes if a source comes from a mouse wheel or a trackpad. If
/// the platform does not distinguish this will always be `Wheel`.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum AxisSource {
    Wheel = 0,
    Finger = 1,
}

/// This represents the raw integer keycode that the system gave us.
///
/// These are identified by encoding, in case the application wants to
/// handle the raw keycodes itself and not risk Dakota not recognizing
/// it.
#[derive(Debug, Clone, Copy)]
pub enum RawKeycode {
    /// The Linux kernel's input encoding.
    ///
    /// Found in uapi/linux/input-event-codes.h
    Linux(u32),
}

/// Dakota Platform Events
///
/// These events are delivered on a virtual output and represent window
/// system events that are relevant to the surface the scene is being
/// applied to. The main event type delieverd here is user input.
#[derive(Debug, Clone)]
pub enum PlatformEvent {
    /// Key has been pressed. Includes the updated modifiers.
    InputKeyDown {
        key: Keycode,
        /// Any utf8 characters that were generated by this keystroke
        utf8: String,
        raw_keycode: RawKeycode,
    },
    /// Key has been released.
    InputKeyUp {
        key: Keycode,
        /// Any utf8 characters that were generated by this keystroke
        utf8: String,
        raw_keycode: RawKeycode,
    },
    /// The set of active Keyboard Modifier keys has changed. The modifier
    /// keypresses are also delivered in the `InputKey*` events, but the current
    /// set of modifiers is included separately here for convenience.
    InputKeyboardModifiers { mods: Mods },
    /// Movement of the mouse relative to the previous position
    ///
    /// This is the amount the mouse moved.
    InputMouseMove { dx: i32, dy: i32 },
    /// A mouse button has been pressed. The button is specified
    /// in the case that there are multiple buttons on the mouse.
    InputMouseButtonDown { button: MouseButton, x: i32, y: i32 },
    /// A mouse button has been released
    InputMouseButtonUp { button: MouseButton, x: i32, y: i32 },
    /// User has taken a scrolling action.
    ///
    /// This is complex since there are a variety of scrolling options
    /// that can be reported by hardware. `horizontal` and vertical` are both
    /// optional values that can be reported. This is to distunguish between
    /// no scrolling taken place and a value of zero, which is reported when
    /// kinetic scrolling stops.
    ///
    /// v120 values similar to windows may also be reported. This allows for
    /// high resolution scroll wheel feedback.
    InputScroll {
        /// The current mouse position
        position: (i32, i32),
        /// horizontal relative motion
        xrel: Option<i32>,
        /// vertical relative motion
        yrel: Option<i32>,
        /// The v120 libinput API value, if it was available
        /// This should only be set on AXIS_SOURCE_WHEEL input devices
        /// (horizontal, vertical)
        v120_val: (f64, f64),
        /// The axis source.
        source: AxisSource,
    },
}

impl PlatformEventSystem {
    pub fn add_event_key_down(&mut self, key: Keycode, utf8: String, raw_key: RawKeycode) {
        self.es_event_queue.push_back(PlatformEvent::InputKeyDown {
            key: key,
            utf8: utf8,
            raw_keycode: raw_key,
        });
    }
    pub fn add_event_key_up(&mut self, key: Keycode, utf8: String, raw_key: RawKeycode) {
        self.es_event_queue.push_back(PlatformEvent::InputKeyUp {
            key: key,
            utf8: utf8,
            raw_keycode: raw_key,
        });
    }

    pub fn add_event_keyboard_modifiers(&mut self, mods: Mods) {
        self.es_event_queue
            .push_back(PlatformEvent::InputKeyboardModifiers { mods: mods });
    }

    pub fn add_event_mouse_move(&mut self, dx: i32, dy: i32) {
        // Update our cached mouse position
        self.es_mouse_pos.0 += dx;
        self.es_mouse_pos.1 += dy;

        self.es_event_queue
            .push_back(PlatformEvent::InputMouseMove { dx: dx, dy: dy });
    }
    pub fn add_event_mouse_button_down(&mut self, button: MouseButton) {
        self.es_event_queue
            .push_back(PlatformEvent::InputMouseButtonDown {
                button: button,
                x: self.es_mouse_pos.0,
                y: self.es_mouse_pos.1,
            });
    }
    pub fn add_event_mouse_button_up(&mut self, button: MouseButton) {
        self.es_event_queue
            .push_back(PlatformEvent::InputMouseButtonUp {
                button: button,
                x: self.es_mouse_pos.0,
                y: self.es_mouse_pos.1,
            });
    }

    pub fn add_event_scroll(
        &mut self,
        x: Option<i32>,
        y: Option<i32>,
        v120: (f64, f64),
        source: AxisSource,
    ) {
        self.es_event_queue.push_back(PlatformEvent::InputScroll {
            position: self.es_mouse_pos,
            xrel: x,
            yrel: y,
            v120_val: v120,
            source: source,
        });
    }

    /// Get the next event
    ///
    /// The app should do this in its main loop after dispatching.
    pub fn pop_event(&mut self) -> Option<PlatformEvent> {
        self.es_event_queue.pop_front()
    }
}
