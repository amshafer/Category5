//! Event Handling
//!
// Austin Shafer - 2022

use crate::dom;
use crate::dom::DakotaDOM;
use crate::Dakota;
use utils::ecs::ECSId;

#[derive(Debug)]
pub enum Event {
    WindowResized(dom::Size),
}

impl Dakota {
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

    pub fn add_event_window_resized(&mut self, dom: &DakotaDOM, new_size: dom::Size) {
        match dom.window.events.as_ref() {
            Some(events) => {
                if events.window_resize.is_some() {
                    self.d_global_event_queue
                        .push(Event::WindowResized(new_size));
                }
            }
            None => {}
        }
    }

    pub fn get_events<'a>(&'a self) -> &'a [Event] {
        self.d_global_event_queue.as_slice()
    }
}
