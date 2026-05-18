// suppress dead_code warnings for future-use fields
#![allow(dead_code)]
mod config;
mod overlay;
mod process;
mod selector;

use adw::prelude::*;
use adw::Application;
use config::GameConf;
use gtk4::glib::clone;
use gtk4::{self as gtk, Align, Orientation, PolicyType};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn main() {
    let app = Application::builder()
        .application_id("com.lsfgvk.gui")
        .build();

    app.connect_activate(build_ui);
    app.run();
}

struct AppState {
    config: config::Config,
    selected_profile: Option<usize>,
    dirty: bool,
}

/// Holds references to the right-panel profile editing widgets.
struct ProfileWidgets {
    name_row: adw::EntryRow,
    active_in_label: gtk::Label,
    multiplier_row: adw::SpinRow,
    flow_row: adw::SpinRow,
    perf_switch: gtk::Switch,
    gpu_row: adw::ComboRow,
    target_fps_row: adw::SpinRow,
    gpu_model: gtk::StringList,
    dll_row: adw::EntryRow,
    fp16_switch: gtk::Switch,
}

/// Holds top-level widgets needed by multiple signal handlers.
struct AppWidgets {
    profile_listbox: gtk::ListBox,
    profile_group: adw::PreferencesGroup,
    status_label: gtk::Label,
    window: adw::ApplicationWindow,
    pw: ProfileWidgets,
}

