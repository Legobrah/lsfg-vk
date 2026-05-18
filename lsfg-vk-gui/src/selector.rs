//! Smart executable selector dialog.
//!
//! A searchable, multi-tab dialog that lets the user pick executables from:
//! - Currently running processes (with Proton/Wine detection)
//! - Installed games (from .desktop files AND Steam library)
//! - Manual file picker
//!
//! BUG FIXES vs. original:
//! - Search filtering stores filtered entries in Rc<RefCell<>> so response handler
//!   always uses the correct filtered list (no index mismatch).
//! - A persistent `selected_executables` set accumulates selections from ALL tabs,
//!   so switching tabs no longer loses earlier selections.
//! - Steam games from appmanifest_*.acf are included in the Installed Games tab.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::glib::clone;
use gtk4::{self as gtk, Align, Orientation, PolicyType, SelectionMode};

use crate::process::{
    build_selector_entries, scan_desktop_apps, scan_processes, EntrySource, SelectorEntry,
};

/// Show the smart selector dialog and call `on_apply` with the selected executable names.
pub fn show_selector_dialog(
    parent: Option<&impl IsA<gtk::Window>>,
    already_added: &[String],
    on_apply: Box<dyn Fn(Vec<String>)>,
) {
    let dialog = gtk::Dialog::with_buttons(
        Some("Select Executables"),
        parent,
        gtk::DialogFlags::MODAL,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Add Selected", gtk::ResponseType::Ok),
        ],
    );
    dialog.set_default_size(640, 520);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);

    // Persistent set of selected executables across ALL tabs.
    let selected_executables: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));

    // Search bar
    let search = gtk::SearchEntry::new();
    search.set_hexpand(true);
    search.set_placeholder_text(Some("Search processes, games..."));
    content.append(&search);

    // Tab switcher: Running | Installed Games | Manual
    let tabs = adw::ViewSwitcher::new();
    let stack = adw::ViewStack::new();
    tabs.set_stack(Some(&stack));
    content.append(&tabs);

    // --- Running processes page ---
    let running_scroll = gtk::ScrolledWindow::new();
    running_scroll.set_vexpand(true);
    running_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    let running_list = gtk::ListBox::new();
    running_list.set_selection_mode(SelectionMode::Multiple);
    running_list.add_css_class("rich-list");
    running_scroll.set_child(Some(&running_list));

    // --- Installed games page ---
    let games_scroll = gtk::ScrolledWindow::new();
    games_scroll.set_vexpand(true);
    games_scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    let games_list = gtk::ListBox::new();
    games_list.set_selection_mode(SelectionMode::Multiple);
    games_list.add_css_class("rich-list");
    games_scroll.set_child(Some(&games_list));

    // --- Manual entry page ---
    let manual_box = gtk::Box::new(Orientation::Vertical, 8);
    manual_box.set_valign(Align::Start);
    manual_box.set_margin_top(12);

    let manual_label = gtk::Label::new(Some("Enter executable names (one per line):"));
    manual_label.set_halign(Align::Start);
    manual_box.append(&manual_label);

    let manual_buf = gtk::TextBuffer::new(None);
    let manual_view = gtk::TextView::with_buffer(&manual_buf);
    manual_view.set_wrap_mode(gtk::WrapMode::WordChar);
    let manual_scroll = gtk::ScrolledWindow::new();
    manual_scroll.set_vexpand(true);
    manual_view.set_top_margin(8);
    manual_view.set_bottom_margin(8);
    manual_view.set_left_margin(8);
    manual_view.set_right_margin(8);
    manual_scroll.set_child(Some(&manual_view));
    manual_box.append(&manual_scroll);

    // File picker button
    let file_btn = gtk::Button::with_label("Browse for executable...");
    file_btn.add_css_class("pill");
    file_btn.set_halign(Align::Start);
    manual_box.append(&file_btn);

    let manual_buf_c = manual_buf.clone();
    let dialog_c = dialog.clone();
    file_btn.connect_clicked(move |_| {
        let chooser = gtk::FileChooserDialog::new(
            Some("Select Executable"),
            Some(&dialog_c),
            gtk::FileChooserAction::Open,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Select", gtk::ResponseType::Accept),
            ],
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Executables"));
        filter.add_mime_type("application/x-executable");
        filter.add_mime_type("application/x-sharedlib");
        filter.add_pattern("*.exe");
        filter.add_pattern("*");
        chooser.set_filter(&filter);

        let manual_buf_c2 = manual_buf_c.clone();
        chooser.connect_response(move |chooser, resp| {
            if resp == gtk::ResponseType::Accept {
                if let Some(file) = chooser.file() {
                    if let Some(path) = file.path() {
                        let name = path
                            .file_name()
                            .map(|f| f.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let current = manual_buf_c2.text(
                            &manual_buf_c2.start_iter(),
                            &manual_buf_c2.end_iter(),
                            false,
                        );
                        let new_text = if current.is_empty() {
                            name
                        } else {
                            format!("{current}\n{name}")
                        };
                        manual_buf_c2.set_text(&new_text);
                    }
                }
            }
            chooser.close();
        });
        chooser.show();
    });

    // Add pages to stack
    let running_page = stack.add_titled(&running_scroll, Some("running"), "Running");
    stack.add_titled(&games_scroll, Some("games"), "Installed Games");
    stack.add_titled(&manual_box, Some("manual"), "Manual");

    // Set icons on pages
    running_page.set_icon_name(Some("media-playback-start-symbolic"));
    let games_page = stack.page(&games_scroll);
    games_page.set_icon_name(Some("applications-games-symbolic"));
    let manual_page = stack.page(&manual_box);
    manual_page.set_icon_name(Some("document-edit-symbolic"));

    // Status label
    let status = gtk::Label::new(Some("Scanning processes..."));
    status.add_css_class("dim-label");
    status.set_halign(Align::Start);
    content.append(&status);

    // Scan data
    let processes = scan_processes();
    let desktop_apps = scan_desktop_apps();
    let entries = build_selector_entries(&processes, &desktop_apps, already_added);

    let running_count = entries
        .iter()
        .filter(|e| e.source == EntrySource::RunningProcess)
        .count();
    let games_count = entries
        .iter()
        .filter(|e| matches!(e.source, EntrySource::DesktopApp | EntrySource::SteamGame))
        .count();
    status.set_label(&format!(
        "Found {running_count} running processes, {games_count} installed games"
    ));

    // Split entries by source
    let running_entries: Vec<SelectorEntry> = entries
        .iter()
        .filter(|e| e.source == EntrySource::RunningProcess)
        .cloned()
        .collect();
    let game_entries: Vec<SelectorEntry> = entries
        .iter()
        .filter(|e| matches!(e.source, EntrySource::DesktopApp | EntrySource::SteamGame))
        .cloned()
        .collect();

    fn make_entry_row(entry: &SelectorEntry) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        let box_ = gtk::Box::new(Orientation::Vertical, 2);
        box_.set_margin_top(6);
        box_.set_margin_bottom(6);
        box_.set_margin_start(8);
        box_.set_margin_end(8);

        let top = gtk::Box::new(Orientation::Horizontal, 8);

        // Running indicator dot
        if entry.is_running {
            let dot = gtk::Image::from_icon_name("media-playback-start-symbolic");
            dot.add_css_class("success");
            top.append(&dot);
        }

        let label = gtk::Label::new(Some(&entry.label));
        label.set_halign(Align::Start);
        label.set_hexpand(true);
        label.set_xalign(0.0);
        top.append(&label);

        // Source badge
        let badge_text = match entry.source {
            EntrySource::RunningProcess => "RUNNING",
            EntrySource::DesktopApp => "INSTALLED",
            EntrySource::SteamGame => "STEAM",
            EntrySource::Manual => "MANUAL",
        };
        let badge = gtk::Label::new(Some(badge_text));
        badge.add_css_class("tag");
        if entry.is_running {
            badge.add_css_class("success");
        } else {
            badge.add_css_class("accent");
        }
        badge.set_valign(Align::Center);
        top.append(&badge);

        let sublabel = gtk::Label::new(Some(&entry.sublabel));
        sublabel.add_css_class("dim-label");
        sublabel.add_css_class("caption");
        sublabel.set_halign(Align::Start);
        sublabel.set_xalign(0.0);

        box_.append(&top);
        box_.append(&sublabel);
        row.set_child(Some(&box_));
        row
    }

    // Filtered entry lists stored in Rc<RefCell<>> so the response handler always
    // reads the current filtered state (fixes search-filter-breaks-selection bug).
    let filtered_running: Rc<RefCell<Vec<SelectorEntry>>> =
        Rc::new(RefCell::new(running_entries.clone()));
    let filtered_games: Rc<RefCell<Vec<SelectorEntry>>> =
        Rc::new(RefCell::new(game_entries.clone()));

    // Populate running list
    for entry in &running_entries {
        let row = make_entry_row(entry);
        running_list.append(&row);
    }

    // Populate games list
    for entry in &game_entries {
        let row = make_entry_row(entry);
        games_list.append(&row);
    }

    // When a row is selected/deselected on any list, update the persistent selection set.
    let selected_execs_c = selected_executables.clone();
    running_list.connect_selected_rows_changed(clone!(
        #[strong]
        filtered_running,
        move |list| {
            let filtered = filtered_running.borrow();
            let mut sel = selected_execs_c.borrow_mut();
            // Remove all running entries from the set first
            for entry in filtered.iter() {
                sel.remove(&entry.executable);
            }
            // Re-add only the selected ones
            for row in list.selected_rows() {
                let idx = row.index() as usize;
                if let Some(entry) = filtered.get(idx) {
                    sel.insert(entry.executable.clone());
                }
            }
        }
    ));

    let selected_execs_c = selected_executables.clone();
    games_list.connect_selected_rows_changed(clone!(
        #[strong]
        filtered_games,
        move |list| {
            let filtered = filtered_games.borrow();
            let mut sel = selected_execs_c.borrow_mut();
            // Remove all game entries from the set first
            for entry in filtered.iter() {
                sel.remove(&entry.executable);
            }
            // Re-add only the selected ones
            for row in list.selected_rows() {
                let idx = row.index() as usize;
                if let Some(entry) = filtered.get(idx) {
                    sel.insert(entry.executable.clone());
                }
            }
        }
    ));

    // Search filtering: rebuilds list and updates the filtered entries
    let running_list_c = running_list.clone();
    let games_list_c = games_list.clone();
    let filtered_running_c = filtered_running.clone();
    let filtered_games_c = filtered_games.clone();

    search.connect_changed(clone!(
        #[strong]
        running_list_c,
        #[strong]
        games_list_c,
        #[strong]
        filtered_running_c,
        #[strong]
        filtered_games_c,
        move |search| {
            let query = search.text().to_string().to_lowercase();
            let filter = if query.is_empty() { None } else { Some(query) };

            let matches_entry = |entry: &SelectorEntry, q: &str| -> bool {
                entry.label.to_lowercase().contains(q)
                    || entry.executable.to_lowercase().contains(q)
                    || entry.sublabel.to_lowercase().contains(q)
            };

            // Rebuild running list
            let mut new_filtered_running = Vec::new();
            // Clear children
            while let Some(child) = running_list_c.first_child() {
                running_list_c.remove(&child);
            }
            for entry in running_entries.iter() {
                if let Some(ref q) = filter {
                    if !matches_entry(entry, q) {
                        continue;
                    }
                }
                let row = make_entry_row(entry);
                running_list_c.append(&row);
                new_filtered_running.push(entry.clone());
            }
            *filtered_running_c.borrow_mut() = new_filtered_running;

            // Rebuild games list
            let mut new_filtered_games = Vec::new();
            while let Some(child) = games_list_c.first_child() {
                games_list_c.remove(&child);
            }
            for entry in game_entries.iter() {
                if let Some(ref q) = filter {
                    if !matches_entry(entry, q) {
                        continue;
                    }
                }
                let row = make_entry_row(entry);
                games_list_c.append(&row);
                new_filtered_games.push(entry.clone());
            }
            *filtered_games_c.borrow_mut() = new_filtered_games;
        }
    ));

    // Handle response: accumulate from ALL tabs via the persistent selection set
    let selected_execs_r = selected_executables.clone();
    let manual_buf_r = manual_buf;

    dialog.connect_response(move |dialog, resp| {
        if resp == gtk::ResponseType::Ok {
            let mut selected: Vec<String>;

            {
                let sel_set = selected_execs_r.borrow();
                selected = sel_set.iter().cloned().collect();
            }

            // Also add manual text entries
            let text =
                manual_buf_r.text(&manual_buf_r.start_iter(), &manual_buf_r.end_iter(), false);
            for line in text.lines() {
                let trimmed = line.trim().to_string();
                if !trimmed.is_empty() {
                    selected.push(trimmed);
                }
            }

            if !selected.is_empty() {
                on_apply(selected);
            }
        }
        dialog.close();
    });

    dialog.show();
}
