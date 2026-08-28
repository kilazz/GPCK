// src/gpu/debug_layer.rs
//! # D3D12 InfoQueue Debug Layer & Diagnostic Interceptor
//!
//! Hooks into `ID3D12InfoQueue1` to intercept runtime GPU validation errors,
//! driver warnings, and DirectStorage pipeline issues directly into GPCK logging.

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D12::*;
#[cfg(windows)]
use windows::core::{Interface, PCSTR};

/// Enables the standard DirectX 12 Debug Layer if called before device creation.
#[cfg(windows)]
pub fn enable_d3d12_debug_layer() {
    unsafe {
        let mut debug: Option<ID3D12Debug> = None;
        if D3D12GetDebugInterface(&mut debug).is_ok()
            && let Some(dbg) = debug
        {
            dbg.EnableDebugLayer();
            crate::core::logger::log_info("[D3D12] Core Debug Layer enabled.");
        }
    }
}

/// Registers the thread-safe diagnostic callback on the active D3D12 device.
/// Returns a registration cookie if successful.
#[cfg(windows)]
pub fn attach_d3d12_debug_callback(device: &ID3D12Device) -> Option<u32> {
    unsafe {
        let info_queue: ID3D12InfoQueue1 = device.cast().ok()?;

        // Prevent internal message queue overflow
        let _ = info_queue.SetMessageCountLimit(8192);

        let mut cookie = 0u32;
        let hr = info_queue.RegisterMessageCallback(
            Some(d3d12_debug_callback_handler),
            D3D12_MESSAGE_CALLBACK_FLAG_NONE,
            std::ptr::null_mut(),
            &mut cookie,
        );

        if hr.is_ok() {
            crate::core::logger::log_info(
                "[D3D12] InfoQueue1 diagnostic callback attached successfully.",
            );
            Some(cookie)
        } else {
            None
        }
    }
}

/// Unregisters the diagnostic callback during device teardown.
#[cfg(windows)]
pub fn detach_d3d12_debug_callback(device: &ID3D12Device, cookie: u32) {
    unsafe {
        if let Ok(info_queue) = device.cast::<ID3D12InfoQueue1>() {
            let _ = info_queue.UnregisterMessageCallback(cookie);
        }
    }
}

/// Native C-ABI callback invoked by the DirectX 12 runtime on diagnostic events.
#[cfg(windows)]
unsafe extern "system" fn d3d12_debug_callback_handler(
    category: D3D12_MESSAGE_CATEGORY,
    severity: D3D12_MESSAGE_SEVERITY,
    id: D3D12_MESSAGE_ID,
    p_description: PCSTR,
    _p_context: *mut c_void,
) {
    if p_description.is_null() {
        return;
    }

    let msg = unsafe { std::ffi::CStr::from_ptr(p_description.0 as *const i8) }.to_string_lossy();

    match severity {
        D3D12_MESSAGE_SEVERITY_CORRUPTION | D3D12_MESSAGE_SEVERITY_ERROR => {
            crate::core::logger::log_error(&format!(
                "[D3D12 Error] Category: {:?}, ID: {:?} | {}",
                category, id, msg
            ));
        }
        D3D12_MESSAGE_SEVERITY_WARNING => {
            crate::core::logger::log_warn(&format!(
                "[D3D12 Warning] Category: {:?}, ID: {:?} | {}",
                category, id, msg
            ));
        }
        D3D12_MESSAGE_SEVERITY_INFO | D3D12_MESSAGE_SEVERITY_MESSAGE => {
            crate::core::logger::log_info(&format!("[D3D12 Info] {}", msg));
        }
        _ => {}
    }
}

// Non-Windows Stubs
#[cfg(not(windows))]
pub fn enable_d3d12_debug_layer() {}
#[cfg(not(windows))]
pub fn attach_d3d12_debug_callback(_device: &()) -> Option<u32> {
    None
}
#[cfg(not(windows))]
pub fn detach_d3d12_debug_callback(_device: &(), _cookie: u32) {}
