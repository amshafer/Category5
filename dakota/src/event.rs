//! Event Handling
//!
// Austin Shafer - 2022

use crate::dom;
use crate::dom::DakotaDOM;
use crate::Dakota;
use std::rc::Rc;
use utils::ecs::ECSId;

pub type HandlerArgs = Rc<Vec<String>>;

#[derive(Debug)]
pub enum Event {
    WindowResized(HandlerArgs, dom::Size),
    WindowClosed(HandlerArgs),
    WindowRedrawComplete(HandlerArgs),
}

impl Dakota {
    /// Look up the ECS Id of an Event Handler given its string name
    ///
    /// This is used to go from the human name the app gave the handler to
    /// the O(1) integer id of the handler that dakota will use.
    pub fn get_handler_id_from_name(&mut self, name: String) -> ECSId {
        // first, get the ECS id for this name
        // check if this event handler has already been defined
        match self.d_name_to_handler_map.iter().find(|(n, _)| *n == name) {
            Some((_, ecs_id)) => ecs_id.clone(),
            // otherwise make a new id for it, it's a new name
            None => {
                let ecs_id = self.d_handler_ecs_inst.mint_new_id();
                self.d_name_to_handler_map
                    .push((name.clone(), ecs_id.clone()));
                ecs_id
            }
        }
    }

    /// Add a window resize event to the global queue
    ///
    /// This signifies that a window was resized, and is triggered
    /// anytime OOD is returned from thundr.
    pub fn add_event_window_resized(&mut self, dom: &DakotaDOM, new_size: dom::Size) {
        if let Some(events) = dom.window.events.as_ref() {
            if let Some(handler) = events.resize.as_ref() {
                self.d_global_event_queue
                    .push(Event::WindowResized(handler.args.clone(), new_size));
            }
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
        if let Some(events) = dom.window.events.as_ref() {
            if let Some(handler) = events.redraw_complete.as_ref() {
                self.d_global_event_queue
                    .push(Event::WindowRedrawComplete(handler.args.clone()));
            }
        }
    }

    /// Notify the app that a window was closed
    ///
    /// This is not an optional event. It will always be sent. It is
    /// optional in the element tree however.
    pub fn add_event_window_closed(&mut self, dom: &DakotaDOM) {
        if let Some(events) = dom.window.events.as_ref() {
            if let Some(handler) = events.closed.as_ref() {
                self.d_global_event_queue
                    .push(Event::WindowClosed(handler.args.clone()));
                return;
            }
        }

        // If we couldn't get the arg array from the tree, then
        // just create an empty one
        self.d_global_event_queue
            .push(Event::WindowClosed(Rc::new(Vec::with_capacity(0))));
    }

    /// Get the slice of currently unhandled events
    ///
    /// The app should do this in its main loop after dispatching.
    /// These will be cleared during each dispatch.
    pub fn get_events<'a>(&'a self) -> &'a [Event] {
        self.d_global_event_queue.as_slice()
    }
}
