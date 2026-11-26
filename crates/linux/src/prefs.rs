use crate::apply_theme;
use adw::prelude::*;
use simplenotorious_core::{Core, EmphasisMarker, Theme};
use std::rc::Rc;

fn pick_folder(win: &adw::ApplicationWindow, done: impl Fn(std::path::PathBuf) + 'static) {
    let d = gtk::FileDialog::builder().title("Choose notes folder").build();
    d.select_folder(Some(win), None::<&gtk::gio::Cancellable>, move |r| {
        if let Some(p) = r.ok().and_then(|f| f.path()) {
            done(p);
        }
    });
}

fn pick_json(win: &adw::ApplicationWindow, done: impl Fn(std::path::PathBuf) + 'static) {
    let d = gtk::FileDialog::builder()
        .title("Choose notes.json location")
        .initial_name("notes.json")
        .build();
    d.save(Some(win), None::<&gtk::gio::Cancellable>, move |r| {
        if let Some(p) = r.ok().and_then(|f| f.path()) {
            done(p);
        }
    });
}

/// Preferences (Ctrl+,).
pub fn show(win: &adw::ApplicationWindow, core: &Core, on_logout: Rc<dyn Fn()>) {
    let s = core.settings();
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    let page = adw::PreferencesPage::new();

    // --- appearance ---
    let g = adw::PreferencesGroup::builder().title("Appearance").build();
    let combo = adw::ComboRow::builder()
        .title("Color scheme")
        .subtitle("Follow the system setting, or always use light or dark")
        .model(&gtk::StringList::new(&["Follow system", "Light", "Dark"]))
        .selected(match s.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        })
        .build();
    {
        let core = core.clone();
        combo.connect_selected_notify(move |r| {
            let t = match r.selected() {
                1 => Theme::Light,
                2 => Theme::Dark,
                _ => Theme::System,
            };
            let _ = core.update_settings(|s| s.theme = t);
            apply_theme(t);
        });
    }
    g.add(&combo);
    page.add(&g);

    // --- editing ---
    let g = adw::PreferencesGroup::builder()
        .title("Markdown")
        .description("Characters used when formatting text as italic or bold")
        .build();
    let marker_row = |title: &str, subtitle: &str, items: &[&str], current: EmphasisMarker| {
        adw::ComboRow::builder()
            .title(title)
            .subtitle(subtitle)
            .model(&gtk::StringList::new(items))
            .selected(match current {
                EmphasisMarker::Asterisk => 0,
                EmphasisMarker::Underscore => 1,
            })
            .build()
    };
    let to_marker = |i: u32| if i == 1 { EmphasisMarker::Underscore } else { EmphasisMarker::Asterisk };
    let italic = marker_row("Italic marker", "Wrap italic text in", &["*text*", "_text_"], s.italic_marker);
    {
        let core = core.clone();
        italic.connect_selected_notify(move |r| {
            let m = to_marker(r.selected());
            let _ = core.update_settings(|s| s.italic_marker = m);
        });
    }
    g.add(&italic);
    let bold = marker_row("Bold marker", "Wrap bold text in", &["**text**", "__text__"], s.bold_marker);
    {
        let core = core.clone();
        bold.connect_selected_notify(move |r| {
            let m = to_marker(r.selected());
            let _ = core.update_settings(|s| s.bold_marker = m);
        });
    }
    g.add(&bold);
    page.add(&g);

    // --- account ---
    let g = adw::PreferencesGroup::builder().title("SimpleNote account").build();
    let acct = adw::ActionRow::builder()
        .title(if core.is_logged_in() {
            "Logged in"
        } else {
            "Not logged in"
        })
        .build();
    let out = gtk::Button::builder()
        .label("Log Out…")
        .valign(gtk::Align::Center)
        .sensitive(core.is_logged_in())
        .build();
    out.add_css_class("destructive-action");
    {
        let dialog = dialog.clone();
        out.connect_clicked(move |_| {
            dialog.close();
            on_logout();
        });
    }
    acct.add_suffix(&out);
    g.add(&acct);
    let sync_row = adw::ActionRow::builder()
        .title("Sync now")
        .subtitle("Edits are always saved locally first")
        .build();
    let sync_btn = gtk::Button::builder()
        .label("Sync")
        .valign(gtk::Align::Center)
        .sensitive(core.is_logged_in())
        .build();
    {
        let core = core.clone();
        sync_btn.connect_clicked(move |_| core.sync_now());
    }
    sync_row.add_suffix(&sync_btn);
    g.add(&sync_row);
    page.add(&g);

    // --- storage ---
    let g = adw::PreferencesGroup::builder()
        .title("Storage")
        .description("Notes are kept as plain .md files in this folder so other tools can sync them.")
        .build();
    let folder = adw::ActionRow::builder()
        .title("Notes folder")
        .subtitle(core.notes_folder().display().to_string())
        .subtitle_selectable(true)
        .build();
    let b = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    {
        let (core, win, folder) = (core.clone(), win.clone(), folder.clone());
        b.connect_clicked(move |_| {
            let (core, folder) = (core.clone(), folder.clone());
            pick_folder(&win, move |p| {
                if core.set_notes_folder(p.clone()).is_ok() {
                    folder.set_subtitle(&p.display().to_string());
                }
            });
        });
    }
    folder.add_suffix(&b);
    g.add(&folder);

    let json_switch = adw::SwitchRow::builder()
        .title("Keep notes.json in sync")
        .subtitle("Write all notes to a SimpleNote-format JSON file after every change")
        .active(s.json_sync_enabled)
        .build();
    {
        let core = core.clone();
        json_switch.connect_active_notify(move |r| {
            let _ = core.update_settings(|s| s.json_sync_enabled = r.is_active());
        });
    }
    g.add(&json_switch);
    let json = adw::ActionRow::builder()
        .title("notes.json location")
        .subtitle(core.json_path().display().to_string())
        .subtitle_selectable(true)
        .build();
    let b = gtk::Button::builder()
        .label("Choose…")
        .valign(gtk::Align::Center)
        .build();
    {
        let (core, win, json) = (core.clone(), win.clone(), json.clone());
        b.connect_clicked(move |_| {
            let (core, json) = (core.clone(), json.clone());
            pick_json(&win, move |p| {
                let _ = core.update_settings(|s| s.json_path = Some(p.clone()));
                json.set_subtitle(&p.display().to_string());
            });
        });
    }
    json.add_suffix(&b);
    g.add(&json);
    page.add(&g);

    dialog.add(&page);
    dialog.present(Some(win));
}
