// Helpers to handle budgeting subsystems based on time
//
// Austin Shafer - 2020
use std::time::{Duration,SystemTime,UNIX_EPOCH};

// Helper to get the current time in milliseconds
pub fn get_current_millis() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Error getting system time")
        .as_millis() as u32
}

// Manages subsystem timings
//
// The motivation for this is frame callbacks, which
// need to take place once every 16 ms (once a frame
// at 60 fps). This struct keeps track of how much
// time is remaining before an action needs to be called,
// and callers can use this number for their timeout
// values.
//
// This isn't a timing subsystem, but rather a helper
// for tracking timing information.
pub struct TimingManager {
    // length of time we are counting down from
    tm_period: u32,
    // the last time we reset this manager
    tm_start: Duration,
}

impl TimingManager {
    // create a new manager to track time
    // periods of length `period`
    pub fn new(period: u32) -> TimingManager {
        TimingManager {
            tm_period: u32,
            tm_start: get_current_millis(),
        }
    }

    // Reset the manager to the current time
    pub fn reset(&mut self) {
        self.tm_start = get_current_millis();
    }

    // Returns true if period ms have passed
    // since this manager was reset
    pub fn is_overdue(&mut self) -> bool {
        let time = get_current_millis();

        // If it has been period ms
        if time - self.tm_start >= self.tm_period {
            return true;
        }
        return false;
    }

    // Returns the number of ms remaining in this
    // tracker
    //
    // If 0 is returned, it is overdue and we
    // should reset it.
    pub fn time_remaining(&mut self) -> u32 {
        let time_elapsed = get_current_millis() - self.tm_start;
        return self.tm_period - time_elapsed;
    }
}