fn build_ui(app: &Application) {
    let state = Rc::new(RefCell::new(AppState {
        config: config::load_config().unwrap_or_else(|e| {
            eprintln!("Warning: {e}");
            config::default_config()
        }),
        selected_profile: None,
        dirty: false,
    }));

    // Guard against re-entrant signal handlers while loading profile into UI.
    // load_profile_into_ui sets loading=true before touching widgets;
    // every "changed" / "notify" handler checks this first and returns early.
    let loading = Rc::new(Cell::new(false));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("lsfg-vk Configuration")
        .default_width(960)
        .default_height(640)
        .build();

    let content = gtk::Box::new(Orientation::Vertical, 0);

    // --- Header bar ---
    let header = adw::HeaderBar::new();
    content.append(&header);

    // --- Main body ---
    let body = gtk::Box::new(Orientation::Horizontal, 0);

    // --- Left panel ---
    let left_panel = gtk::Box::new(Orientation::Vertical, 8);
    left_panel.set_margin_start(12);
    left_panel.set_margin_end(12);
    left_panel.set_margin_top(12);
    left_panel.set_margin_bottom(12);
    left_panel.set_width_request(220);

    let profile_label = gtk::Label::new(Some("Profiles"));
    profile_label.add_css_class("title-4");
    left_panel.append(&profile_label);

    let profile_listbox = gtk::ListBox::new();
    profile_listbox.set_selection_mode(gtk::SelectionMode::Single);
    profile_listbox.add_css_class("navigation-sidebar");

    // Populate profiles
    {
        let s = state.borrow();
        for (i, p) in s.config.profiles.iter().enumerate() {
            let row = make_profile_row(&p.name, i);
            profile_listbox.append(&row);
        }
        let has_profiles = !s.config.profiles.is_empty();
        drop(s);
        if has_profiles {
            if let Some(row) = profile_listbox.row_at_index(0) {
                profile_listbox.select_row(Some(&row));
            }
            state.borrow_mut().selected_profile = Some(0);
        }
    }

    let scrolled = gtk::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    scrolled.set_child(Some(&profile_listbox));
    left_panel.append(&scrolled);

    let btn_box = gtk::Box::new(Orientation::Horizontal, 6);
    let btn_add = gtk::Button::with_label("Add");
    let btn_rename = gtk::Button::with_label("Rename");
    let btn_delete = gtk::Button::with_label("Delete");
    btn_delete.add_css_class("destructive-action");
    btn_box.append(&btn_add);
    btn_box.append(&btn_rename);
    btn_box.append(&btn_delete);
    left_panel.append(&btn_box);

    let separator = gtk::Separator::new(Orientation::Vertical);

    // --- Right panel ---
    let right_panel = gtk::Box::new(Orientation::Vertical, 12);
    right_panel.set_margin_start(16);
    right_panel.set_margin_end(16);
    right_panel.set_margin_top(12);
    right_panel.set_margin_bottom(12);
    right_panel.set_hexpand(true);

    let right_scrolled = gtk::ScrolledWindow::new();
    right_scrolled.set_policy(PolicyType::Never, PolicyType::Automatic);
    let settings_box = gtk::Box::new(Orientation::Vertical, 16);
    settings_box.set_vexpand(true);

    // Global Settings
    let global_group = adw::PreferencesGroup::builder()
        .title("Global Settings")
        .build();

    let dll_row = adw::EntryRow::builder()
        .title("Path to Lossless.dll (empty = auto)")
        .build();
    {
        let s = state.borrow();
        if let Some(ref dll) = s.config.global.dll {
            dll_row.set_text(dll);
        }
    }

    let fp16_row = adw::ActionRow::builder()
        .title("Allow Half-Precision (FP16)")
        .subtitle("Performance boost on AMD, may be slow on older NVIDIA")
        .build();
    let fp16_switch = gtk::Switch::new();
    fp16_switch.set_valign(Align::Center);
    {
        let s = state.borrow();
        fp16_switch.set_active(s.config.global.allow_fp16);
    }
    fp16_row.add_suffix(&fp16_switch);
    fp16_row.set_activatable_widget(Some(&fp16_switch));

    global_group.add(&dll_row);
    global_group.add(&fp16_row);

    // Profile Settings
    let profile_group = adw::PreferencesGroup::builder()
        .title("Profile Settings")
        .description("Select a profile from the list to edit")
        .build();

    let name_row = adw::EntryRow::builder().title("Profile Name").build();

    let active_in_row = adw::ActionRow::builder()
        .title("Active In")
        .subtitle("Executables that trigger this profile")
        .build();
    let active_in_label = gtk::Label::new(Some("(none)"));
    active_in_label.set_valign(Align::Center);
    active_in_label.add_css_class("dim-label");
    active_in_row.add_suffix(&active_in_label);

    // Smart Select button
    let active_in_btn = gtk::Button::with_label("Select...");
    active_in_btn.add_css_class("suggested-action");
    active_in_btn.set_valign(Align::Center);
    active_in_row.add_suffix(&active_in_btn);

    // Manual edit fallback
    let active_in_manual = gtk::Button::with_label("Edit Text...");
    active_in_manual.set_valign(Align::Center);
    active_in_row.add_suffix(&active_in_manual);

    let multiplier_row = adw::SpinRow::builder()
        .title("Multiplier")
        .subtitle("Frame generation multiplier (2-8)")
        .adjustment(&gtk::Adjustment::new(2.0, 2.0, 8.0, 1.0, 1.0, 0.0))
        .build();

    let flow_row = adw::SpinRow::builder()
        .title("Flow Scale")
        .subtitle("Motion estimation resolution (0.25 - 1.00)")
        .adjustment(&gtk::Adjustment::new(1.0, 0.25, 1.0, 0.05, 0.1, 0.0))
        .build();

    let perf_row = adw::ActionRow::builder()
        .title("Performance Mode")
        .subtitle("Use a lighter frame generation model")
        .build();
    let perf_switch = gtk::Switch::new();
    perf_switch.set_valign(Align::Center);
    perf_row.add_suffix(&perf_switch);
    perf_row.set_activatable_widget(Some(&perf_switch));

    let gpu_row = adw::ComboRow::builder()
        .title("GPU")
        .subtitle("Select GPU for frame generation")
        .build();
    let gpu_model = gtk::StringList::new(&[]);
    let gpus = config::detect_gpus();
    for g in &gpus {
        gpu_model.append(g);
    }
    gpu_row.set_model(Some(&gpu_model));

    let pacing_row = adw::ComboRow::builder()
        .title("Pacing Mode")
        .subtitle("How frames are presented")
        .model(&gtk::StringList::new(&["None"]))
        .build();

    // Custom features section
    let custom_group = adw::PreferencesGroup::builder()
        .title("Custom Features")
        .description("Your custom modifications to lsfg-vk")
        .build();

    let target_fps_row = adw::SpinRow::builder()
        .title("Target Output FPS")
        .subtitle("Cap output FPS by throttling real frames (0 = uncapped)")
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 1000.0, 10.0, 30.0, 0.0))
        .build();
    let badge = gtk::Label::new(Some("CUSTOM"));
    badge.add_css_class("tag");
    badge.add_css_class("accent");
    badge.set_valign(Align::Center);
    target_fps_row.add_suffix(&badge);
    custom_group.add(&target_fps_row);

    // Action buttons
    let action_box = gtk::Box::new(Orientation::Horizontal, 8);
    action_box.set_halign(Align::End);
    let validate_btn = gtk::Button::with_label("Validate");
    validate_btn.add_css_class("pill");
    let save_btn = gtk::Button::with_label("Save Configuration");
    save_btn.add_css_class("pill");
    save_btn.add_css_class("suggested-action");
    action_box.append(&validate_btn);
    action_box.append(&save_btn);

    let status_label = gtk::Label::new(Some("Ready"));
    status_label.add_css_class("dim-label");
    status_label.set_halign(Align::Start);

    profile_group.add(&name_row);
    profile_group.add(&active_in_row);
    profile_group.add(&multiplier_row);
    profile_group.add(&flow_row);
    profile_group.add(&perf_row);
    profile_group.add(&gpu_row);
    profile_group.add(&pacing_row);

    settings_box.append(&global_group);
    settings_box.append(&profile_group);
    settings_box.append(&custom_group);
    right_scrolled.set_child(Some(&settings_box));

    // Bottom bar with status + inline metrics
    let bottom_bar = gtk::Box::new(Orientation::Vertical, 6);
    bottom_bar.set_margin_top(4);

    // Inline status bar (layer status + active profiles)
    let (inline_status, _layer_lbl, _active_lbl, monitor_btn) = overlay::build_inline_status();

    let btn_row = gtk::Box::new(Orientation::Horizontal, 12);
    btn_row.append(&status_label);
    btn_row.append(&action_box);

    bottom_bar.append(&inline_status);
    bottom_bar.append(&btn_row);

    right_panel.append(&right_scrolled);
    right_panel.append(&bottom_bar);

    body.append(&left_panel);
    body.append(&separator);
    body.append(&right_panel);
    content.append(&body);

    window.set_content(Some(&content));

    // Bundle widgets
    let pw = ProfileWidgets {
        name_row,
        active_in_label,
        multiplier_row,
        flow_row,
        perf_switch,
        gpu_row,
        target_fps_row,
        gpu_model,
        dll_row,
        fp16_switch,
    };

    let widgets = Rc::new(AppWidgets {
        profile_listbox,
        profile_group,
        status_label,
        window: window.clone(),
        pw,
    });

    // --- Overlay window ---
    let overlay_state = Rc::new(RefCell::new(overlay::create_overlay()));

    {
        let overlay_state = overlay_state.clone();
        monitor_btn.connect_toggled(move |btn| {
            let overlay = overlay_state.borrow();
            if btn.is_active() {
                overlay.window.present();
            } else {
                overlay.window.hide();
            }
        });
    }

    // Sync overlay hidden state back to toggle button
    {
        let monitor_btn = monitor_btn.clone();
        overlay_state.borrow().window.connect_hide(move |_| {
            monitor_btn.set_active(false);
        });
    }

    // --- Initial load ---
    load_profile_into_ui(&state, &widgets, &loading);

    // ====================== Signal handlers ======================

    // Profile selection
    widgets.profile_listbox.connect_row_selected(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                state.borrow_mut().selected_profile = Some(idx);
            }
            load_profile_into_ui(&state, &widgets, &loading);
        }
    ));

    // Add profile
    btn_add.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_| {
            let mut s = state.borrow_mut();
            let name = format!("New Profile {}", s.config.profiles.len() + 1);
            s.config.profiles.push(GameConf::default());
            let idx = s.config.profiles.len() - 1;
            s.config.profiles[idx].name = name.clone();
            s.dirty = true;
            drop(s);

            let row = make_profile_row(&name, idx);
            widgets.profile_listbox.append(&row);
            widgets.profile_listbox.select_row(Some(&row));

            load_profile_into_ui(&state, &widgets, &loading);
        }
    ));

    // Delete profile (with confirmation)
    btn_delete.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_| {
            let profile_name = {
                let s = state.borrow();
                s.selected_profile
                    .and_then(|idx| s.config.profiles.get(idx))
                    .map(|p| p.name.clone())
            };
            let Some(profile_name) = profile_name else {
                return;
            };

            let dialog = gtk::MessageDialog::new(
                Some(&widgets.window),
                gtk::DialogFlags::MODAL,
                gtk::MessageType::Question,
                gtk::ButtonsType::YesNo,
                format!("Delete profile \"{profile_name}\"?"),
            );
            dialog.set_property(
                "secondary-text",
                format!(
                    "This will remove the profile \"{profile_name}\". You still need to save to apply."
                ),
            );
            let state_c = state.clone();
            let widgets_c = widgets.clone();
            let loading_c = loading.clone();
            dialog.connect_response(move |dialog, resp| {
                if resp == gtk::ResponseType::Yes {
                    {
                        let mut s = state_c.borrow_mut();
                        if let Some(idx) = s.selected_profile {
                            if idx < s.config.profiles.len() {
                                s.config.profiles.remove(idx);
                                s.dirty = true;
                            }
                        }
                    }
                    rebuild_listbox(&widgets_c.profile_listbox, &state_c);
                    if let Some(row) = widgets_c.profile_listbox.row_at_index(0) {
                        widgets_c.profile_listbox.select_row(Some(&row));
                    } else {
                        state_c.borrow_mut().selected_profile = None;
                    }
                    load_profile_into_ui(&state_c, &widgets_c, &loading_c);
                }
                dialog.close();
            });
            dialog.show();
        }
    ));

    // Rename
    btn_rename.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        move |_| {
            let mut s = state.borrow_mut();
            if let Some(idx) = s.selected_profile {
                let new_name = widgets.pw.name_row.text().to_string();
                if !new_name.trim().is_empty() && idx < s.config.profiles.len() {
                    s.config.profiles[idx].name = new_name.clone();
                    s.dirty = true;
                    drop(s);
                    if let Some(row) = widgets.profile_listbox.row_at_index(idx as i32) {
                        update_row_label(&row, &new_name);
                    }
                }
            }
        }
    ));

    // Spin rows: auto-save on value change
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.multiplier_row.connect_output(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            #[strong]
            loading_c,
            move |_| {
                if loading_c.get() {
                    return false;
                }
                save_profile_from_ui(&state_c, &widgets_c);
                false
            }
        ));
    }
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.flow_row.connect_output(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            #[strong]
            loading_c,
            move |_| {
                if loading_c.get() {
                    return false;
                }
                save_profile_from_ui(&state_c, &widgets_c);
                false
            }
        ));
    }
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.target_fps_row.connect_output(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            #[strong]
            loading_c,
            move |_| {
                if loading_c.get() {
                    return false;
                }
                save_profile_from_ui(&state_c, &widgets_c);
                false
            }
        ));
    }

    // Performance mode switch
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.perf_switch.connect_active_notify(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            #[strong]
            loading_c,
            move |_| {
                if loading_c.get() {
                    return;
                }
                save_profile_from_ui(&state_c, &widgets_c);
            }
        ));
    }

    // GPU combo row
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.gpu_row.connect_selected_item_notify(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            #[strong]
            loading_c,
            move |_| {
                if loading_c.get() {
                    return;
                }
                save_profile_from_ui(&state_c, &widgets_c);
            }
        ));
    }

    // Name row changed
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.name_row.connect_changed(move |_| {
            if loading_c.get() {
                return;
            }
            save_profile_from_ui(&state_c, &widgets_c);
        });
    }

    // Global: DLL path
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let dll_row_ref = widgets.pw.dll_row.clone();
        widgets.pw.dll_row.connect_changed(move |_| {
            if loading_c.get() {
                return;
            }
            let text = dll_row_ref.text().to_string();
            let mut s = state_c.borrow_mut();
            if text.trim().is_empty() {
                s.config.global.dll = None;
            } else {
                s.config.global.dll = Some(text);
            }
            s.dirty = true;
        });
    }

    // Global: FP16 switch
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let fp16_ref = widgets.pw.fp16_switch.clone();
        widgets.pw.fp16_switch.connect_active_notify(move |_| {
            if loading_c.get() {
                return;
            }
            let active = fp16_ref.is_active();
            state_c.borrow_mut().config.global.allow_fp16 = active;
            state_c.borrow_mut().dirty = true;
        });
    }

    // Active In: Smart Selector button
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let widgets_c = widgets.clone();
        let window_ref = window.clone();
        active_in_btn.connect_clicked(move |_| {
            let already_added = {
                let s = state_c.borrow();
                s.selected_profile
                    .and_then(|idx| s.config.profiles.get(idx))
                    .map(|p| p.active_in_list())
                    .unwrap_or_default()
            };
            let state_cc = state_c.clone();
            let loading_cc = loading_c.clone();
            let widgets_cc = widgets_c.clone();
            selector::show_selector_dialog(
                Some(&window_ref),
                &already_added,
                Box::new(move |selected: Vec<String>| {
                    let mut s = state_cc.borrow_mut();
                    if let Some(idx) = s.selected_profile {
                        if let Some(profile) = s.config.profiles.get_mut(idx) {
                            let mut current = profile.active_in_list();
                            for name in selected {
                                if !current.contains(&name) {
                                    current.push(name);
                                }
                            }
                            profile.set_active_in(current);
                            s.dirty = true;
                        }
                    }
                    drop(s);
                    load_profile_into_ui(&state_cc, &widgets_cc, &loading_cc);
                }),
            );
        });
    }

    // Active In: Manual edit button
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let widgets_c = widgets.clone();
        let window_ref = window.clone();
        active_in_manual.connect_clicked(move |_| {
            let current = {
                let s = state_c.borrow();
                s.selected_profile
                    .and_then(|idx| s.config.profiles.get(idx))
                    .map(|p| p.active_in_list().join(", "))
                    .unwrap_or_default()
            };

            let dialog = gtk::Dialog::with_buttons(
                Some("Edit Active In"),
                Some(&window_ref),
                gtk::DialogFlags::MODAL,
                &[
                    ("Cancel", gtk::ResponseType::Cancel),
                    ("Apply", gtk::ResponseType::Ok),
                ],
            );
            dialog.set_default_size(400, 200);

            let content_area = dialog.content_area();
            content_area.set_spacing(8);
            content_area.set_margin_start(12);
            content_area.set_margin_end(12);
            content_area.set_margin_top(12);
            content_area.set_margin_bottom(12);

            let label = gtk::Label::new(Some("Enter executable names (comma or space separated):"));
            label.set_halign(Align::Start);
            content_area.append(&label);

            let entry = gtk::Entry::new();
            entry.set_text(&current);
            entry.set_hexpand(true);
            content_area.append(&entry);

            let state_cc = state_c.clone();
            let loading_cc = loading_c.clone();
            let widgets_cc = widgets_c.clone();
            dialog.connect_response(move |dialog, resp| {
                if resp == gtk::ResponseType::Ok {
                    let text = entry.text().to_string();
                    let names: Vec<String> = text
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let mut s = state_cc.borrow_mut();
                    if let Some(idx) = s.selected_profile {
                        if let Some(profile) = s.config.profiles.get_mut(idx) {
                            profile.set_active_in(names);
                            s.dirty = true;
                        }
                    }
                    drop(s);
                    load_profile_into_ui(&state_cc, &widgets_cc, &loading_cc);
                }
                dialog.close();
            });
            dialog.show();
        });
    }

    // Validate button
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        validate_btn.connect_clicked(move |_| {
            let s = state_c.borrow();
            match config::validate_config(&s.config) {
                Ok(()) => {
                    widgets_c.status_label.set_text("Configuration is valid");
                    widgets_c.status_label.remove_css_class("error");
                }
                Err(errors) => {
                    widgets_c.status_label.set_text(&errors.join("; "));
                    widgets_c.status_label.add_css_class("error");
                }
            }
        });
    }

    // Save button
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        save_btn.connect_clicked(move |_| {
            let s = state_c.borrow();
            match config::save_config(&s.config) {
                Ok(()) => {
                    widgets_c.status_label.set_text("Configuration saved");
                    widgets_c.status_label.remove_css_class("error");
                    drop(s);
                    state_c.borrow_mut().dirty = false;
                }
                Err(e) => {
                    widgets_c
                        .status_label
                        .set_text(&format!("Save failed: {e}"));
                    widgets_c.status_label.add_css_class("error");
                }
            }
        });
    }

    window.present();
}

