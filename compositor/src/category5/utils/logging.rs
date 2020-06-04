// Category-based logging infrastructure
//
// This will be used from multiple threads, so it needs
// to be stateless
//
// Austin Shafer - 2020
#[allow(dead_code)]
pub enum LogLevel {
    // in order of highest priority
    critical, // Urgent and must always be displayed
    error,
    info, // generic info, not verbose
    debug, // more verbose
    profiling, // profiling related timing
}

impl LogLevel {
    pub fn get_name(&mut self) -> &'static str {
        match self {
            LogLevel::critical => "critical",
            LogLevel::error => "error",
            LogLevel::info => "info",
            LogLevel::debug => "debug",
            LogLevel::profiling => "profiling",
        }
    }
}

#[macro_export]
macro_rules! log {
    ($loglevel:expr, $($format_args:tt)+) => ({
        println!("[{:?}]<{}> {}:{} - {}",
                 get_current_millis(),
                 $loglevel.get_name(),
                 file!(),
                 line!(),
                 format!($($format_args)+)
        );
    })
}
