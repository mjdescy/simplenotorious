use adw::prelude::*;
use simplenotorious_core::markdown;
use webkit6::prelude::*;

/// The privacy policy, embedded at compile time from the repository's `PRIVACY.md`.
pub const TEXT: &str = include_str!("../../../PRIVACY.md");

/// Shows the embedded privacy policy, rendered from Markdown, in a dialog.
pub fn show(parent: &impl IsA<gtk::Widget>) {
    let webview = webkit6::WebView::new();
    if let Some(settings) = WebViewExt::settings(&webview) {
        settings.set_enable_javascript(false); // static document: no scripts
    }
    webview.set_vexpand(true);
    webview.set_hexpand(true);
    load(&webview);

    // Links open in the user's browser; the dialog itself never navigates.
    webview.connect_decide_policy(|_, decision, ty| {
        if ty != webkit6::PolicyDecisionType::NavigationAction {
            return false;
        }
        let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() else {
            return false;
        };
        let Some(action) = nav.navigation_action() else {
            return false;
        };
        if action.navigation_type() == webkit6::NavigationType::LinkClicked {
            if let Some(uri) = action.request().and_then(|r| r.uri()) {
                crate::open_external_uri(&uri);
            }
            decision.ignore();
            return true;
        }
        false
    });

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&webview));

    adw::Dialog::builder()
        .title("Privacy Policy")
        .content_width(560)
        .content_height(640)
        .child(&toolbar)
        .build()
        .present(Some(parent));
}

fn load(webview: &webkit6::WebView) {
    let dark = adw::StyleManager::default().is_dark();
    webview.load_html(&markdown::render_html(TEXT, dark), None);
}
