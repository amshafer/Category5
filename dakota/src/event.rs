//! Event Handling
//!
// Austin Shafer - 2022

use crate::dom;
use crate::dom::DakotaDOM;
use crate::input::{Keycode, Mods, MouseButton};
use lluvia as ll;
use std::collections::VecDeque;
use std::rc::Rc;

pub struct EventSystem {
    /// The global event queue
    /// This will be iterable after dispatching, and
    /// must be cleared (all events handled) before the
    /// next dispatch
    es_global_event_queue: VecDeque<Event>,
    /// These are dakota-only events, which the user doesn't
    /// have to worry about and Dakota itself will handle
    pub(crate) es_dakota_event_queue: Vec<Event>,
    /// The compiled set of event handlers.
    es_handler_ecs_inst: ll::Instance,
    /// Ties a string name to a handler id
    /// (name, Id)
    es_name_to_handler_map: Vec<(String, ll::Entity)>,
    /// Our current mouse position
    ///
    /// The individual backends report relative mouse position changes,
    /// but in the case of button presses we need to report the absolute
    /// location. This adds a place to cache this. The platforms will
    /// report relative mouse changes and we will update this here.
    es_mouse_pos: (f64, f64),
}

impl EventSystem {
    pub fn new() -> Self {
        let handler_ecs = ll::Instance::new();

        Self {
            es_global_event_queue: VecDeque::new(),
            es_dakota_event_queue: Vec::new(),
            es_handler_ecs_inst: handler_ecs,
            es_name_to_handler_map: Vec::new(),
            es_mouse_pos: (0.0, 0.0),
        }
    }
}

pub type HandlerArgs = Rc<Vec<String>>;

/// Source axis for scrolling operations
///
/// This distinguishes if a source comes from a mouse wheel or a trackpad. If
/// the platform does not distinguish this will always be `Wheel`.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AxisSource {
    Wheel = 0,
    Finger = 1,
}

/// Dakota Events
///
/// These events come from a couple possible sources, the most important of
/// which is user input. All mouse movements and button presses are recorded
/// as Events in the Dakota event queue, along with window system events such
/// as resizing the window.
#[derive(Debug, Clone)]
pub enum Event {
    /// The window size has been changed, normally by the user.
    WindowResized {
        args: HandlerArgs,
        size: dom::Size<u32>,
    },
    /// The Dakota Application window has been closed
    WindowClosed { args: HandlerArgs },
    /// This event is triggered every time Dakota draws a frame
    WindowRedrawComplete { args: HandlerArgs },
    /// Key has been pressed. Includes the updated modifiers.
    InputKeyDown { key: Keycode, modifiers: Mods },
    /// Key has been released. Includes the updated modifiers.
    InputKeyUp { key: Keycode, modifiers: Mods },
    /// Movement of the mouse relative to the previous position
    ///
    /// This is the amount the mouse moved.
    InputMouseMove { dx: f64, dy: f64 },
    /// A mouse button has been pressed. The button is specified
    /// in the case that there are multiple buttons on the mouse.
    InputMouseButtonDown { button: MouseButton, x: f64, y: f64 },
    /// A mouse button has been released
    InputMouseButtonUp { button: MouseButton, x: f64, y: f64 },
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
        position: (f64, f64),
        /// horizontal relative motion
        xrel: Option<f64>,
        /// vertical relative motion
        yrel: Option<f64>,
        /// The v120 libinput API value, if it was available
        /// This should only be set on AXIS_SOURCE_WHEEL input devices
        /// (horizontal, vertical)
        v120_val: (f64, f64),
        /// The axis source.
        source: AxisSource,
    },
    /// Indicates that one of the fds that the application provided
    /// for the event loop is readable. This can be used to have
    /// dakota `select()` a set of fds and wake the application up
    /// when they are ready.
    UserFdReadable,
}

impl EventSystem {
    /// Look up the ECS Id of an Event Handler given its string name
    ///
    /// This is used to go from the human name the app gave the handler to
    /// the O(1) integer id of the handler that dakota will use.
    pub fn get_handler_id_from_name(&mut self, name: String) -> ll::Entity {
        // first, get the ECS id for this name
        // check if this event handler has already been defined
        match self.es_name_to_handler_map.iter().find(|(n, _)| *n == name) {
            Some((_, ecs_id)) => ecs_id.clone(),
            // otherwise make a new id for it, it's a new name
            None => {
                let ecs_id = self.es_handler_ecs_inst.add_entity();
                self.es_name_to_handler_map
                    .push((name.clone(), ecs_id.clone()));
                ecs_id
            }
        }
    }

