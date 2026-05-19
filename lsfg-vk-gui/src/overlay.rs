//! Compact game overlay HUD.
//!
//! A small, always-on-top window that shows real-time lsfg-vk
//! frame generation metrics overlaid on top of the game.

use adw::prelude::*;
use gtk4::glib::timeout_add_seconds_local;
use gtk4::{self as gtk, Align, Orientation};

use crate::process;

/// State for the overlay window.
pub struct OverlayState {
    pub window: gtk::Window,
}

/// Create the compact overlay window (initially hidden).
/// Shows a small FPS-counter style HUD in the top-left corner.
pub fn create_overlay() -> OverlayState {
    let window = gtk::Window::new();
    window.set_title(Some("lsfg-vk HUD"));
    window.set_default_size(280, 220);
    window.set_resizable(false);

    // Try to stay on top
    window.set_decorated(false);
    window.set_opacity(0.88);

    let vbox = gtk::Box::new(Orientation::Vertical, 4);
    vbox.set_margin_start(10);
    vbox.set_margin_end(10);
    vbox.set_margin_top(8);
    vbox.set_margin_bottom(8);

    // Header row with title + close button
    let header = gtk::Box::new(Orientation::Horizontal, 6);
    let title = gtk::Label::new(Some("lsfg-vk"));
    title.add_css_class("caption-heading");
    title.set_halign(Align::Start);
    title.set_hexpand(true);

    let close_btn = gtk::Button::from_icon_name("window-close-symbolic");
    close_btn.add_css_class("circular");
    close_btn.add_css_class("small-button");
    close_btn.set_valign(Align::Center);
    header.append(&title);
    header.append(&close_btn);

    // Metrics display (single label, monospace)
    let metrics_label = gtk::Label::new(Some(
        "Waiting for layer...\n\nLaunch a profiled game to see metrics.",
    ));
    metrics_label.set_halign(Align::Start);
    metrics_label.set_xalign(0.0);
    metrics_label.set_selectable(false);
    metrics_label.set_wrap(false);
    metrics_label.add_css_class("monospace");
    metrics_label.add_css_class("small-body");

    // Layer status line
    let layer_label = gtk::Label::new(Some("Layer: checking..."));
    layer_label.set_halign(Align::Start);
    layer_label.set_xalign(0.0);
    layer_label.add_css_class("dim-label");
    layer_label.add_css_class("caption");

    // Config path
    let config_label = gtk::Label::new(Some(&format!(
        "{}",
        crate::config::find_config_path().display()
    )));
    config_label.add_css_class("dim-label");
    config_label.add_css_class("caption");
    config_label.set_halign(Align::Start);

    vbox.append(&header);
    vbox.append(&gtk::Separator::new(Orientation::Horizontal));
    vbox.append(&metrics_label);
    vbox.append(&gtk::Separator::new(Orientation::Horizontal));
    vbox.append(&layer_label);
    vbox.append(&config_label);

    window.set_child(Some(&vbox));

    // Close button
    let window_c = window.clone();
    close_btn.connect_clicked(move |_| {
        window_c.hide();
    });

    // Periodic refresh every 1 second (fast enough for FPS display)
    let metrics_r = metrics_label.clone();
    let layer_r = layer_label.clone();
    let window_r = window.clone();

    timeout_add_seconds_local(1, move || {
        if !window_r.is_visible() {
            return gtk::glib::ControlFlow::Continue;
        }

        refresh_hud(&metrics_r, &layer_r);
        gtk::glib::ControlFlow::Continue
    });

    OverlayState { window }
}