/// Load the currently selected profile's values into the right-panel widgets.
///
/// Sets `loading = true` for the entire duration so that the synchronous
/// "changed" / "notify" signals fired by `set_text`, `set_value`, etc.
/// are ignored by their handlers (avoiding a RefCell double-borrow panic).
fn load_profile_into_ui(
    state: &Rc<RefCell<AppState>>,
    widgets: &Rc<AppWidgets>,
    loading: &Rc<Cell<bool>>,
) {
    loading.set(true);

    let s = state.borrow();
    let pw = &widgets.pw;

    if let Some(idx) = s.selected_profile {
        if let Some(profile) = s.config.profiles.get(idx) {
            pw.name_row.set_text(&profile.name);
            pw.multiplier_row.set_value(profile.multiplier as f64);
            pw.flow_row.set_value(profile.flow_scale as f64);
            pw.perf_switch.set_active(profile.performance_mode);
            pw.target_fps_row
                .set_value(profile.target_fps.unwrap_or(0) as f64);

            // GPU selection: index 0 = "Default" (None), else match by name
            let mut gpu_selected: u32 = 0;
            if let Some(ref gpu_name) = profile.gpu {
                let n = pw.gpu_model.n_items();
                for i in 0..n {
                    if let Some(s) = pw.gpu_model.string(i) {
                        if s.as_str() == gpu_name.as_str() {
                            gpu_selected = i;
                            break;
                        }
                    }
                }
            }
            pw.gpu_row.set_selected(gpu_selected);

            // Active in label
            let list = profile.active_in_list();
            if list.is_empty() {
                pw.active_in_label.set_text("(none)");
            } else {
                pw.active_in_label.set_text(&list.join(", "));
            }

            widgets.profile_group.set_description(None);
        }
    } else {
        // No profile selected – clear / reset all fields
        pw.name_row.set_text("");
        pw.multiplier_row.set_value(2.0);
        pw.flow_row.set_value(1.0);
        pw.perf_switch.set_active(false);
        pw.target_fps_row.set_value(0.0);
        pw.gpu_row.set_selected(0);
        pw.active_in_label.set_text("(none)");
        widgets
            .profile_group
            .set_description(Some("Select a profile from the list to edit"));
    }

    loading.set(false);
}

