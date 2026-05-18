//! Process scanning and detection for Linux /proc filesystem.
//!
//! Scans /proc to find running executables, their command lines, and maps them
//! to installed .desktop applications and Steam games for a rich, searchable selector.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A detected running process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub comm: String,         // short name from /proc/[pid]/comm
    pub exe: Option<String>,  // full path from /proc/[pid]/exe
    pub cmdline: Vec<String>, // parsed argv from /proc/[pid]/cmdline
    /// For Proton/Wine processes, the detected Windows .exe name (e.g. "Subnautica2.exe").
    /// Extracted from cmdline Windows-style paths or /proc/[pid]/maps.
    pub wine_exe: Option<String>,
}

/// An installed application parsed from a .desktop file.
#[derive(Debug, Clone)]
pub struct DesktopApp {
    pub name: String,
    pub exec: Option<String>,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub filename: String, // basename like "steam.desktop"
    pub try_exec: Option<String>,
}

/// A Steam game parsed from appmanifest_*.acf.
#[derive(Debug, Clone)]
pub struct SteamGame {
    pub appid: String,
    pub name: String,
    pub install_dir: String,
    /// Full path to the game's install directory under steamapps/common/
    pub install_path: PathBuf,
    /// Detected .exe basenames in the game directory (smart-filtered to main executables).
    pub exes: Vec<String>,
}

/// Combined entry for the selector: a process matched to an app, or standalone.
#[derive(Debug, Clone)]
pub struct SelectorEntry {
    pub label: String,      // display name
    pub sublabel: String,   // secondary info (exe path, or "running")
    pub executable: String, // what to use as active_in (comm or basename of exe)
    pub is_running: bool,
    pub icon_name: Option<String>,
    pub source: EntrySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    RunningProcess,
    DesktopApp,
    SteamGame,
    Manual,
}

/// Check if a process name looks like a Proton/Wine wrapper (not a real game).
fn is_proton_wrapper_name(comm: &str) -> bool {
    // Common Proton/Wine process wrappers
    matches!(
        comm,
        "wine"
            | "wine64"
            | "wineserver"
            | "proton"
            | "steam"
            | "steamwebhelper"
            | "pv-adverb"
            | "srt-bwrap"
            | "reaper"
            | "pressure-vessel"
    ) || comm.starts_with("proton-")
        || comm.contains("bwrap")
}

/// Extract Windows .exe basename from a path string (e.g. "S:\common\Subnautica2\Subnautica2.exe").
fn extract_exe_basename(s: &str) -> Option<String> {
    // Handle both backslash and forward slash, and strip quotes
    let cleaned = s.trim_matches('"').trim_matches('\'');
    let basename = cleaned.rsplit(['\\', '/']).next()?.to_string();
    if basename.to_lowercase().ends_with(".exe") && basename.len() > 4 {
        Some(basename)
    } else {
        None
    }
}

/// Check /proc/[pid]/maps for .exe mappings (Wine-loaded Windows executables).
fn scan_maps_for_exe(pid: u32) -> Option<String> {
    let maps_path = format!("/proc/{pid}/maps");
    let Ok(content) = fs::read_to_string(&maps_path) else {
        return None;
    };

    let mut exe_entries: Vec<String> = Vec::new();

    for line in content.lines() {
        // Wine maps Windows .exe files; look for lines ending in .exe
        let trimmed = line.trim();
        if let Some(idx) = trimmed.rfind('.') {
            if trimmed[idx..].to_lowercase().starts_with(".exe") {
                // Extract the path from the last column after the inode field
                // Format: address perms offset dev inode pathname
                let parts: Vec<&str> = trimmed.splitn(6, char::is_whitespace).collect();
                if parts.len() >= 6 {
                    let pathname = parts[5].trim();
                    if pathname.to_lowercase().ends_with(".exe") {
                        if let Some(basename) =
                            pathname.rsplit(['\\', '/']).next().map(|s| s.to_string())
                        {
                            // Filter out common non-game exes
                            let lower = basename.to_lowercase();
                            if !lower.contains("crashpad")
                                && !lower.contains("crashreport")
                                && !lower.contains("unitycrashhandler")
                                && !lower.contains("redist")
                                && !lower.contains("setup")
                                && !lower.contains("installer")
                                && !lower.contains("uninstall")
                            {
                                exe_entries.push(basename);
                            }
                        }
                    }
                }
            }
        }
    }

    // Return the first (usually the main game exe), or None
    exe_entries.into_iter().next()
}