/// Refresh the HUD with current data.
fn refresh_hud(metrics_label: &gtk::Label, layer_label: &gtk::Label) {
    // Layer status
    let status = process::check_layer_status();
    if status.disabled {
        layer_label.set_text("Layer: DISABLED");
        layer_label.remove_css_class("success");
        layer_label.add_css_class("error");
    } else if status.installed {
        layer_label.set_text("Layer: Active");
        layer_label.remove_css_class("error");
        layer_label.add_css_class("success");
    } else {
        layer_label.set_text("Layer: NOT installed");
        layer_label.remove_css_class("success");
        layer_label.add_css_class("error");
    }

    // Read metrics file
    match process::read_metrics() {
        Some(m) => {
            let real_fps = m.real_fps;
            let output_fps = m.output_fps;
            let ft = m.frame_time_ms;
            let uptime = m.uptime_secs as u32;

            // Build latency comparison section
            // Native latency = time between real frames (what latency would be without FG)
            // FG latency = actual measured frame time (includes FG pipeline buffering)
            // Overhead = the extra delay FG adds
            let native_lat = if m.native_latency_ms > 0.0 {
                m.native_latency_ms
            } else if real_fps > 0.0 {
                // Fallback: compute from real FPS if layer doesn't provide it yet
                1000.0 / real_fps
            } else {
                0.0
            };

            let fg_lat = if m.fg_latency_ms > 0.0 {
                m.fg_latency_ms
            } else {
                // Fallback: use measured frame time as FG latency
                ft
            };

            let overhead = if m.latency_overhead_ms > 0.0 {
                m.latency_overhead_ms
            } else if native_lat > 0.0 && fg_lat > native_lat {
                // Fallback: compute from difference
                fg_lat - native_lat
            } else {
                0.0
            };

            let latency_section = if native_lat > 0.0 {
                if overhead > 0.0 {
                    format!(
                        "{:<12}{:.1} ms (native)\n\
                         {:<12}{:.1} ms (with FG)\n\
                         {:<12}+{:.1} ms overhead",
                        "Native Lat:",
                        native_lat,
                        "FG Lat:",
                        fg_lat,
                        "Overhead:",
                        overhead,
                    )
                } else {
                    format!(
                        "{:<12}{:.1} ms (native)\n\
                         {:<12}{:.1} ms (with FG)",
                        "Native Lat:",
                        native_lat,
                        "FG Lat:",
                        fg_lat,
                    )
                }
            } else {
                format!("{:<12}{:.1} ms", "Frame Time:", ft)
            };

            let text = format!(
                "{:<12}{}\n\
                 {:<12}{}x\n\
                 {:<12}{:.0} -> {:.0} fps\n\
                 {}\n\
                 {:<12}{}\n\
                 {:<12}{:.0} | {:.0} fps\n\
                 {:<12}{:.0}s",
                "App:",
                m.app_name.as_deref().unwrap_or("?"),
                "Multiplier:",
                m.multiplier,
                "FPS:",
                real_fps,
                output_fps,
                latency_section,
                "Profile:",
                m.profile_name.as_deref().unwrap_or("?"),
                "Real|Out:",
                real_fps,
                output_fps,
                "Uptime:",
                uptime,
            );
            metrics_label.set_text(&text);
            metrics_label.remove_css_class("dim-label");
        }
        None => {
            // Check if any profiled games are running
            let processes = process::scan_processes();
            let config =
                crate::config::load_config().unwrap_or_else(|_| crate::config::default_config());
            let profile_data: Vec<(String, Vec<String>)> = config
                .profiles
                .iter()
                .map(|p| (p.name.clone(), p.active_in_list()))
                .collect();

            let matches = process::find_active_profiles(&processes, &profile_data);

            if matches.is_empty() {
                metrics_label.set_text(
                    "No active FG session.\n\
                     \n\
                     Launch a profiled game to see\n\
                     real-time frame generation metrics.",
                );
            } else {
                let names: Vec<String> = matches
                    .iter()
                    .map(|m| format!("{} (pid {})", m.process_name, m.pid))
                    .collect();
                metrics_label.set_text(&format!(
                    "Process running but no metrics:\n\
                     {}\n\
                     \n\
                     The game may not be using Vulkan,\n\
                     or the layer hasn't hooked in yet.",
                    names.join(", ")
                ));
            }
            metrics_label.add_css_class("dim-label");
        }
    }
}

