mod prefs;
mod window;

use adw::prelude::*;
use simplenotorious_core::{Core, Theme};

pub const APP_ID: &str = "me.mjdescy.Simplenotorious";

pub fn apply_theme(t: Theme) {
    adw::StyleManager::default().set_color_scheme(match t {
        Theme::System => adw::ColorScheme::Default,
        Theme::Light => adw::ColorScheme::ForceLight,
        Theme::Dark => adw::ColorScheme::ForceDark,
    });
}

thread_local! {
    static SCALE_PROVIDER: gtk::CssProvider = gtk::CssProvider::new();
}

/// Scale all text in the app (1.0 = default size).
pub fn apply_text_scale(scale: f64) {
    SCALE_PROVIDER.with(|p| {
        // font-size is inherited, so scaling the window root scales every widget.
        p.load_from_data(&format!("window {{ font-size: {}%; }}", (scale * 100.0).round()));
    });
}

const CSS: &str = "
.note-snippet { opacity: 0.65; font-size: 0.9em; }
.note-title { font-weight: 700; }
.tag-chip { background: alpha(@accent_bg_color, 0.2); color: @accent_color; border-radius: 6px; padding: 1px 10px; min-height: 0; }
.sync-status { opacity: 0.7; }
/* Fixed horizontal metrics so the sync status can align with the note text. */
list.note-list > row { margin: 2px 6px; padding-left: 0; padding-right: 0; }
textview.editor { font-size: 1.05em; }
";

fn main() -> adw::glib::ExitCode {
    let core = match Core::open_default() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot open the notes database: {e}");
            return adw::glib::ExitCode::FAILURE;
        }
    };
    let app = adw::Application::builder().application_id(APP_ID).build();

    app.connect_startup(|_| {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(CSS);
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
            SCALE_PROVIDER.with(|p| {
                gtk::style_context_add_provider_for_display(&display, p, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
            });
        }
    });

    let c = core.clone();
    app.connect_activate(move |app| {
        if let Some(w) = app.active_window() {
            w.present();
            return;
        }
        apply_theme(c.settings().theme);
        apply_text_scale(c.settings().clamped_text_scale());
        window::build(app, &c);
    });

    let c = core.clone();
    app.connect_shutdown(move |_| {
        let _ = c.flush_exports(); // last chance to mirror edits to disk
        c.stop_background();
    });

    app.run()
}
