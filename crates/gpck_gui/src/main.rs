// crates/gpck_gui/src/main.rs
//! # GPCK Studio Desktop Application
//!
//! Slint-based Graphical User Interface for archive creation, live VFS visualizer,
//! texture inspection (PBR Neutral, Heatmap), and DirectStorage / GPU telemetry diagnostics.

mod controller;
mod converters;
mod preview;

use anyhow::{Result, anyhow};
use controller::GuiController;
use gpck_core::core::{crash_handler, logger, settings};
#[cfg(target_os = "windows")]
use gpck_core::gpu::directstorage::GpuDirectStorage;
use gpck_core::gpu::vulkan::VulkanDecompressor;
use slint::{ComponentHandle, SharedString};

slint::include_modules!();

fn main() -> Result<()> {
    // Initialize thread-safe logging and crash handlers
    let _log_guard = logger::init_logger();
    crash_handler::setup_crash_handler();

    logger::log_info("Starting GPCK Studio GUI...");

    // Instantiate Slint application window
    let ui = AppWindow::new().map_err(|e| anyhow!("Failed to initialize Slint UI: {}", e))?;

    // Load persistent user settings and synchronize with UI widgets
    let loaded_settings = settings::load_settings();
    controller::apply_settings_to_ui(&ui, &loaded_settings);

    // Query hardware backend capabilities and update status bar
    let mut backend_status = String::from("CPU: Native (Rayon/SIMD)");
    if let Ok(gpu) = VulkanDecompressor::new() {
        backend_status.push_str(&format!(" | Vulkan GPU: {}", gpu.device_name()));
    }

    #[cfg(target_os = "windows")]
    if let Ok(ds) = GpuDirectStorage::new()
        && ds.is_supported()
    {
        backend_status.push_str(" | DirectStorage: Active (Agility SDK 721)");
    }

    ui.set_status_text(SharedString::from(&backend_status));
    controller::append_log(&ui, &format!("Hardware initialized: {}", backend_status));

    // Attach event controller and button callbacks
    let controller = GuiController::new(ui);
    controller.attach_all_callbacks();

    // Run the main Slint event loop
    controller
        .ui
        .run()
        .map_err(|e| anyhow!("Slint runtime error: {}", e))
}
