#![allow(dead_code)]
mod config;
mod helpers;
mod overlay;
mod process;
mod selector;

use adw::prelude::*;
use adw::Application;
use gtk4::glib::clone;
use gtk4::{self as gtk, Align, Orientation, PolicyType};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Load the bundled custom CSS theme.
fn load_custom_css() {
    let provider = gtk::CssProvider::new();
    // Try loading from the executable's directory first, then from project root
    let css_paths = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("style.css"))),
        Some(std::path::PathBuf::from("style.css")),
    ];
    for path in css_paths.iter().flatten() {
        if path.exists() {
            provider.load_from_file(&gtk::gio::File::for_path(path));
            gtk::style_context_add_provider_for_display(
                &gtk::gdk::Display::default().unwrap(),
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            return;
        }
    }
    // Fallback: embed from project root (works during development)
    let css = include_str!("../style.css");
    provider.load_from_data(css);
    gtk::style_context_add_provider_for_display(
        &gtk::gdk::Display::default().unwrap(),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn main() {
    let app = Application::builder()
        .application_id("com.lsfgvk.gui")
        .build();

    app.connect_activate(|app| {
        load_custom_css();
        build_ui(app);
    });
    app.run();
}

struct AppState {
    config: config::Config,
    selected_profile: Option<usize>,
    dirty: bool,
    last_dirty_time: Option<std::time::Instant>,
}

struct ProfileWidgets {
    name_row: adw::EntryRow,
    multiplier_row: adw::SpinRow,
    flow_row: adw::SpinRow,
    perf_switch: gtk::Switch,
    gpu_row: adw::ComboRow,
    target_fps_row: adw::SpinRow,
    gpu_model: gtk::StringList,
    dll_row: adw::EntryRow,
    fp16_switch: gtk::Switch,
    active_in_editor: selector::ActiveInEditor,
}

struct AppWidgets {
    profile_listbox: gtk::ListBox,
    profile_group: adw::PreferencesGroup,
    custom_group: adw::PreferencesGroup,
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
        last_dirty_time: None,
    }));

    let loading = Rc::new(Cell::new(false));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("lsfg-vk Configuration")
        .default_width(960)
        .default_height(640)
        .build();

    // --- Shortcuts controller ---
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.set_scope(gtk::ShortcutScope::Global);

    // Ctrl+S = Save (flag-based, actual save done in periodic callback below)
    let save_requested = Rc::new(Cell::new(false));
    let save_action = gtk::CallbackAction::new({
        let save_requested = save_requested.clone();
        move |_widget, _| {
            save_requested.set(true);
            gtk::glib::Propagation::Proceed
        }
    });
    let save_shortcut = gtk::Shortcut::new(
        Some(gtk::ShortcutTrigger::parse_string("<Control>s").unwrap()),
        Some(save_action),
    );
    shortcuts.add_shortcut(save_shortcut);

    // Ctrl+N = New profile
    // (wired after widgets are created below via a flag)
    let new_profile_requested = Rc::new(Cell::new(false));
    let new_action = gtk::CallbackAction::new({
        let new_profile_requested = new_profile_requested.clone();
        move |_widget, _| {
            new_profile_requested.set(true);
            gtk::glib::Propagation::Proceed
        }
    });
    let new_shortcut = gtk::Shortcut::new(
        Some(gtk::ShortcutTrigger::parse_string("<Control>n").unwrap()),
        Some(new_action),
    );
    shortcuts.add_shortcut(new_shortcut);

    let content = gtk::Box::new(Orientation::Vertical, 0);
    let header = adw::HeaderBar::new();

    // Menu button with import/export
    let menu = gtk::MenuButton::new();
    menu.set_icon_name("open-menu-symbolic");
    menu.set_tooltip_text(Some("Menu"));
    let menu_model = gio::Menu::new();
    menu_model.append(Some("Export Profiles..."), Some("win.export"));
    menu_model.append(Some("Import Profiles..."), Some("win.import"));
    menu_model.append(Some("Setup Proton Layer"), Some("win.setup-proton"));

    // About section in menu
    let about_section = gio::Menu::new();
    about_section.append(Some("About lsfg-vk GUI"), Some("win.about"));
    menu_model.append_section(None, &about_section);
    menu.set_menu_model(Some(&menu_model));
    header.pack_end(&menu);

    content.append(&header);

    // --- Main body ---
    let body = gtk::Box::new(Orientation::Horizontal, 0);

    // --- Left panel ---
    let left_panel = gtk::Box::new(Orientation::Vertical, 8);
    left_panel.set_margin_start(12);
    left_panel.set_margin_end(12);
    left_panel.set_margin_top(12);
    left_panel.set_margin_bottom(12);
    left_panel.set_width_request(240);

    let profile_label = gtk::Label::new(Some("Profiles"));
    profile_label.add_css_class("title-4");
    left_panel.append(&profile_label);

    // Profile search/filter
    let profile_search = gtk::SearchEntry::new();
    profile_search.set_placeholder_text(Some("Filter profiles..."));
    profile_search.set_hexpand(true);
    left_panel.append(&profile_search);

    let profile_listbox = gtk::ListBox::new();
    profile_listbox.set_selection_mode(gtk::SelectionMode::Single);
    profile_listbox.add_css_class("navigation-sidebar");

    // Populate profiles
    {
        let s = state.borrow();
        for p in s.config.profiles.iter() {
            let row = make_profile_row(&p.name, p.multiplier, p.active_in_list().len());
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

    let btn_box = gtk::Box::new(Orientation::Horizontal, 4);
    let btn_add = gtk::Button::with_label("Add");
    btn_add.set_hexpand(true);
    let btn_dup = gtk::Button::with_label("Duplicate");
    btn_dup.set_hexpand(true);
    let btn_delete = gtk::Button::with_label("Delete");
    btn_delete.add_css_class("destructive-action");
    btn_delete.set_hexpand(true);

    let btn_up = gtk::Button::from_icon_name("go-up-symbolic");
    btn_up.set_tooltip_text(Some("Move Up"));
    let btn_down = gtk::Button::from_icon_name("go-down-symbolic");
    btn_down.set_tooltip_text(Some("Move Down"));

    btn_box.append(&btn_add);
    btn_box.append(&btn_dup);
    btn_box.append(&btn_delete);
    btn_box.append(&btn_up);
    btn_box.append(&btn_down);
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

    // Global enable/disable toggle
    let enabled_row = adw::ActionRow::builder()
        .title("Enable Frame Generation Layer")
        .subtitle("When disabled, the Vulkan layer will not load")
        .build();
    let enabled_switch = gtk::Switch::new();
    enabled_switch.set_valign(Align::Center);
    {
        let s = state.borrow();
        enabled_switch.set_active(s.config.global.enabled);
    }
    enabled_row.add_suffix(&enabled_switch);
    enabled_row.set_activatable_widget(Some(&enabled_switch));
    global_group.add(&enabled_row);

    // Profile Settings
    let profile_group = adw::PreferencesGroup::builder()
        .title("Profile Settings")
        .description("Select a profile from the list to edit")
        .build();

    let name_row = adw::EntryRow::builder().title("Profile Name").build();

    // Active In: chip editor instead of dialog
    let active_in_row = adw::ActionRow::builder()
        .title("Active In")
        .subtitle("Executables that trigger this profile")
        .build();

    // Build the active_in editor with callback
    let state_for_editor = state.clone();
    let loading_for_editor = loading.clone();
    let editor = selector::ActiveInEditor::new(Box::new(move |entries: Vec<String>| {
        if loading_for_editor.get() {
            return;
        }
        let mut s = state_for_editor.borrow_mut();
        if let Some(idx) = s.selected_profile {
            if let Some(profile) = s.config.profiles.get_mut(idx) {
                profile.set_active_in(entries);
                s.dirty = true;
                s.last_dirty_time = Some(std::time::Instant::now());
            }
        }
    }));
    active_in_row.add_suffix(editor.widget());

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

    // Custom features
    let custom_group = adw::PreferencesGroup::builder()
        .title("Custom Features")
        .build();

    let target_fps_row = adw::SpinRow::builder()
        .title("Target Output FPS")
        .subtitle("Cap output FPS (0 = uncapped)")
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 1000.0, 10.0, 30.0, 0.0))
        .build();
    custom_group.add(&target_fps_row);

    // Proton status row
    let proton_row = adw::ActionRow::builder()
        .title("Proton Compatibility")
        .subtitle("Checks if the layer is set up for Steam Proton games")
        .build();
    let proton_status = gtk::Label::new(Some("Checking..."));
    proton_status.set_valign(Align::Center);
    proton_row.add_suffix(&proton_status);
    check_proton_status(&proton_status);

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

    settings_box.append(&global_group);
    settings_box.append(&profile_group);
    settings_box.append(&custom_group);
    settings_box.append(&proton_row);
    right_scrolled.set_child(Some(&settings_box));

    // Bottom bar
    let bottom_bar = gtk::Box::new(Orientation::Vertical, 6);
    bottom_bar.set_margin_top(4);

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

    // Wrap in ToastOverlay for notifications
    let toast_overlay = adw::ToastOverlay::new();
    toast_overlay.set_child(Some(&content));

    window.set_content(Some(&toast_overlay));

    // Bundle widgets
    let pw = ProfileWidgets {
        name_row,
        multiplier_row,
        flow_row,
        perf_switch,
        gpu_row,
        target_fps_row,
        gpu_model,
        dll_row,
        fp16_switch,
        active_in_editor: editor,
    };

    let widgets = Rc::new(AppWidgets {
        profile_listbox,
        profile_group,
        custom_group,
        status_label,
        window: window.clone(),
        pw,
    });

    // Add shortcuts controller to window (clone for later Ctrl+F registration)
    let shortcuts_ref = shortcuts.clone();
    window.add_controller(shortcuts);

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
            s.config.profiles.push(config::GameConf::default());
            let idx = s.config.profiles.len() - 1;
            s.config.profiles[idx].name = name.clone();
            s.dirty = true;
            drop(s);

            let row = make_profile_row(&name, 2, 0);
            widgets.profile_listbox.append(&row);
            widgets.profile_listbox.select_row(Some(&row));
            load_profile_into_ui(&state, &widgets, &loading);
            update_dirty_title(&state, &widgets);
        }
    ));

    // Duplicate profile
    btn_dup.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_| {
            let mut s = state.borrow_mut();
            if let Some(idx) = s.selected_profile {
                if let Some(profile) = s.config.profiles.get(idx).cloned() {
                    let mut dup = profile.clone();
                    dup.name = format!("{} (copy)", dup.name);
                    s.config.profiles.push(dup);
                    s.dirty = true;
                    s.last_dirty_time = Some(std::time::Instant::now());
                    drop(s);

                    let last_idx = state.borrow().config.profiles.len() - 1;
                    let p = state.borrow().config.profiles[last_idx].clone();
                    let row = make_profile_row(
                        &p.name,
                        p.multiplier,
                        p.active_in_list().len(),
                    );
                    widgets.profile_listbox.append(&row);
                    widgets.profile_listbox.select_row(Some(&row));
                    load_profile_into_ui(&state, &widgets, &loading);
                    update_dirty_title(&state, &widgets);
                }
            }
        }
    ));

    // Delete profile
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
                &format!(
                    "Delete profile \"{profile_name}\"?\n\nThis will remove the profile. Save to apply."
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
                                s.last_dirty_time = Some(std::time::Instant::now());
                            }
                        }
                    }
                    rebuild_profile_list(&state_c, &widgets_c);
                    if let Some(row) = widgets_c.profile_listbox.row_at_index(0) {
                        widgets_c.profile_listbox.select_row(Some(&row));
                    } else {
                        state_c.borrow_mut().selected_profile = None;
                    }
                    load_profile_into_ui(&state_c, &widgets_c, &loading_c);
                    update_dirty_title(&state_c, &widgets_c);
                }
                dialog.close();
            });
            dialog.show();
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
                update_dirty_title(&state_c, &widgets_c);
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
                update_dirty_title(&state_c, &widgets_c);
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
                update_dirty_title(&state_c, &widgets_c);
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
                update_dirty_title(&state_c, &widgets_c);
            }
        ));
    }

    // GPU combo
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
                update_dirty_title(&state_c, &widgets_c);
            }
        ));
    }

    // Name row changed
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        widgets.pw.name_row.connect_changed(clone!(
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
                update_dirty_title(&state_c, &widgets_c);
            }
        ));
    }

    // Move profile up
    btn_up.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_| {
            let mut s = state.borrow_mut();
            if let Some(idx) = s.selected_profile {
                if idx > 0 {
                    s.config.profiles.swap(idx, idx - 1);
                    s.dirty = true;
                    s.last_dirty_time = Some(std::time::Instant::now());
                    let new_idx = idx - 1;
                    s.selected_profile = Some(new_idx);
                    drop(s);
                    rebuild_profile_list(&state, &widgets);
                    if let Some(row) = widgets.profile_listbox.row_at_index(new_idx as i32) {
                        widgets.profile_listbox.select_row(Some(&row));
                    }
                    load_profile_into_ui(&state, &widgets, &loading);
                    update_dirty_title(&state, &widgets);
                }
            }
        }
    ));

    // Move profile down
    btn_down.connect_clicked(clone!(
        #[strong]
        state,
        #[strong]
        widgets,
        #[strong]
        loading,
        move |_| {
            let mut s = state.borrow_mut();
            if let Some(idx) = s.selected_profile {
                if idx + 1 < s.config.profiles.len() {
                    s.config.profiles.swap(idx, idx + 1);
                    s.dirty = true;
                    s.last_dirty_time = Some(std::time::Instant::now());
                    let new_idx = idx + 1;
                    s.selected_profile = Some(new_idx);
                    drop(s);
                    rebuild_profile_list(&state, &widgets);
                    if let Some(row) = widgets.profile_listbox.row_at_index(new_idx as i32) {
                        widgets.profile_listbox.select_row(Some(&row));
                    }
                    load_profile_into_ui(&state, &widgets, &loading);
                    update_dirty_title(&state, &widgets);
                }
            }
        }
    ));

    // Global: DLL path
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let dll_row_ref = widgets.pw.dll_row.clone();
        widgets.pw.dll_row.connect_changed(clone!(
            #[strong]
            state_c,
            #[strong]
            loading_c,
            #[strong]
            dll_row_ref,
            move |_| {
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
                s.last_dirty_time = Some(std::time::Instant::now());
                drop(s);
            }
        ));
    }

    // Global: FP16
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let fp16_ref = widgets.pw.fp16_switch.clone();
        widgets.pw.fp16_switch.connect_active_notify(clone!(
            #[strong]
            state_c,
            #[strong]
            loading_c,
            #[strong]
            fp16_ref,
            move |_| {
                if loading_c.get() {
                    return;
                }
                let active = fp16_ref.is_active();
                state_c.borrow_mut().config.global.allow_fp16 = active;
                state_c.borrow_mut().dirty = true;
                state_c.borrow_mut().last_dirty_time = Some(std::time::Instant::now());
            }
        ));
    }

    // Global: Enable/Disable
    {
        let state_c = state.clone();
        let loading_c = loading.clone();
        let enabled_ref = enabled_switch.clone();
        enabled_switch.connect_active_notify(clone!(
            #[strong]
            state_c,
            #[strong]
            loading_c,
            #[strong]
            enabled_ref,
            move |_| {
                if loading_c.get() {
                    return;
                }
                let active = enabled_ref.is_active();
                state_c.borrow_mut().config.global.enabled = active;
                state_c.borrow_mut().dirty = true;
                state_c.borrow_mut().last_dirty_time = Some(std::time::Instant::now());
            }
        ));
    }

    // Validate button
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        validate_btn.connect_clicked(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            move |_| {
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
            }
        ));
    }

    // Save button
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        save_btn.connect_clicked(clone!(
            #[strong]
            state_c,
            #[strong]
            widgets_c,
            move |_| {
                let s = state_c.borrow();
                match config::save_config(&s.config) {
                    Ok(()) => {
                        widgets_c.status_label.set_text("Configuration saved");
                        widgets_c.status_label.remove_css_class("error");
                        drop(s);
                        state_c.borrow_mut().dirty = false;
                        update_dirty_title(&state_c, &widgets_c);
                    }
                    Err(e) => {
                        widgets_c.status_label.set_text(&format!("Save failed: {e}"));
                        widgets_c.status_label.add_css_class("error");
                    }
                }
            }
        ));
    }

    // --- Import / Export / Proton actions ---
    let export_action = gio::SimpleAction::new("export", None);
    let import_action = gio::SimpleAction::new("import", None);
    let setup_proton_action = gio::SimpleAction::new("setup-proton", None);

    // Export profiles
    {
        let state_c = state.clone();
        let toast_overlay = toast_overlay.clone();
        let window_c = window.clone();
        export_action.connect_activate(move |_, _| {
            let dialog = gtk::FileChooserDialog::builder()
                .title("Export Profiles")
                .action(gtk::FileChooserAction::Save)
                .modal(true)
                .build();
            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
            dialog.add_button("Export", gtk::ResponseType::Accept);
            dialog.set_current_name("lsfg-vk-profiles.json");

            let state_c2 = state_c.clone();
            let toast_overlay_c = toast_overlay.clone();
            dialog.connect_response(move |dialog, resp| {
                if resp == gtk::ResponseType::Accept {
                    if let Some(file) = dialog.file() {
                        let path = file.path().unwrap_or_default();
                        let s = state_c2.borrow();
                        match helpers::export_profiles(&s.config.profiles, &path) {
                            Ok(()) => helpers::show_toast(
                                &toast_overlay_c,
                                &format!("Exported {} profiles", s.config.profiles.len()),
                                3,
                            ),
                            Err(e) => helpers::show_toast(
                                &toast_overlay_c,
                                &format!("Export failed: {e}"),
                                5,
                            ),
                        }
                    }
                }
                dialog.close();
            });
            dialog.set_transient_for(Some(&window_c));
            dialog.show();
        });
    }

    // Import profiles
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        let toast_overlay = toast_overlay.clone();
        let window_c = window.clone();
        import_action.connect_activate(move |_, _| {
            let dialog = gtk::FileChooserDialog::builder()
                .title("Import Profiles")
                .action(gtk::FileChooserAction::Open)
                .modal(true)
                .build();
            dialog.add_button("Cancel", gtk::ResponseType::Cancel);
            dialog.add_button("Import", gtk::ResponseType::Accept);

            let state_c2 = state_c.clone();
            let widgets_c2 = widgets_c.clone();
            let loading_c2 = loading_c.clone();
            let toast_overlay_c = toast_overlay.clone();
            dialog.connect_response(move |dialog, resp| {
                if resp == gtk::ResponseType::Accept {
                    if let Some(file) = dialog.file() {
                        let path = file.path().unwrap_or_default();
                        match helpers::import_profiles(&path) {
                            Ok(imported) => {
                                let count = imported.len();
                                let mut s = state_c2.borrow_mut();
                                s.config.profiles.extend(imported);
                                s.dirty = true;
                                s.last_dirty_time = Some(std::time::Instant::now());
                                drop(s);
                                rebuild_profile_list(&state_c2, &widgets_c2);
                                helpers::show_toast(
                                    &toast_overlay_c,
                                    &format!("Imported {count} profiles"),
                                    3,
                                );
                                load_profile_into_ui(&state_c2, &widgets_c2, &loading_c2);
                            }
                            Err(e) => {
                                helpers::show_toast(
                                    &toast_overlay_c,
                                    &format!("Import failed: {e}"),
                                    5,
                                );
                            }
                        }
                    }
                }
                dialog.close();
            });
            dialog.set_transient_for(Some(&window_c));
            dialog.show();
        });
    }

    // Setup Proton
    {
        let toast_overlay = toast_overlay.clone();
        setup_proton_action.connect_activate(move |_, _| match helpers::setup_proton() {
            Ok(count) => helpers::show_toast(
                &toast_overlay,
                &format!("Proton layer set up ({count} installs patched)"),
                4,
            ),
            Err(e) => helpers::show_toast(&toast_overlay, &format!("Setup failed: {e}"), 5),
        });
    }

    window.add_action(&export_action);
    window.add_action(&import_action);
    window.add_action(&setup_proton_action);

    // About dialog
    let about_action = gio::SimpleAction::new("about", None);
    {
        let window_c = window.clone();
        about_action.connect_activate(move |_, _| {
            let dialog = adw::AboutWindow::builder()
                .application_name("lsfg-vk GUI")
                .application_icon("applications-games-symbolic")
                .version(env!("CARGO_PKG_VERSION"))
                .comments("A GTK4/libadwaita GUI for configuring lsfg-vk frame generation")
                .website("https://github.com/Legobrah/lsfg-vk")
                .license_type(gtk::License::MitX11)
                .developers(vec!["devind"])
                .build();
            dialog.set_transient_for(Some(&window_c));
            dialog.present();
        });
    }
    window.add_action(&about_action);

    // --- Profile search filter ---
    {
        let listbox = widgets.profile_listbox.clone();
        profile_search.connect_changed(move |search| {
            let query = search.text().to_lowercase();
            let n = listbox.observe_children().n_items();
            for i in 0..n {
                if let Some(row) = listbox.row_at_index(i as i32) {
                    let visible = if query.is_empty() {
                        true
                    } else if let Some(child) = row.child() {
                        // Walk into the hbox -> find the vbox (Box child) -> first label
                        let mut label_text = String::new();
                        if let Ok(hbox) = child.clone().downcast::<gtk::Box>() {
                            let mut iter = hbox.first_child();
                            while let Some(c) = iter {
                                // The row structure is: Image(icon), Box(vbox)[Label(name), Label(subtitle)], Label(badge)
                                if c.clone().downcast::<gtk::Box>().is_ok() {
                                    if let Some(fc) = c.first_child() {
                                        if let Ok(lbl) = fc.downcast::<gtk::Label>() {
                                            label_text = lbl.text().to_lowercase();
                                        }
                                    }
                                }
                                iter = c.next_sibling();
                            }
                        }
                        label_text.contains(&query)
                    } else {
                        true
                    };
                    row.set_visible(visible);
                }
            }
        });
    }

    // --- Ctrl+F = focus profile search ---
    {
        let search_ref = profile_search.clone();
        let ctrl_f = gtk::CallbackAction::new(move |_widget, _| {
            search_ref.grab_focus();
            gtk::glib::Propagation::Proceed
        });
        shortcuts_ref.add_shortcut(gtk::Shortcut::new(
            Some(gtk::ShortcutTrigger::parse_string("<Control>f").unwrap()),
            Some(ctrl_f),
        ));
    }

    // --- Auto-save (debounced): save after 5s of no changes when dirty ---
    {
        let state_c = state.clone();
        let toast_overlay_c = toast_overlay.clone();
        let widgets_c = widgets.clone();
        gtk::glib::timeout_add_seconds_local(1, move || {
            let s = state_c.borrow();
            if s.dirty {
                if let Some(last_dirty) = s.last_dirty_time {
                    if last_dirty.elapsed() >= std::time::Duration::from_secs(5) {
                        let config = s.config.clone();
                        drop(s);
                        match config::save_config(&config) {
                            Ok(()) => {
                                state_c.borrow_mut().dirty = false;
                                state_c.borrow_mut().last_dirty_time = None;
                                helpers::show_toast(&toast_overlay_c, "Auto-saved", 2);
                                update_dirty_title(&state_c, &widgets_c);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    // --- Ctrl+S periodic check ---
    {
        let save_requested = save_requested.clone();
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let toast_overlay_c = toast_overlay.clone();
        gtk::glib::timeout_add_seconds_local(1, move || {
            if save_requested.get() {
                save_requested.set(false);
                let s = state_c.borrow();
                match config::save_config(&s.config) {
                    Ok(()) => {
                        drop(s);
                        state_c.borrow_mut().dirty = false;
                        state_c.borrow_mut().last_dirty_time = None;
                        update_dirty_title(&state_c, &widgets_c);
                        helpers::show_toast(&toast_overlay_c, "Saved", 2);
                        widgets_c.status_label.set_text("Configuration saved");
                        widgets_c.status_label.remove_css_class("error");
                    }
                    Err(e) => {
                        drop(s);
                        widgets_c
                            .status_label
                            .set_text(&format!("Save failed: {e}"));
                        widgets_c.status_label.add_css_class("error");
                    }
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    // --- Ctrl+N periodic check ---
    // Check if Ctrl+N was pressed and add a profile using a recurring idle source
    {
        let state_c = state.clone();
        let widgets_c = widgets.clone();
        let loading_c = loading.clone();
        let new_profile_requested = new_profile_requested.clone();
        gtk::glib::timeout_add_seconds_local(1, move || {
            if new_profile_requested.get() {
                new_profile_requested.set(false);
                let mut s = state_c.borrow_mut();
                let name = format!("New Profile {}", s.config.profiles.len() + 1);
                s.config.profiles.push(config::GameConf::default());
                let idx = s.config.profiles.len() - 1;
                s.config.profiles[idx].name = name.clone();
                s.dirty = true;
                s.last_dirty_time = Some(std::time::Instant::now());
                drop(s);

                let row = make_profile_row(&name, 2, 0);
                widgets_c.profile_listbox.append(&row);
                widgets_c.profile_listbox.select_row(Some(&row));
                load_profile_into_ui(&state_c, &widgets_c, &loading_c);
                update_dirty_title(&state_c, &widgets_c);
            }
            gtk::glib::ControlFlow::Continue
        });
    }

    // --- Close-request: warn if dirty ---
    {
        let state_c = state.clone();
        let window_c = window.clone();
        window.connect_close_request(move |_| {
            let dirty = state_c.borrow().dirty;
            if dirty {
                let dialog = gtk::MessageDialog::new(
                    Some(&window_c),
                    gtk::DialogFlags::MODAL,
                    gtk::MessageType::Question,
                    gtk::ButtonsType::YesNo,
                    "You have unsaved changes. Quit anyway?",
                );
                dialog.set_title(Some("Unsaved Changes"));
                let window_ref = window_c.clone();
                dialog.connect_response(move |dialog, response| {
                    if response == gtk::ResponseType::Yes {
                        window_ref.close();
                    }
                    dialog.close();
                });
                dialog.present();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }

    window.present();
}

/// Update window title with dirty indicator.
fn update_dirty_title(state: &Rc<RefCell<AppState>>, widgets: &Rc<AppWidgets>) {
    let dirty = state.borrow().dirty;
    let title = if dirty {
        "* lsfg-vk Configuration"
    } else {
        "lsfg-vk Configuration"
    };
    widgets.window.set_title(Some(title));
}

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
            widgets.profile_group.set_sensitive(true);
            widgets.custom_group.set_sensitive(true);
            pw.name_row.set_text(&profile.name);
            pw.multiplier_row.set_value(profile.multiplier as f64);
            pw.flow_row.set_value(profile.flow_scale as f64);
            pw.perf_switch.set_active(profile.performance_mode);
            pw.target_fps_row
                .set_value(profile.target_fps.unwrap_or(0) as f64);

            // GPU
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

            // Active in -> chips
            pw.active_in_editor.set_entries(profile.active_in_list());

            widgets.profile_group.set_description(None);
        }
    } else {
        widgets.profile_group.set_sensitive(false);
        widgets.custom_group.set_sensitive(false);
        pw.name_row.set_text("");
        pw.multiplier_row.set_value(2.0);
        pw.flow_row.set_value(1.0);
        pw.perf_switch.set_active(false);
        pw.target_fps_row.set_value(0.0);
        pw.gpu_row.set_selected(0);
        pw.active_in_editor.set_entries(vec![]);
        widgets
            .profile_group
            .set_description(Some("Select a profile from the list to edit"));
    }

    loading.set(false);
}

fn save_profile_from_ui(state: &Rc<RefCell<AppState>>, widgets: &Rc<AppWidgets>) {
    let name = widgets.pw.name_row.text().to_string();
    let multiplier = widgets.pw.multiplier_row.value() as u32;
    let flow_scale = widgets.pw.flow_row.value() as f32;
    let performance_mode = widgets.pw.perf_switch.is_active();
    let target_fps_val = widgets.pw.target_fps_row.value() as u32;
    let target_fps = if target_fps_val == 0 { None } else { Some(target_fps_val) };
    let gpu_selected = widgets.pw.gpu_row.selected();
    let gpu_name: Option<String> = if gpu_selected == 0 {
        None
    } else {
        widgets.pw.gpu_model.string(gpu_selected).map(|s| s.to_string())
    };

    let mut s = state.borrow_mut();
    let Some(idx) = s.selected_profile else { return };
    let Some(profile) = s.config.profiles.get_mut(idx) else { return };

    let name_changed = profile.name != name;

    profile.name = name;
    profile.multiplier = multiplier;
    profile.flow_scale = flow_scale;
    profile.performance_mode = performance_mode;
    profile.target_fps = target_fps;
    profile.gpu = gpu_name;
    s.dirty = true;

    drop(s);

    if name_changed {
        let s2 = state.borrow();
        if let Some(p) = s2.config.profiles.get(idx) {
            if let Some(row) = widgets.profile_listbox.row_at_index(idx as i32) {
                update_row_label(&row, &p.name, p.multiplier, p.active_in_list().len());
            }
        }
    }
}

/// Create a profile sidebar row with name + multiplier badge + active_in count.
/// Rebuild the entire profile listbox from state.
fn rebuild_profile_list(state: &Rc<RefCell<AppState>>, widgets: &Rc<AppWidgets>) {
    // Remove all rows
    while let Some(row) = widgets.profile_listbox.row_at_index(0) {
        widgets.profile_listbox.remove(&row);
    }
    // Re-add from state
    let s = state.borrow();
    for p in s.config.profiles.iter() {
        let row = make_profile_row(&p.name, p.multiplier, p.active_in_list().len());
        widgets.profile_listbox.append(&row);
    }
}

fn make_profile_row(name: &str, multiplier: u32, active_count: usize) -> gtk::ListBoxRow {
    let row = gtk::ListBoxRow::new();
    let box_ = gtk::Box::new(Orientation::Horizontal, 8);
    box_.set_margin_start(8);
    box_.set_margin_end(8);
    box_.set_margin_top(6);
    box_.set_margin_bottom(6);

    let icon = gtk::Image::from_icon_name("applications-games-symbolic");
    icon.set_pixel_size(20);
    box_.append(&icon);

    let vbox = gtk::Box::new(Orientation::Vertical, 2);
    let label = gtk::Label::new(Some(name));
    label.set_halign(Align::Start);
    label.set_hexpand(true);
    label.set_xalign(0.0);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    vbox.append(&label);

    let subtitle = gtk::Label::new(Some(&format!(
        "{}x FG  {}  {}",
        multiplier,
        if active_count == 0 { "no targets" } else { "targets" },
        active_count,
    )));
    subtitle.add_css_class("dim-label");
    subtitle.add_css_class("caption");
    subtitle.set_halign(Align::Start);
    subtitle.set_xalign(0.0);
    vbox.append(&subtitle);

    box_.append(&vbox);

    let badge = gtk::Label::new(Some(&format!("{}x", multiplier)));
    badge.add_css_class("tag");
    badge.add_css_class("accent");
    badge.set_valign(Align::Center);
    box_.append(&badge);

    row.set_child(Some(&box_));
    row
}

fn update_row_label(row: &gtk::ListBoxRow, name: &str, multiplier: u32, active_count: usize) {
    let child = row.child();
    let Some(box_) = child.and_then(|c| c.downcast::<gtk::Box>().ok()) else {
        return;
    };

    // The structure is: icon, vbox(label, subtitle), badge
    let mut iter = box_.first_child();
    while let Some(c) = iter {
        if let Ok(vbox) = c.clone().downcast::<gtk::Box>() {
            let fc = vbox.first_child();
            if let Some(label) = fc.clone().and_then(|l| l.downcast::<gtk::Label>().ok()) {
                label.set_text(name);
            }
            if let Some(fc2) = fc.as_ref().and_then(|l| l.next_sibling()) {
                if let Ok(sub) = fc2.clone().downcast::<gtk::Label>() {
                    sub.set_text(&format!(
                        "{}x FG  {}  {}",
                        multiplier,
                        if active_count == 0 {
                            "no targets"
                        } else {
                            "targets"
                        },
                        active_count,
                    ));
                }
            }
        }
        if let Ok(badge) = c.clone().downcast::<gtk::Label>() {
            let text = badge.text();
            if text.ends_with('x') && text.len() <= 3 {
                badge.set_text(&format!("{}x", multiplier));
            }
        }
        iter = c.next_sibling();
    }
}

/// Check if Proton compatibility is set up.
fn check_proton_status(label: &gtk::Label) {
    let proton_lib = std::path::Path::new(
        &std::env::var("HOME").unwrap_or_default(),
    )
    .join(".local/share/Steam/steamapps/common/Proton - Experimental/files/lib/x86_64-linux-gnu/liblsfg-vk.so");

    let user_layer = std::path::Path::new(
        &std::env::var("HOME").unwrap_or_default(),
    )
    .join(".local/share/vulkan/implicit_layer.d/VkLayer_LS_frame_generation.json");

    if proton_lib.exists() && user_layer.exists() {
        label.set_text("Ready");
        label.add_css_class("success");
    } else if !proton_lib.exists() && !user_layer.exists() {
        label.set_text("Not set up (native games OK)");
        label.add_css_class("dim-label");
    } else {
        let missing = if !proton_lib.exists() { ".so " } else { "" }
            .to_string()
            + if !user_layer.exists() { "JSON" } else { "" };
        label.set_text(&format!("Incomplete: missing {}", missing.trim()));
        label.add_css_class("warning");
    }
}
