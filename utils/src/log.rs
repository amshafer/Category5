// This makes it easy to import the logging stuff
// Austin Shafer - 2020

#![allow(unused_imports)]
pub use crate::debug;
pub use crate::error;
pub use crate::info;
pub use crate::log_internal;
pub use crate::profiling;
pub use crate::{logging::LogLevel, timing::get_current_millis};
