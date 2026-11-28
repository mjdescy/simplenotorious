use crate::{licenses, privacy};
use adw::prelude::*;
use std::rc::Rc;

const LICENSES_URI: &str = "simplenotorious:licenses";
const PRIVACY_URI: &str = "simplenotorious:privacy";

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

/// Adds a row below "Legal" that calls `show`; falls back to an AboutDialog link with `uri`.
fn add_row(dialog: &adw::AboutDialog, title: &str, uri: &'static str, show: Rc<dyn Fn()>) {
    let group = dialog
        .child()
        .and_then(|c| find_legal_row(&c))
        .and_then(|r| r.ancestor(adw::PreferencesGroup::static_type()))
        .and_then(|g| g.downcast::<adw::PreferencesGroup>().ok());
    if let Some(group) = group {
        let row = adw::ActionRow::builder().title(title).activatable(true).build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        row.connect_activated(move |_| (*show)());
        group.add(&row);
    } else {
        dialog.add_link(title, uri);
        dialog.connect_activate_link(move |_, target| {
            if target != uri {
                return false;
            }
            (*show)();
            true
        });
    }
}

/// Adds the app's own legal entries below "Legal" on the About page.
pub fn add_to_about(dialog: &adw::AboutDialog, parent: &adw::ApplicationWindow) {
    let win = parent.clone();
    add_row(
        dialog,
        "Third-Party Licenses",
        LICENSES_URI,
        Rc::new(move || licenses::show(&win)),
    );
    let win = parent.clone();
    add_row(
        dialog,
        "Privacy Policy",
        PRIVACY_URI,
        Rc::new(move || privacy::show(&win)),
    );
}
