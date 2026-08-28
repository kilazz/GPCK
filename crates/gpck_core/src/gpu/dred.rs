// src/gpu/dred.rs
//! # Device Removed Extended Data (DRED 1.3) Diagnostics
//!
//! Enables automatic GPU breadcrumbs and virtual address fault capture to accurately
//! pinpoint the exact asset and streaming request that caused a GPU crash or TDR.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::sync::{Once, OnceLock};
#[cfg(windows)]
use windows::Win32::Graphics::Direct3D12::*;
#[cfg(windows)]
use windows::core::{GUID, Interface};

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_DRED_ALLOCATION_NODE1 {
    pub ObjectNameA: *const i8,
    pub ObjectNameW: *const u16,
    pub AllocationType: u32,
    pub pNext: *const D3D12_DRED_ALLOCATION_NODE1,
    pub pObject: *const c_void,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_DRED_BREADCRUMB_CONTEXT {
    pub BreadcrumbIndex: u32,
    pub pContextString: *const u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_AUTO_BREADCRUMB_NODE1 {
    pub pCommandListDebugNameA: *const i8,
    pub pCommandListDebugNameW: *const u16,
    pub pCommandQueueDebugNameA: *const i8,
    pub pCommandQueueDebugNameW: *const u16,
    pub pCommandList: *mut c_void,
    pub pCommandQueue: *mut c_void,
    pub BreadcrumbCount: u32,
    pub pLastBreadcrumbValue: *const u32,
    pub pCommandHistory: *const u32,
    pub pNext: *const D3D12_AUTO_BREADCRUMB_NODE1,
    pub BreadcrumbContextsCount: u32,
    pub pBreadcrumbContexts: *const D3D12_DRED_BREADCRUMB_CONTEXT,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT1 {
    pub pHeadAutoBreadcrumbNode: *const D3D12_AUTO_BREADCRUMB_NODE1,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_DRED_PAGE_FAULT_OUTPUT2 {
    pub PageFaultVA: u64,
    pub pHeadExistingAllocationNode: *const D3D12_DRED_ALLOCATION_NODE1,
    pub pHeadRecentFreedAllocationNode: *const D3D12_DRED_ALLOCATION_NODE1,
    pub PageFaultFlags: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct D3D12_DEVICE_REMOVED_EXTENDED_DATA3 {
    pub DeviceRemovedReason: windows::core::HRESULT,
    pub AutoBreadcrumbsOutput: D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT1,
    pub PageFaultOutput: D3D12_DRED_PAGE_FAULT_OUTPUT2,
    pub DeviceState: u32,
}

#[repr(C)]
struct ID3D12DeviceRemovedExtendedDataSettingsVtbl {
    pub parent: [usize; 3],
    pub SetAutoBreadcrumbsEnablement: unsafe extern "system" fn(this: *mut c_void, enablement: u32),
    pub SetPageFaultEnablement: unsafe extern "system" fn(this: *mut c_void, enablement: u32),
    pub SetWatsonDumpEnablement: unsafe extern "system" fn(this: *mut c_void, enablement: u32),
}

#[repr(C)]
struct ID3D12DeviceRemovedExtendedData2Vtbl {
    pub parent: [usize; 3],
    pub GetAutoBreadcrumbsOutput1: unsafe extern "system" fn(
        this: *mut c_void,
        pOutput: *mut D3D12_DRED_AUTO_BREADCRUMBS_OUTPUT1,
    ) -> windows::core::HRESULT,
    pub GetPageFaultAllocationOutput2: unsafe extern "system" fn(
        this: *mut c_void,
        pOutput: *mut D3D12_DRED_PAGE_FAULT_OUTPUT2,
    ) -> windows::core::HRESULT,
    pub GetDeviceState: unsafe extern "system" fn(this: *mut c_void) -> u32,
}

pub struct DredDiagnosticEngine;

impl DredDiagnosticEngine {
    /// Configures DRED before creating the D3D12 device.
    #[cfg(windows)]
    pub fn configure_dred() {
        static DRED_INIT: Once = Once::new();
        static D3D12_LIB: OnceLock<Option<libloading::Library>> = OnceLock::new();

        DRED_INIT.call_once(|| {
            let clsid_dred_settings = GUID::from_u128(0x82BC481C_6B9B_4030_AEDB_7EE3D1DF1E63);
            let iid_dred_settings = GUID::from_u128(0x82BC481C_6B9B_4030_AEDB_7EE3D1DF1E63);

            let d3d12_lib =
                D3D12_LIB.get_or_init(|| unsafe { libloading::Library::new("d3d12.dll").ok() });

            if let Some(lib) = d3d12_lib
                && let Ok(get_interface) = unsafe {
                    lib.get::<unsafe extern "system" fn(
                        *const GUID,
                        *const GUID,
                        *mut *mut c_void,
                    ) -> windows::core::HRESULT>(b"D3D12GetInterface\0")
                }
            {
                unsafe {
                    let mut settings_raw: *mut c_void = std::ptr::null_mut();
                    if (get_interface)(&clsid_dred_settings, &iid_dred_settings, &mut settings_raw)
                        .is_ok()
                        && !settings_raw.is_null()
                    {
                        let vtbl = *(settings_raw
                            as *const *const ID3D12DeviceRemovedExtendedDataSettingsVtbl);
                        ((*vtbl).SetAutoBreadcrumbsEnablement)(settings_raw, 2);
                        ((*vtbl).SetPageFaultEnablement)(settings_raw, 2);

                        // Release the COM interface
                        let unk_vtbl = *(settings_raw as *const *const usize);
                        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                            std::mem::transmute(*unk_vtbl.add(2));
                        release(settings_raw);

                        crate::core::logger::log_info(
                            "[DRED] DRED 1.3 Forced On (Breadcrumbs & PageFault Tracking active).",
                        );
                    }
                }
            }
        });
    }

    /// Captures DRED diagnostic output upon GPU Device Removal / Crash.
    #[cfg(windows)]
    pub fn analyze_device_removal(device: &ID3D12Device) -> Option<String> {
        let iid_dred2 = GUID::from_u128(0x67FC5816_E4CA_4915_BF18_42541272DA54);
        let device_raw = Interface::as_raw(device);
        let mut dred_raw: *mut c_void = std::ptr::null_mut();

        unsafe {
            let unk_vtbl = *(device_raw as *const *const usize);
            let query_interface: unsafe extern "system" fn(
                *mut c_void,
                *const GUID,
                *mut *mut c_void,
            ) -> windows::core::HRESULT = std::mem::transmute(*unk_vtbl);

            if (query_interface)(device_raw, &iid_dred2, &mut dred_raw).is_ok()
                && !dred_raw.is_null()
            {
                let vtbl = *(dred_raw as *const *const ID3D12DeviceRemovedExtendedData2Vtbl);
                let mut page_fault = D3D12_DRED_PAGE_FAULT_OUTPUT2 {
                    PageFaultVA: 0,
                    pHeadExistingAllocationNode: std::ptr::null(),
                    pHeadRecentFreedAllocationNode: std::ptr::null(),
                    PageFaultFlags: 0,
                };

                let mut report = String::from("=== GPU DEVICE REMOVAL DRED 1.3 REPORT ===\n");
                if ((*vtbl).GetPageFaultAllocationOutput2)(dred_raw, &mut page_fault).is_ok() {
                    report.push_str(&format!(
                        "Faulting GPU Virtual Address (VA): 0x{:016X}\n",
                        page_fault.PageFaultVA
                    ));

                    if !page_fault.pHeadExistingAllocationNode.is_null() {
                        let node = &*page_fault.pHeadExistingAllocationNode;
                        if !node.ObjectNameA.is_null() {
                            let name = std::ffi::CStr::from_ptr(node.ObjectNameA).to_string_lossy();
                            report.push_str(&format!(
                                "Matching Live Resource Allocation: '{}'\n",
                                name
                            ));
                        }
                    }
                }

                return Some(report);
            }
        }
        None
    }
}