/// Detect the Windows .exe name for a Proton/Wine process.
fn detect_wine_exe(pid: u32, cmdline: &[String], exe: Option<&str>) -> Option<String> {
    let exe_lower = exe.map(|e| e.to_lowercase()).unwrap_or_default();
    let is_wine = exe_lower.contains("wine")
        || exe_lower.contains("proton")
        || exe_lower.contains("pressure-vessel")
        || exe_lower.contains("bwrap")
        || exe_lower.contains("reaper");

    if !is_wine {
        return None;
    }

    // Strategy 1: Check cmdline for Windows-style paths with .exe
    // Proton passes the Windows game exe as an argument, often with paths like:
    // "S:\common\Subnautica2\Subnautica2.exe" or "C:\..." etc.
    for arg in cmdline.iter().rev() {
        // Look for Windows-style paths (backslash or drive letter)
        if (arg.contains('\\') || arg.contains(":/") || arg.contains(":\\"))
            && arg.to_lowercase().contains(".exe")
        {
            if let Some(basename) = extract_exe_basename(arg) {
                let lower = basename.to_lowercase();
                // Skip common helper/trivial executables
                if !lower.starts_with("steam")
                    && !lower.starts_with("proton")
                    && !lower.contains("launcher")
                    && !lower.contains("crash")
                    && lower != "wine64.exe"
                    && lower != "wine.exe"
                {
                    return Some(basename);
                }
            }
        }
    }

    // Strategy 2: Check /proc/[pid]/maps for loaded .exe files
    if let Some(exe_name) = scan_maps_for_exe(pid) {
        return Some(exe_name);
    }

    None
}

/// Scan /proc for all running processes (excluding kernel threads).
pub fn scan_processes() -> Vec<ProcessInfo> {
    let mut procs = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return procs;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };

        let proc_dir = entry.path();

        // comm (short process name)
        let comm = fs::read_to_string(proc_dir.join("comm"))
            .unwrap_or_default()
            .trim()
            .to_string();

        if comm.is_empty() {
            continue;
        }

        // exe (symlink to binary)
        let exe = fs::read_link(proc_dir.join("exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned());

        // cmdline (null-separated arguments)
        let cmdline: Vec<String> = fs::read(proc_dir.join("cmdline"))
            .map(|data| {
                data.split(|&b| b == 0)
                    .filter_map(|s| {
                        let s = String::from_utf8_lossy(s).into_owned();
                        if s.is_empty() {
                            None
                        } else {
                            Some(s)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Detect Windows exe for Proton/Wine processes
        let wine_exe = detect_wine_exe(pid, &cmdline, exe.as_deref());

        procs.push(ProcessInfo {
            pid,
            comm,
            exe,
            cmdline,
            wine_exe,
        });
    }

    procs.sort_by_key(|a| a.comm.to_lowercase());
    procs.dedup_by(|a, b| {
        // Deduplicate by effective name: prefer wine_exe for Proton, comm otherwise
        let a_name = a.wine_exe.as_deref().unwrap_or(&a.comm);
        let b_name = b.wine_exe.as_deref().unwrap_or(&b.comm);
        a_name.eq_ignore_ascii_case(b_name)
    });
    procs
}

/// Parse all relevant .desktop files from standard XDG locations.
pub fn scan_desktop_apps() -> Vec<DesktopApp> {
    let mut apps = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    let dirs = xdg_data_dirs();

    for dir in &dirs {
        let apps_dir = dir.join("applications");
        load_desktop_dir(&apps_dir, &mut apps, &mut seen_names);
    }

    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn xdg_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // XDG_DATA_HOME
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            dirs.push(PathBuf::from(xdg));
        }
    }
    if dirs.is_empty() {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local").join("share"));
        }
    }

    // XDG_DATA_DIRS
    let extra =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for p in extra.split(':') {
        if !p.is_empty() {
            dirs.push(PathBuf::from(p));
        }
    }

    dirs
}

fn load_desktop_dir(
    dir: &Path,
    apps: &mut Vec<DesktopApp>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Recurse into subdirectories (e.g., applications/Utilities/)
        if path.is_dir() {
            load_desktop_dir(&path, apps, seen);
            continue;
        }

        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }

        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut in_desktop_entry = false;
        let mut name = String::new();
        let mut exec = None;
        let mut icon = None;
        let mut categories = Vec::new();
        let mut try_exec = None;
        let mut no_display = false;
        let mut terminal = false;

        for line in content.lines() {
            let line = line.trim();

            if line == "[Desktop Entry]" {
                in_desktop_entry = true;
                continue;
            }
            if line.starts_with('[') {
                in_desktop_entry = false;
                continue;
            }
            if !in_desktop_entry {
                continue;
            }

            if let Some(val) = line.strip_prefix("Name=") {
                name = val.to_string();
            } else if let Some(val) = line.strip_prefix("Exec=") {
                // Strip field codes like %f, %u, etc.
                let cleaned = val
                    .split_whitespace()
                    .filter(|part| !part.starts_with('%'))
                    .collect::<Vec<_>>()
                    .join(" ");
                exec = Some(cleaned);
            } else if let Some(val) = line.strip_prefix("Icon=") {
                icon = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("Categories=") {
                categories = val
                    .split(';')
                    .map(String::from)
                    .filter(|s| !s.is_empty())
                    .collect();
            } else if let Some(val) = line.strip_prefix("TryExec=") {
                try_exec = Some(val.to_string());
            } else if line == "NoDisplay=true" || line == "Hidden=true" {
                no_display = true;
            } else if line == "Terminal=true" {
                terminal = true;
            }
        }

        if no_display || terminal || name.is_empty() {
            continue;
        }

        // Skip duplicate names
        if !seen.insert(name.clone()) {
            continue;
        }

        apps.push(DesktopApp {
            name,
            exec,
            icon,
            categories,
            filename,
            try_exec,
        });
    }
}

/// Extract the executable basename from an Exec= line.
fn exec_basename(exec_line: &str) -> Option<String> {
    let first = exec_line.split_whitespace().next()?;
    let basename = Path::new(first).file_name()?.to_string_lossy().into_owned();
    if basename.is_empty() {
        None
    } else {
        Some(basename)
    }
}

/// Find the Steam install directory.
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

/// Parse a simple ACF/VDF key-value file (one level deep).
fn parse_acf(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        // Look for lines like: "key"\t\t"value"
        if line.starts_with('"') {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let key = parts[0].trim_matches('"').to_string();
                let val = parts[1].trim().trim_matches('"').to_string();
                map.insert(key, val);
            }
        }
    }
    map
}

