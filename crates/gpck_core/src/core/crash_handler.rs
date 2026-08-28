// crates/gpck_core/src/core/crash_handler.rs
//! # Cross-Platform Crash & Exception Handler
//!
//! Intercepts Rust panics, native Windows SEH exceptions (e.g. Access Violations),
//! and Unix POSIX signals (SIGSEGV, SIGBUS, SIGILL, SIGABRT) generating crash reports
//! in the centralized `<root>/crashes/` directory.

use super::paths::GpckPaths;
use std::fs::File;
use std::io::Write;
use std::panic;

/// Registers the global Rust panic hook and native platform crash handlers.
pub fn setup_crash_handler() {
    // Rust Standard Panic Hook
    panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let panic_msg = format!(
            "RUST PANIC DETECTED:\n{}\n\nBACKTRACE:\n{}",
            info, backtrace
        );

        super::logger::log_error(&panic_msg);
        write_crash_file("RUST_PANIC", &panic_msg);
    }));

    // Windows Native SEH Exceptions
    #[cfg(target_os = "windows")]
    unsafe {
        win_crash_handler::register_exception_filter();
    }

    // Unix POSIX Signals (Linux / macOS / Android / Consoles)
    #[cfg(unix)]
    unsafe {
        unix_crash_handler::register_signal_handlers();
    }
}

/// Writes a formatted crash report to `<root>/crashes/gpck_crash.log`.
fn write_crash_file(crash_type: &str, details: &str) {
    let crash_dir = GpckPaths::get_crashes_dir();
    let crash_file = crash_dir.join("gpck_crash.log");

    if let Ok(mut f) = File::create(&crash_file) {
        let sys_info = format!(
            "==================================================\n\
             GPCK CRASH REPORT\n\
             Crash Type : {}\n\
             OS         : {}\n\
             Arch       : {}\n\
             ==================================================\n\n\
             DETAILS:\n{}\n",
            crash_type,
            std::env::consts::OS,
            std::env::consts::ARCH,
            details
        );
        let _ = f.write_all(sys_info.as_bytes());
        let _ = f.flush();
    }
}

// Windows Native Handler
#[cfg(target_os = "windows")]
mod win_crash_handler {
    use super::*;
    use std::ffi::c_void;

    #[repr(C)]
    struct EXCEPTION_RECORD {
        exception_code: u32,
        exception_flags: u32,
        exception_record: *mut EXCEPTION_RECORD,
        exception_address: *mut c_void,
        number_parameters: u32,
        exception_information: [usize; 15],
    }

    #[repr(C)]
    struct EXCEPTION_POINTERS {
        exception_record: *mut EXCEPTION_RECORD,
        context_record: *mut c_void,
    }

    type PvectoredExceptionHandler =
        unsafe extern "system" fn(pointers: *mut EXCEPTION_POINTERS) -> i32;

    unsafe extern "system" fn unhandled_exception_filter(pointers: *mut EXCEPTION_POINTERS) -> i32 {
        unsafe {
            if pointers.is_null() || (*pointers).exception_record.is_null() {
                return 0;
            }

            let rec = &*(*pointers).exception_record;
            let code = rec.exception_code;
            let addr = rec.exception_address;

            let code_str = match code {
                0xc0000409 => {
                    "STATUS_STACK_BUFFER_OVERRUN (0xc0000409) - C++ Stack Corruption / FastFail"
                }
                0xc0000005 => {
                    "STATUS_ACCESS_VIOLATION (0xc0000005) - Invalid Memory Read/Write Pointer"
                }
                0xc000001d => {
                    "STATUS_ILLEGAL_INSTRUCTION (0xc000001d) - Unsupported CPU Instruction"
                }
                0xc0000094 => "STATUS_INTEGER_DIVIDE_BY_ZERO (0xc0000094)",
                _ => "UNKNOWN_NATIVE_WINDOWS_EXCEPTION",
            };

            let details = format!(
                "Exception Code    : 0x{:08X} ({})\n\
                 Exception Address : {:p}",
                code, code_str, addr
            );

            crate::core::logger::log_error(&details);
            write_crash_file("WINDOWS_NATIVE_EXCEPTION", &details);

            0 // EXCEPTION_CONTINUE_SEARCH
        }
    }

    pub unsafe fn register_exception_filter() {
        unsafe extern "system" {
            fn SetUnhandledExceptionFilter(
                lpTopLevelExceptionFilter: Option<PvectoredExceptionHandler>,
            ) -> *mut c_void;
        }
        unsafe {
            SetUnhandledExceptionFilter(Some(unhandled_exception_filter));
        }
    }
}

// Unix Native Signal Handler
#[cfg(unix)]
mod unix_crash_handler {
    use super::*;

    extern "C" fn handle_signal(sig: libc::c_int) {
        let sig_name = match sig {
            libc::SIGSEGV => "SIGSEGV (Segmentation Fault - Invalid Memory Access)",
            libc::SIGBUS => "SIGBUS (Bus Error - Alignment / Unmapped Memory Access)",
            libc::SIGILL => "SIGILL (Illegal Instruction)",
            libc::SIGFPE => "SIGFPE (Floating Point Exception / Divide by Zero)",
            libc::SIGABRT => "SIGABRT (Process Aborted)",
            _ => "UNKNOWN_UNIX_SIGNAL",
        };

        let details = format!("Captured POSIX Signal: {} ({})", sig, sig_name);
        crate::core::logger::log_error(&details);
        write_crash_file("UNIX_NATIVE_SIGNAL", &details);

        // Reset default handler and terminate
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    pub unsafe fn register_signal_handlers() {
        let signals = [
            libc::SIGSEGV,
            libc::SIGBUS,
            libc::SIGILL,
            libc::SIGFPE,
            libc::SIGABRT,
        ];

        for &sig in &signals {
            unsafe {
                libc::signal(sig, handle_signal as usize);
            }
        }
    }
}