/// Read the current widget values and write them back into the selected profile
/// in `state`.  Called from every per-profile "changed" signal handler.
fn save_profile_from_ui(state: &Rc<RefCell<AppState>>, widgets: &Rc<AppWidgets>) {
    // Read all widget values first (no RefCell borrow needed for widget access)
    let name = widgets.pw.name_row.text().to_string();
    let multiplier = widgets.pw.multiplier_row.value() as u32;
    let flow_scale = widgets.pw.flow_row.value() as f32;
    let performance_mode = widgets.pw.perf_switch.is_active();
    let target_fps_val = widgets.pw.target_fps_row.value() as u32;
    let target_fps = if target_fps_val == 0 {
        None
    } else {
        Some(target_fps_val)
    };
    let gpu_selected = widgets.pw.gpu_row.selected();
    let gpu_name: Option<String> = if gpu_selected == 0 {
        None
    } else {
        widgets
            .pw
            .gpu_model
            .string(gpu_selected)
            .map(|s| s.to_string())
    };

    // Borrow state mutably and apply
    let mut s = state.borrow_mut();
    let Some(idx) = s.selected_profile else {
        return;
    };
    let Some(profile) = s.config.profiles.get_mut(idx) else {
        return;
    };

    let name_changed = profile.name != name;

    profile.name = name;
    profile.multiplier = multiplier;
    profile.flow_scale = flow_scale;
    profile.performance_mode = performance_mode;
    profile.target_fps = target_fps;
    profile.gpu = gpu_name;
    s.dirty = true;

    drop(s);

    // Keep listbox label in sync when the user types a new name
    if name_changed {
        if let Some(row) = widgets.profile_listbox.row_at_index(idx as i32) {
            update_row_label(&row, widgets.pw.name_row.text().as_str());
        }
    }
}