/// Check if a Steam app name should be excluded (runtimes, Proton, redistributables, etc.).
fn is_steam_app_excluded(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("proton")
        || lower.contains("steam linux runtime")
        || lower.contains("steamworks common redistributable")
        || lower.contains("steam linux runtime")
        || lower.contains("eac runtime")
        || lower.contains("battleye runtime")
        || lower.starts_with("steam ")
}

/// Determine the main .exe files from a game install directory.
/// Heuristically filters out helper/trivial exes (crash handlers, uninstallers, etc.).
fn find_game_exes(install_path: &Path) -> Vec<String> {
    let mut exes = Vec::new();

    // First, check root level for .exe files
    if let Ok(entries) = fs::read_dir(install_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext.eq_ignore_ascii_case("exe") {
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy().to_string();
                        exes.push(name_str);
                    }
                }
            }
        }
    }

    // If no root-level exes, search deeper (up to 3 levels)
    if exes.is_empty() {
        if let Ok(entries) = fs::read_dir(install_path) {
            for entry in entries.flatten() {
                let subdir = entry.path();
                if subdir.is_dir() {
                    if let Ok(sub_entries) = fs::read_dir(&subdir) {
                        for sub_entry in sub_entries.flatten() {
                            let path = sub_entry.path();
                            if let Some(ext) = path.extension() {
                                if ext.eq_ignore_ascii_case("exe") {
                                    if let Some(name) = path.file_name() {
                                        let name_str = name.to_string_lossy().to_string();
                                        exes.push(name_str);
                                    }
                                }
                            }
                            // One more level (e.g., Binaries/Win64/)
                            if path.is_dir() {
                                if let Ok(deep_entries) = fs::read_dir(&path) {
                                    for deep_entry in deep_entries.flatten() {
                                        let deep_path = deep_entry.path();
                                        if let Some(ext) = deep_path.extension() {
                                            if ext.eq_ignore_ascii_case("exe") {
                                                if let Some(name) = deep_path.file_name() {
                                                    let name_str =
                                                        name.to_string_lossy().to_string();
                                                    exes.push(name_str);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Filter out helper/trivial executables
    let filtered: Vec<String> = exes
        .into_iter()
        .filter(|exe| {
            let lower = exe.to_lowercase();
            !lower.contains("crash")
                && !lower.contains("uninstall")
                && !lower.contains("redist")
                && !lower.contains("setup")
                && lower != "createdump.exe"
                && !lower.contains("handler")
                && !lower.contains("diag")
                && !lower.contains("report")
        })
        .collect();

    // If we have *Win64-Shipping.exe or similar, prefer those
    let shipping: Vec<String> = filtered
        .iter()
        .filter(|e| e.to_lowercase().contains("shipping") || e.to_lowercase().contains("win64"))
        .cloned()
        .collect();

    if !shipping.is_empty() {
        shipping
    } else if !filtered.is_empty() {
        filtered
    } else {
        Vec::new()
    }
}

/// Parse Steam's libraryfolders.vdf to get all Steam library paths.
fn parse_steam_library_paths(steam_dir: &Path) -> Vec<PathBuf> {
    let vdf_path = steam_dir.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = fs::read_to_string(&vdf_path) else {
        return vec![steam_dir.join("steamapps")];
    };

    let mut paths = Vec::new();
    let mut in_apps = false;

    for line in content.lines() {
        let line = line.trim();
        if line.contains("\"path\"") {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let val = parts[1].trim().trim_matches('"');
                let steamapps = PathBuf::from(val).join("steamapps");
                if steamapps.exists() && !paths.contains(&steamapps) {
                    paths.push(steamapps);
                }
            }
        }
        // Track nesting to avoid parsing app ids
        if line.contains("\"apps\"") {
            in_apps = true;
        } else if line == "}" && in_apps {
            in_apps = false;
        }
    }

    if paths.is_empty() {
        paths.push(steam_dir.join("steamapps"));
    }

    paths
}

/// Scan Steam installation for installed games.
/// Parses appmanifest_*.acf files and finds .exe files in game directories.
pub fn scan_steam_games() -> Vec<SteamGame> {
    let Some(steam_dir) = find_steam_dir() else {
        return Vec::new();
    };

    let library_paths = parse_steam_library_paths(&steam_dir);
    let mut games = Vec::new();
    let mut seen_appids = std::collections::HashSet::new();

    for lib_path in &library_paths {
        let Ok(entries) = fs::read_dir(lib_path) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            if !filename.starts_with("appmanifest_") || !filename.ends_with(".acf") {
                continue;
            }

            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };

            let fields = parse_acf(&content);
            let appid = fields.get("appid").cloned().unwrap_or_default();
            let name = fields.get("name").cloned().unwrap_or_default();
            let install_dir = fields.get("installdir").cloned().unwrap_or_default();

            if appid.is_empty() || name.is_empty() || install_dir.is_empty() {
                continue;
            }

            // Skip duplicates
            if !seen_appids.insert(appid.clone()) {
                continue;
            }

            // Skip runtimes and tools
            if is_steam_app_excluded(&name) {
                continue;
            }

            let install_path = lib_path.join("common").join(&install_dir);
            if !install_path.exists() {
                continue;
            }

            let exes = find_game_exes(&install_path);

            games.push(SteamGame {
                appid,
                name,
                install_dir,
                install_path,
                exes,
            });
        }
    }

    games.sort_by_key(|g| g.name.to_lowercase());
    games
}

/// Build the combined list of selector entries from running processes, desktop apps,
/// and Steam games. Groups them intelligently: running processes first, then installed
/// apps and Steam games, deduplicating.
pub fn build_selector_entries(
    processes: &[ProcessInfo],
    desktop_apps: &[DesktopApp],
    already_added: &[String],
) -> Vec<SelectorEntry> {
    let mut entries = Vec::new();
    let mut seen_exes: HashMap<String, usize> = HashMap::new();

    // Already-added set for marking
    let added_set: std::collections::HashSet<String> = already_added.iter().cloned().collect();

    // Phase 1: Running processes
    for proc in processes {
        // For Proton/Wine, prefer the detected Windows exe name
        let exe_name = proc.wine_exe.as_ref().cloned().unwrap_or_else(|| {
            proc.exe
                .as_ref()
                .and_then(|p| Path::new(p).file_name())
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| proc.comm.clone())
        });

        let display = exe_name.trim().to_string();
        if display.is_empty()
            || display == "lsfg-vk-gui"
            || is_proton_wrapper_name(&display.replace(".exe", ""))
        {
            // Skip Proton wrapper names (we'll show the Windows exe name instead)
            if proc.wine_exe.is_none() && is_proton_wrapper_name(&proc.comm) {
                continue;
            }
            if display.is_empty() || display == "lsfg-vk-gui" {
                continue;
            }
        }

        if seen_exes.contains_key(&display.to_lowercase()) {
            continue;
        }

        // Try to find matching desktop app for icon/rich name
        let matched_app = desktop_apps.iter().find(|app| {
            if let Some(ref exec) = app.exec {
                exec_basename(exec).is_some_and(|b| b.eq_ignore_ascii_case(&display))
            } else {
                false
            }
        });

        let label = matched_app.map_or_else(|| display.clone(), |app| app.name.clone());

        let icon = matched_app.and_then(|app| app.icon.clone());

        let is_added = added_set.contains(&display);

        let sublabel = if is_added {
            "Already in profile".to_string()
        } else if proc.wine_exe.is_some() {
            format!(
                "Proton/Wine: {}",
                proc.exe.as_deref().unwrap_or("(unknown)")
            )
        } else {
            proc.exe.as_deref().unwrap_or("(unknown path)").to_string()
        };

        seen_exes.insert(display.to_lowercase(), entries.len());
        entries.push(SelectorEntry {
            label,
            sublabel,
            executable: display,
            is_running: true,
            icon_name: icon,
            source: EntrySource::RunningProcess,
        });
    }

    // Phase 2: Desktop apps not already seen as running
    for app in desktop_apps {
        let Some(ref exec) = app.exec else { continue };
        let Some(exe_name) = exec_basename(exec) else {
            continue;
        };

        if seen_exes.contains_key(&exe_name.to_lowercase()) {
            continue;
        }

        // Filter to Game-related categories or common game stores
        let is_game = app.categories.iter().any(|c| c == "Game")
            || app.filename.contains("steam")
            || app.filename.contains("heroic")
            || app.filename.contains("lutris")
            || app.filename.contains("bottles")
            || app.filename.contains("proton");

        if !is_game {
            continue;
        }

        let is_added = added_set.contains(&exe_name);

        seen_exes.insert(exe_name.to_lowercase(), entries.len());
        entries.push(SelectorEntry {
            label: app.name.clone(),
            sublabel: if is_added {
                "Already in profile".to_string()
            } else {
                format!("Installed ({})", app.filename.replace(".desktop", ""))
            },
            executable: exe_name,
            is_running: false,
            icon_name: app.icon.clone(),
            source: EntrySource::DesktopApp,
        });
    }

    // Phase 3: Steam games not already seen
    let steam_games = scan_steam_games();
    for game in &steam_games {
        for exe in &game.exes {
            if seen_exes.contains_key(&exe.to_lowercase()) {
                continue;
            }

            let is_added = added_set.contains(exe);

            seen_exes.insert(exe.to_lowercase(), entries.len());
            entries.push(SelectorEntry {
                label: format!("{} ({})", game.name, exe),
                sublabel: if is_added {
                    "Already in profile".to_string()
                } else {
                    format!("Steam: {}", game.install_path.display())
                },
                executable: exe.clone(),
                is_running: false,
                icon_name: Some("applications-games-symbolic".to_string()),
                source: EntrySource::SteamGame,
            });
        }
    }

    entries
}

/// Layer JSON filenames to search for (covers both upstream and fork naming).
const LAYER_JSON_NAMES: &[&str] = &[
    "VkLayer_LS_frame_generation.json",
    "VkLayer_LSFGVK_frame_generation.json",
];

/// Environment variables that disable the layer.
const DISABLE_ENV_VARS: &[&str] = &["DISABLE_LSFG", "DISABLE_LSFGVK"];

/// Directories to search for the Vulkan implicit layer JSON.
fn layer_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // /etc/vulkan/implicit_layer.d/ (where pacman/AUR installs it)
    dirs.push(PathBuf::from("/etc/vulkan/implicit_layer.d"));

    // XDG_DATA_HOME (~/.local/share/vulkan/implicit_layer.d/)
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    let xdg_data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join(".local").join("share"));
    dirs.push(xdg_data.join("vulkan").join("implicit_layer.d"));

    // System dirs
    dirs.push(PathBuf::from("/usr/share/vulkan/implicit_layer.d"));
    dirs.push(PathBuf::from("/usr/local/share/vulkan/implicit_layer.d"));

    // XDG_DATA_DIRS
    let extra =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for p in extra.split(':') {
        if !p.is_empty() {
            dirs.push(PathBuf::from(p).join("vulkan").join("implicit_layer.d"));
        }
    }

    dirs
}

/// Check if the lsfg-vk Vulkan layer is installed and enabled.
/// Searches multiple directories and recognizes both upstream and fork naming.
pub fn check_layer_status() -> LayerStatus {
    let mut found_path: Option<PathBuf> = None;

    for dir in layer_search_dirs() {
        for name in LAYER_JSON_NAMES {
            let candidate = dir.join(name);
            if candidate.exists() {
                found_path = Some(candidate);
                break;
            }
        }
        if found_path.is_some() {
            break;
        }
    }

    let installed = found_path.is_some();

    // Check all known disable env vars
    let disabled = DISABLE_ENV_VARS
        .iter()
        .any(|var| std::env::var(var).is_ok_and(|v| v == "1"));

    // Also check if the layer .so exists
    let so_exists = Path::new("/usr/lib/liblsfg-vk.so").exists()
        || Path::new("/usr/local/lib/liblsfg-vk.so").exists()
        || Path::new("/usr/lib/liblsfg-vk-layer.so").exists();

    LayerStatus {
        installed: installed && so_exists,
        disabled,
        layer_path: found_path,
    }
}

#[derive(Debug, Clone)]
pub struct LayerStatus {
    pub installed: bool,
    pub disabled: bool,
    pub layer_path: Option<PathBuf>,
}

/// Read the lsfg-vk metrics file if it exists.
/// The layer writes metrics to /tmp/lsfg-vk-metrics.json when available.
pub fn read_metrics() -> Option<Metrics> {
    let paths = [
        PathBuf::from("/tmp/lsfg-vk-metrics.json"),
        dirs::cache_dir()?.join("lsfg-vk").join("metrics.json"),
    ];

    for path in &paths {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(metrics) = serde_json::from_str::<Metrics>(&content) {
                return Some(metrics);
            }
            // Try simpler parse for future format
            let _ = content; // file exists but unreadable format
        }
    }
    None
}

/// Runtime metrics from the lsfg-vk layer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Metrics {
    pub app_name: Option<String>,
    pub profile_name: Option<String>,
    pub multiplier: u32,
    pub real_fps: f32,
    pub output_fps: f32,
    pub frame_time_ms: f32,
    pub gpu_name: Option<String>,
    pub flow_scale: f32,
    pub performance_mode: bool,
    pub dropped_frames: u32,
    pub uptime_secs: f32,
}

/// Find which profiles match currently running processes.
pub fn find_active_profiles(
    processes: &[ProcessInfo],
    profile_names: &[(String, Vec<String>)], // (profile_name, active_in list)
) -> Vec<ActiveMatch> {
    let mut matches = Vec::new();

    for (profile_name, active_ins) in profile_names {
        for active_in in active_ins {
            for proc in processes {
                // Get the effective exe name: prefer wine_exe for Proton/Wine processes
                let exe_name_owned;
                let exe_name = match proc.wine_exe.as_deref() {
                    Some(w) => w,
                    None => {
                        exe_name_owned = proc
                            .exe
                            .as_ref()
                            .and_then(|p| Path::new(p).file_name())
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_else(|| proc.comm.clone());
                        &exe_name_owned
                    }
                };

                // Match by comm (exact), exe basename (case-insensitive), full exe path,
                // or wine_exe (for Proton/Wine processes)
                let matched = proc.comm == *active_in
                    || exe_name.eq_ignore_ascii_case(active_in)
                    || proc.exe.as_ref().is_some_and(|e| e.ends_with(active_in))
                    || proc
                        .wine_exe
                        .as_ref()
                        .is_some_and(|w| w.eq_ignore_ascii_case(active_in));

                if matched {
                    matches.push(ActiveMatch {
                        profile_name: profile_name.clone(),
                        process_name: proc.comm.clone(),
                        process_exe: proc.exe.clone(),
                        pid: proc.pid,
                        active_in_match: active_in.clone(),
                    });
                }
            }
        }
    }

    matches
}

#[derive(Debug, Clone)]
pub struct ActiveMatch {
    pub profile_name: String,
    pub process_name: String,
    pub process_exe: Option<String>,
    pub pid: u32,
    pub active_in_match: String,
}
