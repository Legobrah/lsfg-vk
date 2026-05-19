//! Inline chip-based active_in editor with autocomplete.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4::glib::clone;
use gtk4::{self as gtk, Align, Orientation, PolicyType};

use crate::process::{build_selector_entries, scan_desktop_apps, scan_processes, SelectorEntry};

type ChangedCb = Rc<RefCell<Box<dyn Fn(Vec<String>)>>>;

struct Inner {
    entries: Vec<String>,
    on_changed: ChangedCb,
    all_candidates: Vec<SelectorEntry>,
    chip_box: gtk::FlowBox,
    search: gtk::SearchEntry,
    popover: gtk::Popover,
    listbox: gtk::ListBox,
}

pub struct ActiveInEditor {
    inner: Rc<RefCell<Inner>>,
    root: gtk::Box,
}

impl ActiveInEditor {
    pub fn new(on_changed: Box<dyn Fn(Vec<String>)>) -> Self {
        let root = gtk::Box::new(Orientation::Vertical, 6);

        let chip_box = gtk::FlowBox::new();
        chip_box.set_orientation(gtk::Orientation::Horizontal);
        chip_box.set_homogeneous(false);
        chip_box.set_min_children_per_line(1);
        chip_box.set_max_children_per_line(20);
        chip_box.set_selection_mode(gtk::SelectionMode::None);
        chip_box.set_column_spacing(4);
        chip_box.set_row_spacing(4);

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Type to add executable..."));
        search.set_hexpand(true);

        let popover = gtk::Popover::new();
        popover.set_autohide(true);
        popover.set_has_arrow(false);
        popover.set_position(gtk::PositionType::Bottom);
        popover.set_size_request(400, -1);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
        scroll.set_max_content_height(280);
        scroll.set_propagate_natural_height(true);

        let listbox = gtk::ListBox::new();
        listbox.set_selection_mode(gtk::SelectionMode::None);
        listbox.add_css_class("rich-list");
        scroll.set_child(Some(&listbox));
        popover.set_child(Some(&scroll));

        let procs = scan_processes();
        let apps = scan_desktop_apps();
        let all_candidates = build_selector_entries(&procs, &apps, &[]);

        let on_changed = Rc::new(RefCell::new(on_changed));

        let inner = Rc::new(RefCell::new(Inner {
            entries: Vec::new(),
            on_changed,
            all_candidates,
            chip_box: chip_box.clone(),
            search: search.clone(),
            popover: popover.clone(),
            listbox: listbox.clone(),
        }));

        // Search changed -> refresh popover
        let inner_c = inner.clone();
        search.connect_changed(clone!(
            #[strong]
            inner_c,
            move |_| {
                Self::refresh_popover(&inner_c);
            }
        ));

        // Show popover when search gets focus and has text
        let popover_c = popover.clone();
        search.connect_notify_local(
            Some("has-focus"),
            clone!(
                #[strong]
                popover_c,
                move |search: &gtk::SearchEntry, _| {
                    if search.has_focus() && !search.text().is_empty() {
                        popover_c.popup();
                    }
                }
            ),
        );

        // Escape to close
        let ec = gtk::EventControllerKey::new();
        let popover_c2 = popover.clone();
        ec.connect_key_pressed(clone!(
            #[strong]
            popover_c2,
            move |_, keyval, _, _| {
                if keyval == gtk::gdk::Key::Escape {
                    popover_c2.popdown();
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            }
        ));
        search.add_controller(ec);

        popover.set_parent(&search);

        root.append(&chip_box);
        root.append(&search);

        Self { inner, root }
    }

    pub fn widget(&self) -> &gtk::Box {
        &self.root
    }

    pub fn set_entries(&self, entries: Vec<String>) {
        self.inner.borrow_mut().entries = entries;
        Self::rebuild_chips(&self.inner);
    }

    pub fn get_entries(&self) -> Vec<String> {
        self.inner.borrow().entries.clone()
    }

    fn fire_changed(inner: &Rc<RefCell<Inner>>, entries: Vec<String>) {
        let cb = inner.borrow().on_changed.clone();
        cb.borrow()(entries);
    }

    fn rebuild_chips(inner: &Rc<RefCell<Inner>>) {
        let (entries, chip_box) = {
            let b = inner.borrow();
            (b.entries.clone(), b.chip_box.clone())
        };

        while let Some(child) = chip_box.first_child() {
            chip_box.remove(&child);
        }

        for name in &entries {
            let chip = Self::make_chip(name, inner);
            chip_box.insert(&chip, -1);
        }
    }

    fn make_chip(name: &str, inner: &Rc<RefCell<Inner>>) -> gtk::Box {
        let chip = gtk::Box::new(Orientation::Horizontal, 4);
        chip.add_css_class("pill");
        chip.set_margin_top(2);
        chip.set_margin_bottom(2);

        let label = gtk::Label::new(Some(name));
        label.add_css_class("caption");
        label.set_max_width_chars(30);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        chip.append(&label);

        let btn = gtk::Button::from_icon_name("window-close-symbolic");
        btn.add_css_class("small-button");
        btn.set_tooltip_text(Some("Remove"));
        btn.set_valign(Align::Center);
        btn.add_css_class("circular");

        let name_owned = name.to_string();
        let inner_c = inner.clone();
        btn.connect_clicked(clone!(
            #[strong]
            inner_c,
            move |_| {
                let new_entries = {
                    let mut b = inner_c.borrow_mut();
                    b.entries.retain(|e| e != &name_owned);
                    b.entries.clone()
                };
                Self::rebuild_chips(&inner_c);
                Self::fire_changed(&inner_c, new_entries);
            }
        ));

        chip.append(&btn);
        chip
    }

    fn refresh_popover(inner: &Rc<RefCell<Inner>>) {
        let (query, listbox, popover, existing, all) = {
            let b = inner.borrow();
            (
                b.search.text().to_string().to_lowercase(),
                b.listbox.clone(),
                b.popover.clone(),
                b.entries.iter().map(|e| e.to_lowercase()).collect::<std::collections::HashSet<String>>(),
                b.all_candidates.clone(),
            )
        };

        while let Some(child) = listbox.first_child() {
            listbox.remove(&child);
        }

        if query.is_empty() {
            popover.popdown();
            return;
        }

        let mut count = 0;
        for entry in all.iter() {
            if count >= 30 {
                break;
            }
            if !entry.label.to_lowercase().contains(&query)
                && !entry.executable.to_lowercase().contains(&query)
                && !entry.sublabel.to_lowercase().contains(&query)
            {
                continue;
            }

            let already = existing.contains(&entry.executable.to_lowercase());

            let row = gtk::ListBoxRow::new();
            let hbox = gtk::Box::new(Orientation::Horizontal, 8);
            hbox.set_margin_start(8);
            hbox.set_margin_end(8);
            hbox.set_margin_top(4);
            hbox.set_margin_bottom(4);

            let lbl = gtk::Label::new(Some(&entry.label));
            lbl.set_halign(Align::Start);
            lbl.set_hexpand(true);
            lbl.set_xalign(0.0);
            lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
            hbox.append(&lbl);

            let exe_lbl = gtk::Label::new(Some(&entry.executable));
            exe_lbl.add_css_class("dim-label");
            exe_lbl.add_css_class("caption");
            exe_lbl.set_valign(Align::Center);
            hbox.append(&exe_lbl);

            if already {
                let badge = gtk::Label::new(Some("ADDED"));
                badge.add_css_class("dim-label");
                badge.add_css_class("caption");
                badge.set_valign(Align::Center);
                hbox.append(&badge);
            }

            row.set_child(Some(&hbox));

            let exec_name = entry.executable.clone();
            let inner_c = inner.clone();
            row.connect_activate(clone!(
                #[strong]
                inner_c,
                move |_row| {
                    let new_entries = {
                        let mut b = inner_c.borrow_mut();
                        let lower = exec_name.to_lowercase();
                        if b.entries.iter().any(|e| e.to_lowercase() == lower) {
                            return;
                        }
                        b.entries.push(exec_name.clone());
                        let search = b.search.clone();
                        let popover = b.popover.clone();
                        let listbox = b.listbox.clone();
                        drop(b);
                        search.set_text("");
                        popover.popdown();
                        while let Some(child) = listbox.first_child() {
                            listbox.remove(&child);
                        }
                        inner_c.borrow().entries.clone()
                    };
                    Self::rebuild_chips(&inner_c);
                    Self::fire_changed(&inner_c, new_entries);
                }
            ));

            listbox.append(&row);
            count += 1;
        }

        if count > 0 {
            popover.popup();
        } else {
            popover.popdown();
        }
    }
}
