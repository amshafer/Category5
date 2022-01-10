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
    debug,     // debugging related, not verbose
    info,      // more verbose
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
macro_rules! debug {
    ($($format_args:tt)+) => {{
        #[cfg(debug_assertions)]
        log::log_internal!(log::LogLevel::debug, $($format_args)+)
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
                    "DEBUG" => crate::utils::logging::LogLevel::debug.get_level(),
                    _ => *DEFAULT_LEVEL,
                },
                Err(_) => *DEFAULT_LEVEL,
            };
        }

        // !! NOTE: current log level set here !!
        //
        // Currently set to the debug level (2)
        if $loglevel.get_level() <= *LOG_LEVEL_RAW {
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