    /// Add a window resize event to the global queue
    ///
    /// This signifies that a window was resized, and is triggered
    /// anytime OOD is returned from thundr.
    pub fn add_event_window_resized(&mut self, dom: &DakotaDOM, new_size: dom::Size<u32>) {
        if let Some(handler) = dom.window.events.resize.as_ref() {
            self.es_global_event_queue.push_back(Event::WindowResized {
                args: handler.args.clone(),
                size: new_size,
            });
        }
    }

    /// Add a redraw request completion to the global queue
    ///
    /// Since while dispatching it isn't guaranteed that a redraw
    /// will take place, this lets a client know that the previous frame
    /// was drawn, and it should handle any once-per-frame actions it
    /// needs to take.
    ///
    /// This isn't a performance limiting event, the app doesn't need to
    /// use this to control drawing. This should be used to queue up the
    /// next elements to be presented, or run subroutines. Dakota will
    /// internally worry about drawing everything.
    pub fn add_event_window_redraw_complete(&mut self, dom: &DakotaDOM) {
        if let Some(handler) = dom.window.events.redraw_complete.as_ref() {
            self.es_global_event_queue
                .push_back(Event::WindowRedrawComplete {
                    args: handler.args.clone(),
                });
        }
    }

    /// Notify the app that a window was closed
    ///
    /// This is not an optional event. It will always be sent. It is
    /// optional in the element tree however.
    pub fn add_event_window_closed(&mut self, dom: &DakotaDOM) {
        if let Some(handler) = dom.window.events.closed.as_ref() {
            self.es_global_event_queue.push_back(Event::WindowClosed {
                args: handler.args.clone(),
            });
            return;
        }

        // If we couldn't get the arg array from the tree, then
        // just create an empty one
        self.es_global_event_queue.push_back(Event::WindowClosed {
            args: Rc::new(Vec::with_capacity(0)),
        });
    }

    pub fn add_event_key_down(&mut self, key: Keycode, mods: Mods) {
        self.es_global_event_queue.push_back(Event::InputKeyDown {
            key: key,
            modifiers: mods,
        });
    }
    pub fn add_event_key_up(&mut self, key: Keycode, mods: Mods) {
        self.es_global_event_queue.push_back(Event::InputKeyUp {
            key: key,
            modifiers: mods,
        });
    }

    pub fn add_event_mouse_move(&mut self, dx: f64, dy: f64) {
        self.es_global_event_queue
            .push_back(Event::InputMouseMove { dx: dx, dy: dy });
    }
    pub fn add_event_mouse_button_down(&mut self, button: MouseButton) {
        self.es_global_event_queue
            .push_back(Event::InputMouseButtonDown {
                button: button,
                x: self.es_mouse_pos.0,
                y: self.es_mouse_pos.1,
            });
    }
    pub fn add_event_mouse_button_up(&mut self, button: MouseButton) {
        self.es_global_event_queue
            .push_back(Event::InputMouseButtonUp {
                button: button,
                x: self.es_mouse_pos.0,
                y: self.es_mouse_pos.1,
            });
    }

    pub fn add_event_scroll(
        &mut self,
        x: Option<f64>,
        y: Option<f64>,
        v120: (f64, f64),
        source: AxisSource,
    ) {
        // Update our cached mouse position
        if let Some(x) = x {
            self.es_mouse_pos.0 += x;
        }
        if let Some(y) = x {
            self.es_mouse_pos.1 += y;
        }

        let ev = Event::InputScroll {
            position: self.es_mouse_pos,
            xrel: x,
            yrel: y,
            v120_val: v120,
            source: source,
        };

        self.es_global_event_queue.push_back(ev.clone());

        // We also want to handle scrolling ourselves, so put this event on the
        // dakota queue as well
        self.es_dakota_event_queue.push(ev);
    }

    pub fn add_event_user_fd(&mut self) {
        self.es_global_event_queue.push_back(Event::UserFdReadable);
    }

    /// Drain the queue of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn drain_events<'a>(&'a mut self) -> std::collections::vec_deque::Drain<'a, Event> {
        self.es_global_event_queue.drain(0..)
    }
}
