// Category-based logging infrastructure
//
// This will be used from multiple threads, so it needs
// to be stateless
//
// Austin Shafer - 2020
#[allow(dead_code,non_camel_case_types)]
pub enum LogLevel {
    // in order of highest priority
    critical, // Urgent and must always be displayed
    error,
    debug, // debugging related, not verbose
    info, // more verbose
    profiling, // profiling related timing
}

impl LogLevel {
    pub fn get_name(&mut self) -> &'static str {
        match self {
            LogLevel::critical => "critical",
            LogLevel::error => "error",
            LogLevel::debug => "debug",
            LogLevel::info => "info",
            LogLevel::profiling => "profiling",
        }
    }

    pub fn get_level(&mut self) -> u32 {
        match self {
            LogLevel::critical => 0,
            LogLevel::error => 1,
            LogLevel::debug => 2,
            LogLevel::info => 3,
            LogLevel::profiling => 4,
        }
    }
}

#[macro_export]
macro_rules! log {
    ($loglevel:expr, $($format_args:tt)+) => ({
        // !! NOTE: current log level set here !!
        //
        // Currently set to the debug level (2)
        if $loglevel.get_level() <= 2 {
            println!("[{:?}]<{}> {}:{} - {}",
                     get_current_millis(),
                     $loglevel.get_name(),
                     file!(),
                     line!(),
                     format!($($format_args)+)
            );
        }
    })
}
