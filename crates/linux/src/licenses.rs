use adw::prelude::*;
use gtk::{glib, pango};
use serde::Deserialize;
use std::path::PathBuf;

/// One third-party crate, as stored in `data/licenses.json` (see `scripts/gen-licenses.nu`).
#[derive(Deserialize)]
struct Entry {
    name: String,
    version: String,
    authors: String,
    description: String,
    license_id: String,
    license_text: String,
}

const FILE_NAME: &str = "licenses.json";

/// Installed copy first (`$XDG_DATA_DIRS`, which includes /app/share in the Flatpak), then the source tree.
fn data_file() -> Option<PathBuf> {
    glib::system_data_dirs()
        .into_iter()
        .map(|d| d.join("simplenotorious").join(FILE_NAME))
        .chain([PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join(FILE_NAME)])
        .find(|p| p.is_file())
}

fn load() -> Result<Vec<Entry>, String> {
    let path = data_file().ok_or_else(|| format!("{FILE_NAME} was not found"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn detail_page(e: &Entry) -> adw::NavigationPage {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();

    let label = |text: &str, classes: &[&str]| {
        let l = gtk::Label::builder()
            .label(text)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(pango::WrapMode::WordChar)
            .selectable(true)
            .css_classes(classes.iter().map(|c| c.to_string()).collect::<Vec<_>>())
            .build();
        content.append(&l);
        l
    };
    let authors = if e.authors.is_empty() {
        "Unknown"
    } else {
        e.authors.as_str()
    };
    label(&format!("{} {}", e.name, e.version), &["title-2"]);
    label(&format!("By {authors}"), &["dim-label"]);
    if !e.description.is_empty() {
        label(&e.description, &[]);
    }
    label(&format!("License: {}", e.license_id), &["heading"]);
    label(&e.license_text, &["monospace", "caption"]);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&adw::Clamp::builder().maximum_size(700).child(&content).build())
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    adw::NavigationPage::builder().title(&e.name).child(&toolbar).build()
}

fn list_page(nav: &adw::NavigationView, entries: Vec<Entry>) -> adw::NavigationPage {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    for e in &entries {
        let row = adw::ActionRow::builder()
            .title(&e.name)
            .subtitle(format!("{} · {}", e.version, e.license_id))
            .use_markup(false)
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        list.append(&row);
    }
    {
        let nav = nav.clone();
        let entries = std::rc::Rc::new(entries);
        list.connect_row_activated(move |_, row| {
            if let Some(e) = entries.get(row.index() as usize) {
                nav.push(&detail_page(e));
            }
        });
    }

    let search = gtk::SearchEntry::builder().placeholder_text("Search libraries").build();
    {
        let list = list.clone();
        let search = search.clone();
        list.set_filter_func(move |row| {
            let q = search.text().to_lowercase();
            let r = row.downcast_ref::<adw::ActionRow>();
            q.is_empty()
                || r.is_some_and(|r| {
                    r.title().to_lowercase().contains(&q)
                        || r.subtitle().unwrap_or_default().to_lowercase().contains(&q)
                })
        });
    }
    {
        let list = list.clone();
        search.connect_search_changed(move |_| list.invalidate_filter());
    }

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(24)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&search);
    content.append(&list);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&adw::Clamp::builder().maximum_size(700).child(&content).build())
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&scroll));
    adw::NavigationPage::builder()
        .title("Third-Party Licenses")
        .child(&toolbar)
        .build()
}

const FALLBACK_URI: &str = "simplenotorious:licenses";

/// libadwaita has no API for extra rows on the About page, so find the built-in "Legal" row
/// (the one that pushes the "legal" page) and append ours to its group.
fn find_legal_row(w: &gtk::Widget) -> Option<adw::ActionRow> {
    if let Some(row) = w.downcast_ref::<adw::ActionRow>() {
        let a = row.upcast_ref::<gtk::Actionable>();
        let target = a.action_target_value();
        if a.action_name().as_deref() == Some("navigation.push")
            && target.as_ref().and_then(|v| v.str()) == Some("legal")
        {
            return Some(row.clone());
        }
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        if let Some(found) = find_legal_row(&c) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}

/// Adds a "Third-Party Licenses" row directly below "Legal" on the About page.
pub fn add_to_about(dialog: &adw::AboutDialog, parent: &adw::ApplicationWindow) {
    let group = dialog
        .child()
        .and_then(|c| find_legal_row(&c))
        .and_then(|r| r.ancestor(adw::PreferencesGroup::static_type()))
        .and_then(|g| g.downcast::<adw::PreferencesGroup>().ok());
    let win = parent.clone();
    if let Some(group) = group {
        let row = adw::ActionRow::builder()
            .title("Third-Party Licenses")
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        row.connect_activated(move |_| show(&win));
        group.add(&row);
    } else {
        dialog.add_link("Third-Party Licenses", FALLBACK_URI);
        dialog.connect_activate_link(move |_, uri| {
            if uri != FALLBACK_URI {
                return false;
            }
            show(&win);
            true
        });
    }
}

/// Third-party licenses: a list of libraries; selecting one slides over to its license text.
fn show(parent: &impl IsA<gtk::Widget>) {
    let nav = adw::NavigationView::new();
    match load() {
        Ok(entries) => nav.add(&list_page(&nav, entries)),
        Err(msg) => {
            let status = adw::StatusPage::builder()
                .icon_name("dialog-warning-symbolic")
                .title("Licenses unavailable")
                .description(glib::markup_escape_text(&msg))
                .build();
            let toolbar = adw::ToolbarView::new();
            toolbar.add_top_bar(&adw::HeaderBar::new());
            toolbar.set_content(Some(&status));
            nav.add(
                &adw::NavigationPage::builder()
                    .title("Third-Party Licenses")
                    .child(&toolbar)
                    .build(),
            );
        }
    }
    adw::Dialog::builder()
        .title("Third-Party Licenses")
        .content_width(560)
        .content_height(640)
        .child(&nav)
        .build()
        .present(Some(parent));
}