/// Build an inline status widget for embedding in the main window.
/// Returns (box, layer_label, active_label, monitor_button, osd_button).
pub fn build_inline_status() -> (gtk::Box, gtk::Label, gtk::Label, gtk::ToggleButton, gtk::ToggleButton) {
    let box_ = gtk::Box::new(Orientation::Horizontal, 10);
    box_.add_css_class("toolbar");
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    box_.set_margin_start(4);
    box_.set_margin_end(4);

    // Layer indicator
    let layer_dot = gtk::Image::from_icon_name("emblem-default-symbolic");
    layer_dot.set_pixel_size(14);
    layer_dot.add_css_class("success");
    let layer_label = gtk::Label::new(Some("Layer: checking..."));
    layer_label.set_halign(Align::Start);

    // Active profiles indicator
    let active_icon = gtk::Image::from_icon_name("system-run-symbolic");
    active_icon.set_pixel_size(14);
    let active_label = gtk::Label::new(Some("No active profiles"));
    active_label.set_halign(Align::Start);
    active_label.set_hexpand(true);

    // Quick metrics (shows FPS if available)
    let metrics_label = gtk::Label::new(Some(""));
    metrics_label.set_halign(Align::End);
    metrics_label.add_css_class("monospace");

    // In-Game OSD toggle button (writes/creates /tmp/lsfg-vk-osd)
    let osd_btn = gtk::ToggleButton::new();
    osd_btn.set_icon_name("view-reveal-symbolic");
    osd_btn.set_tooltip_text(Some("Toggle In-Game Overlay OSD"));
    osd_btn.set_valign(Align::Center);
    // Read initial state from file
    osd_btn.set_active(std::path::Path::new("/tmp/lsfg-vk-osd").exists()
        && std::fs::read_to_string("/tmp/lsfg-vk-osd").unwrap_or_default().starts_with('1'));

    // Monitor button
    let monitor_btn = gtk::ToggleButton::new();
    monitor_btn.set_icon_name("utilities-system-monitor-symbolic");
    monitor_btn.set_tooltip_text(Some("Toggle FG Monitor"));
    monitor_btn.set_valign(Align::Center);

    box_.append(&layer_dot);
    box_.append(&layer_label);
    box_.append(&active_icon);
    box_.append(&active_label);
    box_.append(&metrics_label);
    box_.append(&osd_btn);
    box_.append(&monitor_btn);

    // OSD toggle handler: write/remove /tmp/lsfg-vk-osd
    {
        let osd_btn_r = osd_btn.clone();
        osd_btn.connect_toggled(move |btn| {
            if btn.is_active() {
                let _ = std::fs::write("/tmp/lsfg-vk-osd", "1");
                osd_btn_r.set_tooltip_text(Some("In-Game OSD: ON (click to hide)"));
                osd_btn_r.add_css_class("success");
            } else {
                let _ = std::fs::remove_file("/tmp/lsfg-vk-osd");
                osd_btn_r.set_tooltip_text(Some("In-Game OSD: OFF (click to show)"));
                osd_btn_r.remove_css_class("success");
            }
        });
    }
    // Set initial tooltip
    if osd_btn.is_active() {
        osd_btn.set_tooltip_text(Some("In-Game OSD: ON (click to hide)"));
        osd_btn.add_css_class("success");
    } else {
        osd_btn.set_tooltip_text(Some("In-Game OSD: OFF (click to show)"));
    }

    // Initial refresh
    let layer_label_r = layer_label.clone();
    let active_label_r = active_label.clone();
    let metrics_label_r = metrics_label.clone();
    let layer_dot_r = layer_dot.clone();
    refresh_inline(
        &layer_label_r,
        &active_label_r,
        &metrics_label_r,
        &layer_dot_r,
    );

    // Periodic refresh every 3 seconds
    timeout_add_seconds_local(3, move || {
        refresh_inline(
            &layer_label_r,
            &active_label_r,
            &metrics_label_r,
            &layer_dot_r,
        );
        gtk::glib::ControlFlow::Continue
    });

    (box_, layer_label, active_label, monitor_btn, osd_btn)
}

fn refresh_inline(
    layer_label: &gtk::Label,
    active_label: &gtk::Label,
    metrics_label: &gtk::Label,
    layer_dot: &gtk::Image,
) {
    let status = process::check_layer_status();

    if status.disabled {
        layer_label.set_text("Layer: DISABLED");
        layer_dot.set_icon_name(Some("dialog-warning-symbolic"));
        layer_dot.remove_css_class("success");
        layer_dot.add_css_class("warning");
    } else if status.installed {
        layer_label.set_text("Layer: Active");
        layer_dot.set_icon_name(Some("emblem-default-symbolic"));
        layer_dot.remove_css_class("warning");
        layer_dot.add_css_class("success");
    } else {
        layer_label.set_text("Layer: Not installed");
        layer_dot.set_icon_name(Some("dialog-error-symbolic"));
        layer_dot.remove_css_class("success");
        layer_dot.add_css_class("error");
    }

    // Check active profiles
    let processes = process::scan_processes();
    let config = crate::config::load_config().unwrap_or_else(|_| crate::config::default_config());
    let profile_data: Vec<(String, Vec<String>)> = config
        .profiles
        .iter()
        .map(|p| (p.name.clone(), p.active_in_list()))
        .collect();

    let matches = process::find_active_profiles(&processes, &profile_data);
    if matches.is_empty() {
        active_label.set_text("No active profiles");
        active_label.remove_css_class("success");
        metrics_label.set_text("");
    } else {
        let names: Vec<&str> = matches.iter().map(|m| m.profile_name.as_str()).collect();
        let unique: std::collections::BTreeSet<&str> = names.into_iter().collect();
        active_label.set_text(&format!(
            "Active: {} profile{}",
            unique.len(),
            if unique.len() == 1 { "" } else { "s" }
        ));
        active_label.add_css_class("success");

        // Show quick FPS and latency overhead from metrics if available
        if let Some(m) = process::read_metrics() {
            let mut text = format!("{:.0}fps -> {:.0}fps", m.real_fps, m.output_fps);
            // Compute latency overhead (prefer layer-provided, fallback to computed)
            let overhead = if m.latency_overhead_ms > 0.0 {
                m.latency_overhead_ms
            } else if m.real_fps > 0.0 {
                let native_lat = 1000.0 / m.real_fps;
                let fg_lat = if m.fg_latency_ms > 0.0 { m.fg_latency_ms } else { m.frame_time_ms };
                if fg_lat > native_lat { fg_lat - native_lat } else { 0.0 }
            } else {
                0.0
            };
            if overhead > 0.0 {
                text = format!(
                    "{:.0}fps -> {:.0}fps  +{:.0}ms lat",
                    m.real_fps, m.output_fps, overhead
                );
            }
            metrics_label.set_text(&text);
            metrics_label.add_css_class("success");
        } else {
            metrics_label.remove_css_class("success");
            metrics_label.set_text("");
        }
    }
}
