//! Miscellaneous helper functions for the lsfg-vk GUI.
//!
//! Provides toast notifications, Proton layer setup, profile import/export,
//! and running-game detection.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::GameConf;
use crate::process;

// ---------------------------------------------------------------------------
// 1. Toast helper
// ---------------------------------------------------------------------------

/// Show a short-lived toast notification on the given `ToastOverlay`.
///
/// The toast auto-dismisses after *timeout* seconds (default 3).
pub fn show_toast(overlay: &adw::ToastOverlay, message: &str, timeout: u32) {
    let toast = adw::Toast::builder()
        .title(message)
        .timeout(timeout)
        .build();
    overlay.add_toast(toast);
}

// ---------------------------------------------------------------------------
// 2. Proton / Vulkan implicit-layer setup
// ---------------------------------------------------------------------------

/// Copy `liblsfg-vk.so` into every discovered Steam Proton `lib/x86_64-linux-gnu/`
/// directory **and** write a Vulkan implicit-layer JSON manifest under
/// `~/.local/share/vulkan/implicit_layer.d/` so that Proton games pick up the
/// layer automatically.
///
/// Returns `Ok(count)` where *count* is the number of Proton installs patched,
/// or an error string describing what went wrong.
pub fn setup_proton() -> Result<usize, String> {
    // --- Locate liblsfg-vk.so ---
    let lib_candidates = [
        "/usr/lib/liblsfg-vk.so",
        "/usr/local/lib/liblsfg-vk.so",
        "/usr/lib/liblsfg-vk-layer.so",
    ];

    let lib_src = lib_candidates
        .iter()
        .find(|p| Path::new(p).exists())
        .ok_or_else(|| {
            format!(
                "liblsfg-vk.so not found. Searched: {}",
                lib_candidates.join(", ")
            )
        })?;

    // --- Find Steam Proton installs ---
    let steam_dir = find_steam_dir().ok_or("Steam installation not found")?;
    let proton_dirs = discover_proton_dirs(&steam_dir);

    if proton_dirs.is_empty() {
        return Err("No Proton installations found under Steam".into());
    }

    let mut patched = 0usize;

    for proton_root in &proton_dirs {
        let target_dir = proton_root.join("lib").join("x86_64-linux-gnu");
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create {}: {}", target_dir.display(), e))?;

        let target_so = target_dir.join("liblsfg-vk.so");
        fs::copy(lib_src, &target_so).map_err(|e| {
            format!(
                "Failed to copy {} -> {}: {}",
                lib_src,
                target_so.display(),
                e
            )
        })?;

        patched += 1;
    }

    // --- Write Vulkan implicit-layer JSON ---
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let xdg_data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".local").join("share"));

    let layer_dir = xdg_data.join("vulkan").join("implicit_layer.d");
    fs::create_dir_all(&layer_dir)
        .map_err(|e| format!("Failed to create {}: {}", layer_dir.display(), e))?;

    let abs_lib = fs::canonicalize(lib_src).unwrap_or_else(|_| PathBuf::from(lib_src));
    let abs_lib_str = abs_lib.to_string_lossy();

    let manifest = serde_json::json!({
        "file_format_version": "1.0.0",
        "layer": {
            "name": "VK_LAYER_LS_frame_generation",
            "type": "GLOBAL",
            "api_version": "1.3.0",
            "implementation_version": "1",
            "description": "Lossless Scaling Frame Generation (lsfg-vk)",
            "library_path": abs_lib_str.as_ref(),
            "enable_environment": {
                "LSFGVK_ENABLE": "1"
            },
            "disable_environment": {
                "DISABLE_LSFGVK": "1"
            }
        }
    });

    let manifest_path = layer_dir.join("VkLayer_LS_frame_generation.json");
    let manifest_str =
        serde_json::to_string_pretty(&manifest).map_err(|e| format!("JSON serialize: {e}"))?;
    fs::write(&manifest_path, manifest_str)
        .map_err(|e| format!("Failed to write {}: {}", manifest_path.display(), e))?;

    Ok(patched)
}

/// Locate the primary Steam installation directory.
fn find_steam_dir() -> Option<PathBuf> {
    let candidates = [
        dirs::home_dir().map(|h| h.join(".steam").join("steam")),
        dirs::home_dir().map(|h| h.join(".local").join("share").join("Steam")),
    ];

    for candidate in candidates.iter().flatten() {
        if candidate.join("steamapps").exists() {
            return Some(candidate.clone());
        }
    }

    None
}

/// Discover Proton installation directories under the Steam root.
///
/// Looks in both `~/.steam/steam/compatibilitytools.d/` and
/// `~/.steam/steam/steamapps/common/` for directories whose name contains
/// "Proton" and that have a `lib/` subtree (official Proton uses `dist/lib/`).
fn discover_proton_dirs(steam_dir: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();

    let search_roots = [
        steam_dir.join("compatibilitytools.d"),
        steam_dir.join("steamapps").join("common"),
    ];

    for root in &search_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !name.contains("Proton") {
                continue;
            }
            // Prefer `dist/` subdirectory if present (official Proton structure).
            let dist = path.join("dist");
            let candidate = if dist.is_dir() { &dist } else { &path };

            if candidate.join("lib").is_dir() {
                results.push(candidate.clone());
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// 3. Profile import / export
// ---------------------------------------------------------------------------

/// Export a slice of profiles to a JSON file at *path*.
///
/// Serialises with `serde_json::to_string_pretty`.
pub fn export_profiles(profiles: &[GameConf], path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(profiles)
        .map_err(|e| format!("Failed to serialise profiles: {e}"))?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
    }

    fs::write(path, json)
        .map_err(|e| format!("Failed to write profiles to {}: {}", path.display(), e))
}

/// Import profiles from a JSON file at *path*.
///
/// Returns the deserialised list of `GameConf` profiles.
pub fn import_profiles(path: &Path) -> Result<Vec<GameConf>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read profiles from {}: {}", path.display(), e))?;

    serde_json::from_str::<Vec<GameConf>>(&content)
        .map_err(|e| format!("Failed to parse profiles from {}: {}", path.display(), e))
}

// ---------------------------------------------------------------------------
// 4. Detect running game matching a profile
// ---------------------------------------------------------------------------

/// Scan running processes and return the first `GameConf` whose `active_in`
/// list matches a live process.
///
/// Uses `process::scan_processes()` from the sibling module to enumerate
/// `/proc`, then checks each process against the supplied *profiles*.
/// Returns `None` when no match is found.
pub fn detect_running_game(profiles: &[GameConf]) -> Option<GameConf> {
    let procs = process::scan_processes();

    for profile in profiles {
        let active_names = profile.active_in_list();
        for proc_info in &procs {
            // Effective exe name: prefer wine_exe for Proton/Wine processes
            let exe_name_owned;
            let exe_name = match proc_info.wine_exe.as_deref() {
                Some(w) => w,
                None => {
                    exe_name_owned = proc_info
                        .exe
                        .as_ref()
                        .and_then(|p| Path::new(p).file_name())
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| proc_info.comm.clone());
                    &exe_name_owned
                }
            };

            for active_in in &active_names {
                if proc_info.comm == *active_in
                    || exe_name.eq_ignore_ascii_case(active_in)
                    || proc_info
                        .exe
                        .as_ref()
                        .is_some_and(|e| e.ends_with(active_in))
                    || proc_info
                        .wine_exe
                        .as_ref()
                        .is_some_and(|w| w.eq_ignore_ascii_case(active_in))
                {
                    return Some(profile.clone());
                }
            }
        }
    }

    None
}
