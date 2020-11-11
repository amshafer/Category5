// This makes it easy to import the logging stuff
// Austin Shafer - 2020

#![allow(unused_imports)]
pub use crate::log;
pub use crate::category5::utils::{
    timing::get_current_millis,
    logging::LogLevel,
};