/// Create a single profile row for the left-panel listbox.
fn make_profile_row(name: &str, _index: usize) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let box_ = gtk::Box::new(Orientation::Horizontal, 8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(6);
    box_.set_margin_bottom(6);

    let icon = gtk::Image::from_icon_name("applications-games-symbolic");
    icon.set_pixel_size(20);
    box_.append(&icon);

    let label = gtk::Label::new(Some(name));
    label.set_halign(Align::Start);
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    box_.append(&label);

    row.set_child(Some(&box_));
    row
}

/// Remove all children from the listbox and repopulate from state.
fn rebuild_listbox(listbox: &gtk::ListBox, state: &Rc<RefCell<AppState>>) {
    // Remove all children
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }
    // Re-add all profiles
    let s = state.borrow();
    for (i, p) in s.config.profiles.iter().enumerate() {
        let row = make_profile_row(&p.name, i);
        listbox.append(&row);
    }
}

/// Update the text label inside an existing listbox row.
fn update_row_label(row: &gtk::ListBoxRow, name: &str) {
    let child = row.child();
    if let Some(box_) = child.and_then(|c| c.downcast::<gtk::Box>().ok()) {
        // The label is the second child (after the icon)
        let mut found = false;
        let mut child = box_.first_child();
        while let Some(c) = child {
            if let Ok(label) = c.clone().downcast::<gtk::Label>() {
                label.set_text(name);
                found = true;
                break;
            }
            child = c.next_sibling();
        }
        if !found {
            // Fallback: just set the row's accessible description
        }
    }
}
