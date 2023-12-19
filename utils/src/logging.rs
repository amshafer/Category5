// Category-based logging infrastructure
//
// This will be used from multiple threads, so it needs
// to be stateless
//
// Austin Shafer - 2020

#[allow(dead_code, non_camel_case_types)]
pub enum LogLevel {
    // in order of highest priority
    critical, // Urgent and must always be displayed
    error,
    debug,     // debugging related, fairly verbose
    verbose,   // more verbose debug output
    info,      // most verbose
    profiling, // profiling related timing, absurdly verbose
}

impl LogLevel {
    pub fn get_name(&mut self) -> &'static str {
        match self {
            LogLevel::critical => "critical",
            LogLevel::error => "error",
            LogLevel::debug => "debug",
            LogLevel::verbose => "verbose",
            LogLevel::info => "info",
            LogLevel::profiling => "profiling",
        }
    }

    pub fn get_level(&mut self) -> u32 {
        match self {
            LogLevel::critical => 0,
            LogLevel::error => 1,
            LogLevel::debug => 2,
            LogLevel::verbose => 3,
            LogLevel::info => 4,
            LogLevel::profiling => 5,
        }
    }
}

#[macro_export]
macro_rules! debug {
    ($($format_args:tt)+) => {{
        #[cfg(debug_assertions)]
        log::log_internal!(log::LogLevel::debug, $($format_args)+)
    }};
}

#[macro_export]
macro_rules! verbose {
    ($($format_args:tt)+) => {{
        #[cfg(debug_assertions)]
        log::log_internal!(log::LogLevel::verbose, $($format_args)+)
    }};
}

#[macro_export]
macro_rules! profiling {
    ($($format_args:tt)+) => {{
        #[cfg(debug_assertions)]
        log::log_internal!(log::LogLevel::profiling, $($format_args)+)
    }};
}

#[macro_export]
macro_rules! info {
    ($($format_args:tt)+) => {{
        #[cfg(debug_assertions)]
        log::log_internal!(log::LogLevel::info, $($format_args)+)
    }};
}

#[macro_export]
macro_rules! error {
    ($($format_args:tt)+) => {{
        log::log_internal!(log::LogLevel::error, $($format_args)+)
    }};
}

#[allow(unused_macros)]
#[macro_export]
macro_rules! log_internal{
    ($loglevel:expr, $($format_args:tt)+) => ({

        lazy_static::lazy_static! {
            static ref DEFAULT_LEVEL: u32 = crate::utils::logging::LogLevel::error.get_level();

            static ref LOG_LEVEL_RAW: u32 = match std::env::var("CATEGORY5_LOG") {
                Ok(val) => match val.as_str() {
                    "debug" => crate::utils::logging::LogLevel::debug.get_level(),
                    "verbose" => crate::utils::logging::LogLevel::verbose.get_level(),
                    "info" => crate::utils::logging::LogLevel::info.get_level(),
                    _ => *DEFAULT_LEVEL,
                },
                Err(_) => *DEFAULT_LEVEL,
            };
        }

        // !! NOTE: current log level set here !!
        //
        // Currently set to the debug level (2)
        let is_err = $loglevel.get_level() <= *DEFAULT_LEVEL;
        let mut should_log = $loglevel.get_level() <= *LOG_LEVEL_RAW;

        // If this variable is defined check that our log statements
        // come from files that contain this string
        if let Ok(m) = std::env::var("CATEGORY5_LOG_MATCH") {
            should_log = should_log && file!().contains(m.as_str());
        }

        // If it is an error or our conditions are met then log it
        if is_err || should_log {
            let fmtstr = format!("[{:?}]<{}> {}:{} - {}",
                log::get_current_millis(),
                $loglevel.get_name(),
                file!(),
                line!(),
                format!($($format_args)+)
            );

            println!("{}", fmtstr);

            #[cfg(debug_assertions)]
            {
                // Append to a log file
                use std::fs::OpenOptions;
                use std::io::prelude::*;

                let mut file = OpenOptions::new()
                    .write(true)
                    .append(true)
                    .create(true)
                    .open("/tmp/cat5_debug_log.txt")
                    .unwrap();

                if let Err(e) = writeln!(file, "{}", fmtstr) {
                    eprintln!("Couldn't write to debug file: {}", e);
                }
            }
        }
    })
}
