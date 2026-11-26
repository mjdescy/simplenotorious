use crate::prefs;
use adw::prelude::*;
use gtk::{gio, glib};
use simplenotorious_core::{Core, Event, Filter, NoteSummary, SyncStatus};
use sourceview5::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Duration};
use webkit6::prelude::*;

#[derive(Clone, PartialEq)]
enum Mode {
    All,
    Trash,
    Tag(String),
}

#[derive(Default)]
struct State {
    current: Option<String>,
    mode_tags: Vec<String>,
    save_gen: u64,
    /// Editor has changes not yet written to the core.
    pending: bool,
    /// Suppress change handlers while we programmatically update widgets.
    busy: bool,
    preview: bool,
    status: Option<SyncStatus>,
}

struct Ui {
    core: Core,
    win: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    root: gtk::Stack,
    banner: adw::Banner,
    split: adw::OverlaySplitView,
    search: gtk::SearchEntry,
    filter_dd: gtk::DropDown,
    filter_model: gtk::StringList,
    list: gtk::ListBox,
    status_lbl: gtk::Label,
    content: gtk::Stack,
    title: adw::WindowTitle,
    buffer: sourceview5::Buffer,
    view: sourceview5::View,
    webview: webkit6::WebView,
    tag_box: gtk::Box,
    tag_entry: gtk::Entry,
    pin_btn: gtk::ToggleButton,
    preview_btn: gtk::ToggleButton,
    trash_btn: gtk::Button,
    info_btn: gtk::MenuButton,
    info_lbl: gtk::Label,
    login_entry: gtk::Entry,
    login_btn: gtk::Button,
    login_err: gtk::Label,
    st: RefCell<State>,
}

pub fn build(app: &adw::Application, core: &Core) {
    let ui = Ui::new(app, core);
    ui.refresh_list();
    ui.on_status(core.status());
    ui.win.present();
}

impl Ui {
    fn new(app: &adw::Application, core: &Core) -> Rc<Ui> {
        // ---------------- sidebar ----------------
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search")
            .hexpand(true)
            .build();
        let new_btn = gtk::Button::builder()
            .icon_name("document-new-symbolic")
            .tooltip_text("New note (Ctrl+N)")
            .action_name("win.new-note")
            .build();
        let side_header = adw::HeaderBar::new();
        side_header.set_title_widget(Some(&search));
        side_header.set_show_end_title_buttons(false);

        let filter_model = gtk::StringList::new(&["All Notes", "Trash"]);
        let filter_dd = gtk::DropDown::builder().model(&filter_model).build();
        filter_dd.set_margin_start(8);
        filter_dd.set_margin_end(8);
        filter_dd.set_margin_top(6);
        filter_dd.set_margin_bottom(6);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("navigation-sidebar");
        list.add_css_class("note-list");
        let list_scroll = gtk::ScrolledWindow::builder()
            .child(&list)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();

        let status_lbl = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .margin_top(6)
            .margin_bottom(8)
            .build();
        status_lbl.add_css_class("sync-status");

        let side_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        side_body.append(&filter_dd);
        side_body.append(&list_scroll);
        side_body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        side_body.append(&status_lbl);
        let sidebar = adw::ToolbarView::new();
        sidebar.add_top_bar(&side_header);
        sidebar.set_content(Some(&side_body));

        // ---------------- content ----------------
        let sidebar_btn = gtk::ToggleButton::builder()
            .icon_name("sidebar-show-symbolic")
            .tooltip_text("Toggle sidebar (F9)")
            .build();
        let title = adw::WindowTitle::new("Simplenotorious", "");
        let preview_btn = gtk::ToggleButton::builder()
            .icon_name("view-reveal-symbolic")
            .tooltip_text("Toggle Markdown preview (Ctrl+P)")
            .build();
        let pin_btn = gtk::ToggleButton::builder()
            .icon_name("view-pin-symbolic")
            .tooltip_text("Pin note (Ctrl+Alt+P)")
            .build();
        let trash_btn = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .tooltip_text("Move to trash (Ctrl+Alt+D)")
            .build();
        let info_lbl = gtk::Label::builder().xalign(0.0).build();
        let info_pop = gtk::Popover::builder().child(&info_lbl).build();
        info_lbl.set_margin_start(12);
        info_lbl.set_margin_end(12);
        info_lbl.set_margin_top(10);
        info_lbl.set_margin_bottom(10);
        // An open popover grabs key events, so give it its own Ctrl+Alt+I to close it.
        {
            let sc = gtk::ShortcutController::new();
            sc.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("<Control><Alt>i"),
                Some(gtk::CallbackAction::new(|w, _| {
                    if let Ok(p) = w.clone().downcast::<gtk::Popover>() {
                        p.popdown();
                    }
                    glib::Propagation::Stop
                })),
            ));
            info_pop.add_controller(sc);
        }
        let info_btn = gtk::MenuButton::builder()
            .icon_name("dialog-information-symbolic")
            .tooltip_text("Note information (Ctrl+Alt+I)")
            .popover(&info_pop)
            .build();

        let menu = gio::Menu::new();
        menu.append(Some("Sync Now"), Some("win.sync"));
        menu.append(Some("Import notes.json…"), Some("win.import"));
        menu.append(Some("Export notes.json…"), Some("win.export"));
        let sec = gio::Menu::new();
        sec.append(Some("Preferences"), Some("app.preferences"));
        sec.append(Some("Keyboard Shortcuts"), Some("app.shortcuts"));
        sec.append(Some("Log Out…"), Some("win.logout"));
        sec.append(Some("About Simplenotorious"), Some("app.about"));
        menu.append_section(None, &sec);
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .primary(true)
            .build();

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        header.pack_start(&sidebar_btn);
        header.pack_end(&menu_btn);
        header.pack_end(&preview_btn);
        header.pack_end(&info_btn);
        header.pack_end(&trash_btn);
        header.pack_end(&pin_btn);

        let buffer = sourceview5::Buffer::new(None);
        if let Some(lang) = sourceview5::LanguageManager::default().language("markdown") {
            buffer.set_language(Some(&lang));
        }
        buffer.set_highlight_syntax(true);
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_left_margin(36);
        view.set_right_margin(36);
        view.set_top_margin(24);
        view.set_bottom_margin(24);
        view.set_pixels_above_lines(2);
        view.add_css_class("editor");
        view.set_auto_indent(true);
        let editor_scroll = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .hexpand(true)
            .build();

        let webview = webkit6::WebView::new();
        if let Some(s) = WebViewExt::settings(&webview) {
            s.set_enable_javascript(false); // read-only preview: no scripts
        }
        webview.set_vexpand(true);
        webview.set_hexpand(true);
        // links open in the user's browser; the preview itself never navigates
        webview.connect_decide_policy(|_, decision, ty| {
            use webkit6::prelude::*;
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
                    let _ = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
                }
                decision.ignore();
                return true;
            }
            false
        });

        let empty = adw::StatusPage::builder()
            .icon_name("document-edit-symbolic")
            .title("No Note Selected")
            .description("Select a note or press Ctrl+N to create one.")
            .build();
        let content = gtk::Stack::new();
        content.add_named(&empty, Some("empty"));
        content.add_named(&editor_scroll, Some("editor"));
        content.add_named(&webview, Some("preview"));

        let tag_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let tag_entry = gtk::Entry::builder()
            .placeholder_text("Add tag…")
            .width_chars(14)
            .build();
        tag_entry.add_css_class("flat");
        let tag_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        tag_bar.set_margin_start(16);
        tag_bar.set_margin_end(16);
        tag_bar.set_margin_top(6);
        tag_bar.set_margin_bottom(8);
        tag_bar.append(&tag_box);
        tag_bar.append(&tag_entry);

        // Keep the sidebar's sync-status bar the same height as the tag bar.
        let bar_height = gtk::SizeGroup::new(gtk::SizeGroupMode::Vertical);
        bar_height.add_widget(&tag_bar);
        bar_height.add_widget(&status_lbl);

        let content_body = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_body.append(&content);
        content_body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content_body.append(&tag_bar);
        let content_view = adw::ToolbarView::new();
        content_view.add_top_bar(&header);
        content_view.set_content(Some(&content_body));

        // ---------------- split view + banner ----------------
        let split = adw::OverlaySplitView::builder()
            .sidebar(&sidebar)
            .content(&content_view)
            .sidebar_width_fraction(0.28)
            .min_sidebar_width(250.0)
            .max_sidebar_width(380.0)
            .show_sidebar(!core.settings().sidebar_collapsed)
            .build();
        split.set_vexpand(true);
        sidebar_btn.set_active(split.shows_sidebar());
        split
            .bind_property("show-sidebar", &sidebar_btn, "active")
            .bidirectional()
            .sync_create()
            .build();

        // The "new note" button lives in the sidebar header while the sidebar is
        // shown, and moves to the far left of the content header while it is hidden.
        let place_new_btn = {
            let new_btn = new_btn.clone();
            let side_header = side_header.clone();
            let header = header.clone();
            let sidebar_btn = sidebar_btn.clone();
            move |sidebar_shown: bool| {
                let (from, to) = if sidebar_shown {
                    (&header, &side_header)
                } else {
                    (&side_header, &header)
                };
                // Detach from whichever bar currently holds the button.
                if new_btn.is_ancestor(from) {
                    from.remove(&new_btn);
                }
                if new_btn.parent().is_none() {
                    if sidebar_shown {
                        to.pack_end(&new_btn);
                    } else {
                        // Leftmost: pack before the sidebar toggle, which is
                        // re-packed after it.
                        header.remove(&sidebar_btn);
                        to.pack_start(&new_btn);
                        to.pack_start(&sidebar_btn);
                    }
                }
            }
        };
        place_new_btn(split.shows_sidebar());
        split.connect_show_sidebar_notify(move |s| place_new_btn(s.shows_sidebar()));

        let banner = adw::Banner::builder()
            .title("You're offline. Changes are saved on this computer and will sync when the connection returns.")
            .build();
        let main_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        main_page.append(&banner);
        main_page.append(&split);

        // ---------------- first-run login page ----------------
        let open_login = gtk::Button::builder()
            .label("Open SimpleNote Login Page")
            .halign(gtk::Align::Center)
            .build();
        open_login.add_css_class("suggested-action");
        open_login.add_css_class("pill");
        let step2 = gtk::Label::builder()
            .label("After signing in, paste the code shown (it looks like  you@example.com:ABC123)")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        step2.add_css_class("dim-label");
        let login_entry = gtk::Entry::builder()
            .placeholder_text("email:CODE")
            .activates_default(true)
            .build();
        let login_btn = gtk::Button::builder()
            .label("Log In")
            .halign(gtk::Align::Center)
            .build();
        login_btn.add_css_class("pill");
        let login_err = gtk::Label::builder().wrap(true).visible(false).build();
        login_err.add_css_class("error");
        let lb = gtk::Box::new(gtk::Orientation::Vertical, 14);
        for w in [
            open_login.upcast_ref::<gtk::Widget>(),
            step2.upcast_ref(),
            login_entry.upcast_ref(),
            login_btn.upcast_ref(),
            login_err.upcast_ref(),
        ] {
            lb.append(w);
        }
        let clamp = adw::Clamp::builder().maximum_size(420).child(&lb).build();
        let login_page = adw::StatusPage::builder()
            .icon_name("avatar-default-symbolic")
            .title("Welcome to Simplenotorious")
            .description("Log in to SimpleNote to view and sync your notes.")
            .child(&clamp)
            .build();
        let login_view = adw::ToolbarView::new();
        let login_header = adw::HeaderBar::new();
        login_header.add_css_class("flat");
        login_view.add_top_bar(&login_header);
        login_view.set_content(Some(&login_page));

        let root = gtk::Stack::new();
        root.add_named(&login_view, Some("login"));
        root.add_named(&main_page, Some("main"));
        root.set_visible_child_name(if core.is_logged_in() { "main" } else { "login" });

        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&root));
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title("Simplenotorious")
            .default_width(1000)
            .default_height(680)
            .content(&toasts)
            .build();

        let ui = Rc::new(Ui {
            core: core.clone(),
            win,
            toasts,
            root,
            banner,
            split,
            search,
            filter_dd,
            filter_model,
            list,
            status_lbl,
            content,
            title,
            buffer,
            view,
            webview,
            tag_box,
            tag_entry,
            pin_btn,
            preview_btn,
            trash_btn,
            info_btn,
            info_lbl,
            login_entry,
            login_btn,
            login_err,
            st: RefCell::new(State::default()),
        });
        ui.wire(app, &open_login);
        ui
    }

    fn wire(self: &Rc<Self>, app: &adw::Application, open_login: &gtk::Button) {
        let ui = self;

        // ----- actions & shortcuts -----
        let add = |name: &str, f: Box<dyn Fn(&Rc<Ui>)>| {
            let a = gio::SimpleAction::new(name, None);
            let ui2 = ui.clone();
            a.connect_activate(move |_, _| f(&ui2));
            ui.win.add_action(&a);
        };
        add("new-note", Box::new(|ui| ui.new_note()));
        add("sync", Box::new(|ui| ui.core.sync_now()));
        add("import", Box::new(|ui| ui.import_json()));
        add("export", Box::new(|ui| ui.export_json()));
        add("logout", Box::new(|ui| ui.confirm_logout()));
        add(
            "focus-search",
            Box::new(|ui| {
                ui.split.set_show_sidebar(true);
                ui.search.grab_focus();
            }),
        );
        add(
            "sidebar",
            Box::new(|ui| ui.split.set_show_sidebar(!ui.split.shows_sidebar())),
        );
        add(
            "pin",
            Box::new(|ui| {
                if ui.pin_btn.is_sensitive() {
                    ui.pin_btn.set_active(!ui.pin_btn.is_active());
                }
            }),
        );
        add(
            "trash",
            Box::new(|ui| {
                if ui.trash_btn.is_sensitive() {
                    ui.trash_or_restore();
                }
            }),
        );
        add(
            "info",
            Box::new(|ui| {
                if ui.info_btn.is_sensitive() {
                    if ui.info_btn.popover().is_some_and(|p| p.is_visible()) {
                        ui.info_btn.popdown();
                    } else {
                        ui.info_btn.popup();
                    }
                }
            }),
        );
        add(
            "preview",
            Box::new(|ui| ui.preview_btn.set_active(!ui.preview_btn.is_active())),
        );

        let prefs_action = gio::SimpleAction::new("preferences", None);
        {
            let ui = ui.clone();
            prefs_action.connect_activate(move |_, _| {
                let u = ui.clone();
                prefs::show(&ui.win, &ui.core, Rc::new(move || u.confirm_logout()));
            });
        }
        app.add_action(&prefs_action);
        let about = gio::SimpleAction::new("about", None);
        {
            let ui = ui.clone();
            about.connect_activate(move |_, _| {
                adw::AboutDialog::builder()
                    .application_name("Simplenotorious")
                    .developer_name("Simplenotorious")
                    .version(env!("CARGO_PKG_VERSION"))
                    .comments("A native Markdown notes app for SimpleNote.")
                    .license_type(gtk::License::MitX11)
                    .build()
                    .present(Some(&ui.win));
            });
        }
        app.add_action(&about);
        let shortcuts = gio::SimpleAction::new("shortcuts", None);
        {
            let ui = ui.clone();
            shortcuts.connect_activate(move |_, _| show_shortcuts(&ui.win));
        }
        app.add_action(&shortcuts);
        app.set_accels_for_action("app.shortcuts", &["<Control>question", "<Control><Shift>slash"]);
        app.set_accels_for_action("app.preferences", &["<Control>comma"]);
        app.set_accels_for_action("win.new-note", &["<Control>n"]);
        app.set_accels_for_action("win.sync", &["<Control>r"]);
        app.set_accels_for_action("win.preview", &["<Control>p"]);
        app.set_accels_for_action("win.pin", &["<Control><Alt>p"]);
        app.set_accels_for_action("win.trash", &["<Control><Alt>d"]);
        app.set_accels_for_action("win.info", &["<Control><Alt>i"]);
        app.set_accels_for_action("win.sidebar", &["F9"]);
        app.set_accels_for_action("win.focus-search", &["<Control>f"]);
        app.set_accels_for_action("window.close", &["<Control>w"]);
        app.set_accels_for_action("app.quit", &["<Control>q"]);
        let quit = gio::SimpleAction::new("quit", None);
        {
            let app = app.clone();
            quit.connect_activate(move |_, _| app.quit());
        }
        app.add_action(&quit);

        // ----- sidebar -----
        {
            let ui2 = ui.clone();
            ui.search.connect_search_changed(move |_| ui2.refresh_list());
            let ui2 = ui.clone();
            ui.filter_dd.connect_selected_notify(move |_| {
                if !ui2.st.borrow().busy {
                    ui2.refresh_list();
                }
            });
            let ui2 = ui.clone();
            ui.list.connect_row_selected(move |_, row| {
                if ui2.st.borrow().busy {
                    return;
                }
                let id = row.map(|r| r.widget_name().to_string());
                ui2.select(id);
            });
            let core = ui.core.clone();
            ui.split.connect_show_sidebar_notify(move |s| {
                let collapsed = !s.shows_sidebar();
                let _ = core.update_settings(|st| st.sidebar_collapsed = collapsed);
            });
        }

        // ----- editor -----
        {
            let ui2 = ui.clone();
            ui.buffer.connect_changed(move |_| ui2.schedule_save());
            let ui2 = ui.clone();
            ui.pin_btn.connect_toggled(move |b| {
                if ui2.st.borrow().busy {
                    return;
                }
                if let Some(id) = ui2.current() {
                    let _ = ui2.core.set_pinned(&id, b.is_active());
                    ui2.refresh_list();
                }
            });
            let ui2 = ui.clone();
            ui.trash_btn.connect_clicked(move |_| ui2.trash_or_restore());
            let ui2 = ui.clone();
            ui.preview_btn.connect_toggled(move |b| ui2.set_preview(b.is_active()));
            let ui2 = ui.clone();
            ui.tag_entry.connect_activate(move |e| {
                let text = e.text().trim().trim_start_matches('#').to_string();
                e.set_text("");
                if text.is_empty() {
                    return;
                }
                if let Some(id) = ui2.current() {
                    if let Ok(Some(n)) = ui2.core.get(&id) {
                        let mut tags = n.tags;
                        tags.push(text);
                        let _ = ui2.core.set_tags(&id, tags);
                        ui2.load_meta();
                        ui2.refresh_list();
                    }
                }
            });
            let ui2 = ui.clone();
            adw::StyleManager::default().connect_dark_notify(move |_| {
                ui2.apply_editor_scheme();
                if ui2.st.borrow().preview {
                    ui2.render_preview();
                }
            });
            let ui2 = ui.clone();
            ui.win.connect_close_request(move |_| {
                ui2.save_now();
                glib::Propagation::Proceed
            });
        }
        self.apply_editor_scheme();

        // ----- login -----
        {
            let ui2 = ui.clone();
            open_login.connect_clicked(move |_| {
                gtk::UriLauncher::new(ui2.core.login_url()).launch(Some(&ui2.win), None::<&gio::Cancellable>, |_| {});
            });
            let ui2 = ui.clone();
            ui.login_btn.connect_clicked(move |_| ui2.do_login());
            let ui2 = ui.clone();
            ui.login_entry.connect_activate(move |_| ui2.do_login());
        }

        // ----- core events (arrive on the sync thread) -----
        let (tx, rx) = async_channel::unbounded::<Event>();
        ui.core.set_listener(move |e| {
            let _ = tx.send_blocking(e);
        });
        let ui2 = ui.clone();
        glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                match ev {
                    Event::NotesChanged => ui2.refresh_list(),
                    Event::StatusChanged(s) => ui2.on_status(s),
                }
            }
        });
        ui.core.start_background();
    }

    // ---------------------------------------------------------------- state

    /// Id of the open note. Returns an owned value so no `RefCell` borrow is held
    /// across the callers' `if let` bodies (which mutate state).
    fn current(&self) -> Option<String> {
        self.st.borrow().current.clone()
    }

    fn mode(&self) -> Mode {
        let i = self.filter_dd.selected() as usize;
        let st = self.st.borrow();
        match i {
            0 => Mode::All,
            1 => Mode::Trash,
            n => st.mode_tags.get(n - 2).cloned().map(Mode::Tag).unwrap_or(Mode::All),
        }
    }

    fn refresh_list(self: &Rc<Self>) {
        let prev_mode = self.mode();
        self.st.borrow_mut().busy = true;

        // rebuild the filter drop-down (All / Trash / tags)
        let tags: Vec<String> = self
            .core
            .tags()
            .unwrap_or_default()
            .into_iter()
            .map(|(t, _)| t)
            .collect();
        if tags != self.st.borrow().mode_tags {
            let mut labels = vec!["All Notes".to_string(), "Trash".to_string()];
            labels.extend(tags.iter().map(|t| format!("#{t}")));
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            self.filter_model.splice(0, self.filter_model.n_items(), &refs);
            self.st.borrow_mut().mode_tags = tags.clone();
            let idx = match &prev_mode {
                Mode::All => 0,
                Mode::Trash => 1,
                Mode::Tag(t) => tags.iter().position(|x| x == t).map(|i| i + 2).unwrap_or(0),
            };
            self.filter_dd.set_selected(idx as u32);
        }
        let mode = self.mode();

        let q = self.search.text().to_string();
        let filter = Filter {
            query: Some(q).filter(|q| !q.trim().is_empty()),
            tag: if let Mode::Tag(t) = &mode {
                Some(t.clone())
            } else {
                None
            },
            trash: mode == Mode::Trash,
        };
        let notes = self.core.list(&filter).unwrap_or_default();

        // keep following a note whose id changed after its first upload
        let current = self.current().map(|c| self.core.resolve_id(&c));
        self.st.borrow_mut().current = current.clone();

        while let Some(c) = self.list.first_child() {
            self.list.remove(&c);
        }
        let mut selected_row = None;
        for n in &notes {
            let row = make_row(n);
            self.list.append(&row);
            if current.as_deref() == Some(n.id.as_str()) {
                selected_row = Some(row);
            }
        }
        if let Some(r) = &selected_row {
            self.list.select_row(Some(r));
        }
        self.st.borrow_mut().busy = false;

        let has_current = self.current().is_some();
        let pending = self.st.borrow().pending;
        if has_current && selected_row.is_none() && !pending {
            // note is filtered out or gone: leave it open only if it still exists
            let exists = current
                .as_deref()
                .and_then(|c| self.core.get(c).ok().flatten())
                .is_some();
            if !exists {
                self.select(None);
            }
        } else if let (Some(id), true) = (current, has_current && !pending) {
            // refresh from remote/file edits unless the user is mid-edit
            if let Ok(Some(n)) = self.core.get(&id) {
                if n.content.replace("\r\n", "\n") != self.buffer_text() {
                    self.load_note_text(&n.content);
                }
                self.load_meta();
                let preview = self.st.borrow().preview;
                if preview {
                    self.render_preview();
                }
            }
        }
        if notes.is_empty() && self.current().is_none() {
            self.content.set_visible_child_name("empty");
        }
    }

    fn select(self: &Rc<Self>, id: Option<String>) {
        self.save_now();
        self.st.borrow_mut().current = id.clone();
        let Some(id) = id else {
            self.title.set_title("Simplenotorious");
            self.title.set_subtitle("");
            self.content.set_visible_child_name("empty");
            self.tag_entry.set_sensitive(false);
            self.pin_btn.set_sensitive(false);
            self.trash_btn.set_sensitive(false);
            return;
        };
        let Ok(Some(n)) = self.core.get(&id) else { return };
        self.tag_entry.set_sensitive(true);
        self.pin_btn.set_sensitive(true);
        self.trash_btn.set_sensitive(true);
        self.load_note_text(&n.content);
        self.load_meta();
        if self.st.borrow().preview {
            self.render_preview();
        } else {
            self.content.set_visible_child_name("editor");
        }
    }

    fn buffer_text(&self) -> String {
        self.buffer
            .text(&self.buffer.start_iter(), &self.buffer.end_iter(), false)
            .to_string()
    }

    fn load_note_text(&self, content: &str) {
        self.st.borrow_mut().busy = true;
        self.buffer.set_enable_undo(false);
        self.buffer.set_text(&content.replace("\r\n", "\n"));
        self.buffer.set_enable_undo(true);
        self.buffer.place_cursor(&self.buffer.start_iter());
        self.st.borrow_mut().busy = false;
    }

    /// Title, pin state, trash button, tag chips, info popover.
    fn load_meta(self: &Rc<Self>) {
        let Some(id) = self.current() else { return };
        let Ok(Some(n)) = self.core.get(&id) else { return };
        self.st.borrow_mut().busy = true;
        self.title.set_title(&n.title());
        self.title.set_subtitle(if n.deleted { "In Trash" } else { "" });
        self.pin_btn.set_active(n.pinned);
        self.view.set_editable(!n.deleted);
        self.trash_btn.set_icon_name(if n.deleted {
            "edit-undo-symbolic"
        } else {
            "user-trash-symbolic"
        });
        self.trash_btn
            .set_tooltip_text(Some(if n.deleted {
                "Restore note (Ctrl+Alt+D)"
            } else {
                "Move to trash (Ctrl+Alt+D)"
            }));
        let words = n.content.split_whitespace().count();
        self.info_lbl.set_label(&format!(
            "Created: {}\nModified: {}\n{} words · {} characters",
            fmt_time(n.created),
            fmt_time(n.modified),
            words,
            n.content.chars().count()
        ));
        while let Some(c) = self.tag_box.first_child() {
            self.tag_box.remove(&c);
        }
        for t in &n.tags {
            let b = gtk::Button::with_label(t);
            b.add_css_class("tag-chip");
            b.add_css_class("flat");
            b.set_tooltip_text(Some("Click to remove this tag"));
            let (ui, tag, id) = (self.clone(), t.clone(), id.clone());
            b.connect_clicked(move |_| {
                if let Ok(Some(n)) = ui.core.get(&id) {
                    let tags = n.tags.into_iter().filter(|x| *x != tag).collect();
                    let _ = ui.core.set_tags(&id, tags);
                    ui.load_meta();
                    ui.refresh_list();
                }
            });
            self.tag_box.append(&b);
        }
        self.st.borrow_mut().busy = false;
    }

    fn schedule_save(self: &Rc<Self>) {
        if self.st.borrow().busy {
            return;
        }
        let gen_ = {
            let mut st = self.st.borrow_mut();
            st.save_gen += 1;
            st.pending = true;
            st.save_gen
        };
        let ui = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if ui.st.borrow().save_gen == gen_ {
                ui.save_now();
            }
        });
    }

    fn save_now(self: &Rc<Self>) {
        if !self.st.borrow().pending {
            return;
        }
        self.st.borrow_mut().pending = false;
        let Some(id) = self.current() else { return };
        match self.core.update_content(&id, &self.buffer_text()) {
            Ok(_) => {
                self.load_meta();
                self.refresh_list();
            }
            Err(e) => self.toast(&format!("Could not save note: {e}")),
        }
    }

    fn new_note(self: &Rc<Self>) {
        self.save_now();
        self.search.set_text("");
        self.filter_dd.set_selected(0);
        match self.core.create_note() {
            Ok(n) => {
                self.set_preview_off();
                self.refresh_list();
                self.select(Some(n.id.clone()));
                self.refresh_list();
                self.content.set_visible_child_name("editor");
                self.view.grab_focus();
            }
            Err(e) => self.toast(&format!("Could not create note: {e}")),
        }
    }

    fn trash_or_restore(self: &Rc<Self>) {
        self.save_now();
        let Some(id) = self.current() else { return };
        let deleted = self.core.get(&id).ok().flatten().map(|n| n.deleted).unwrap_or(false);
        let r = if deleted {
            self.core.restore(&id)
        } else {
            self.core.trash(&id)
        };
        match r {
            Ok(()) => {
                // An empty, never-synced note is discarded rather than trashed.
                let discarded = !deleted && self.core.get(&id).ok().flatten().is_none();
                self.toast(if deleted {
                    "Note restored"
                } else if discarded {
                    "Empty note deleted"
                } else {
                    "Note moved to trash"
                });
                self.select(None);
                self.refresh_list();
            }
            Err(e) => self.toast(&format!("Error: {e}")),
        }
    }

    // -------------------------------------------------------------- preview

    fn set_preview_off(&self) {
        if self.preview_btn.is_active() {
            self.preview_btn.set_active(false);
        }
    }

    fn set_preview(self: &Rc<Self>, on: bool) {
        self.save_now();
        self.st.borrow_mut().preview = on;
        if self.st.borrow().current.is_none() {
            return;
        }
        if on {
            self.render_preview();
        } else {
            self.content.set_visible_child_name("editor");
            self.view.grab_focus();
        }
    }

    fn render_preview(&self) {
        let Some(id) = self.current() else { return };
        let dark = adw::StyleManager::default().is_dark();
        if let Ok(html) = self.core.render_preview(&id, dark) {
            self.webview.load_html(&html, None);
            self.content.set_visible_child_name("preview");
        }
    }

    fn apply_editor_scheme(&self) {
        let dark = adw::StyleManager::default().is_dark();
        let mgr = sourceview5::StyleSchemeManager::default();
        let scheme = if dark {
            mgr.scheme("Adwaita-dark")
        } else {
            mgr.scheme("Adwaita")
        };
        self.buffer.set_style_scheme(scheme.as_ref());
    }

    // --------------------------------------------------------------- status

    fn on_status(self: &Rc<Self>, s: SyncStatus) {
        self.st.borrow_mut().status = Some(s.clone());
        self.banner
            .set_revealed(s == SyncStatus::Offline && self.core.is_logged_in());
        let text = match &s {
            SyncStatus::LoggedOut => "Not logged in".to_string(),
            SyncStatus::Idle => "Ready".to_string(),
            SyncStatus::Syncing => "Syncing…".to_string(),
            SyncStatus::Synced => format!("Synced {}", self.core.last_sync_ms().map(fmt_clock).unwrap_or_default()),
            SyncStatus::Offline => "Offline — changes saved locally".to_string(),
            SyncStatus::Error(m) => format!("Sync problem: {m}"),
        };
        self.status_lbl.set_label(&text);
        self.status_lbl.set_tooltip_text(Some(&text));
        self.status_lbl.set_ellipsize(gtk::pango::EllipsizeMode::End);
        match s {
            SyncStatus::LoggedOut => self.root.set_visible_child_name("login"),
            _ => {
                if self.root.visible_child_name().as_deref() == Some("login") && self.core.is_logged_in() {
                    self.root.set_visible_child_name("main");
                    self.refresh_list();
                }
            }
        }
        if let SyncStatus::Error(m) = &s {
            self.toast(m);
        }
    }

    fn toast(&self, msg: &str) {
        self.toasts.add_toast(adw::Toast::new(msg));
    }

    // ------------------------------------------------------------- account

    fn do_login(self: &Rc<Self>) {
        let cred = self.login_entry.text().trim().to_string();
        if cred.is_empty() {
            return;
        }
        self.login_btn.set_sensitive(false);
        self.login_err.set_visible(false);
        let ui = self.clone();
        let core = self.core.clone();
        glib::spawn_future_local(async move {
            let res = gio::spawn_blocking(move || core.login(&cred).map_err(|e| e.to_string())).await;
            ui.login_btn.set_sensitive(true);
            match res {
                Ok(Ok(())) => {
                    ui.login_entry.set_text("");
                    ui.root.set_visible_child_name("main");
                    ui.refresh_list();
                }
                Ok(Err(m)) => {
                    ui.login_err.set_label(&format!("Login failed: {m}"));
                    ui.login_err.set_visible(true);
                }
                Err(_) => {
                    ui.login_err.set_label("Login failed unexpectedly.");
                    ui.login_err.set_visible(true);
                }
            }
        });
    }

    fn confirm_logout(self: &Rc<Self>) {
        self.save_now();
        let d = adw::AlertDialog::new(
            Some("Log out of SimpleNote?"),
            Some(
                "Your notes stay in your notes folder unless you also remove the local copy. Notes not yet synced would be lost if removed.",
            ),
        );
        d.add_responses(&[
            ("cancel", "Cancel"),
            ("keep", "Log Out"),
            ("wipe", "Log Out and Remove Local Notes"),
        ]);
        d.set_response_appearance("wipe", adw::ResponseAppearance::Destructive);
        d.set_default_response(Some("cancel"));
        d.set_close_response("cancel");
        let ui = self.clone();
        d.choose(Some(&self.win), None::<&gio::Cancellable>, move |resp| {
            let wipe = match resp.as_str() {
                "keep" => false,
                "wipe" => true,
                _ => return,
            };
            match ui.core.logout(wipe) {
                Ok(()) => {
                    ui.select(None);
                    ui.refresh_list();
                    ui.root.set_visible_child_name("login");
                }
                Err(e) => ui.toast(&format!("Log out failed: {e}")),
            }
        });
    }

    fn import_json(self: &Rc<Self>) {
        let d = gtk::FileDialog::builder().title("Import notes.json").build();
        let ui = self.clone();
        d.open(Some(&self.win), None::<&gio::Cancellable>, move |r| {
            let Some(p) = r.ok().and_then(|f| f.path()) else { return };
            match std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|s| ui.core.import_json(&s).map_err(|e| e.to_string()))
            {
                Ok(n) => {
                    ui.toast(&format!("Imported {n} notes"));
                    ui.refresh_list();
                }
                Err(e) => ui.toast(&format!("Import failed: {e}")),
            }
        });
    }

    fn export_json(self: &Rc<Self>) {
        self.save_now();
        let d = gtk::FileDialog::builder()
            .title("Export notes.json")
            .initial_name("notes.json")
            .build();
        let ui = self.clone();
        d.save(Some(&self.win), None::<&gio::Cancellable>, move |r| {
            let Some(p) = r.ok().and_then(|f| f.path()) else { return };
            match ui.core.export_json_to(&p) {
                Ok(()) => ui.toast("Exported notes.json"),
                Err(e) => ui.toast(&format!("Export failed: {e}")),
            }
        });
    }
}

fn make_row(n: &NoteSummary) -> gtk::ListBoxRow {
    let vb = gtk::Box::new(gtk::Orientation::Vertical, 2);
    vb.set_margin_top(8);
    vb.set_margin_bottom(8);
    vb.set_margin_start(6);
    vb.set_margin_end(6);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    if n.pinned {
        let pin = gtk::Image::from_icon_name("view-pin-symbolic");
        pin.add_css_class("accent");
        top.append(&pin);
    }
    let t = gtk::Label::builder()
        .label(&n.title)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();
    t.add_css_class("note-title");
    top.append(&t);
    vb.append(&top);
    if !n.snippet.is_empty() {
        let s = gtk::Label::builder()
            .label(&n.snippet)
            .xalign(0.0)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .lines(2)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        s.add_css_class("note-snippet");
        vb.append(&s);
    }
    let row = gtk::ListBoxRow::new();
    row.set_child(Some(&vb));
    row.set_widget_name(&n.id);
    row
}

fn fmt_time(ms: i64) -> String {
    glib::DateTime::from_unix_local(ms / 1000)
        .ok()
        .and_then(|d| d.format("%b %e, %Y %l:%M %p").ok())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

fn fmt_clock(ms: i64) -> String {
    glib::DateTime::from_unix_local(ms / 1000)
        .ok()
        .and_then(|d| d.format("%l:%M %p").ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// GNOME-style keyboard shortcuts overlay (Ctrl+?).
fn show_shortcuts(parent: &adw::ApplicationWindow) {
    let dialog = adw::ShortcutsDialog::new();
    let section = |title: &str, items: &[(&str, &str)]| {
        let s = adw::ShortcutsSection::new(Some(title));
        for (t, a) in items {
            s.add(adw::ShortcutsItem::new(t, a));
        }
        dialog.add(s);
    };
    section(
        "Notes",
        &[
            ("New note", "<Control>n"),
            ("Pin note", "<Control><Alt>p"),
            ("Move to trash / restore note", "<Control><Alt>d"),
            ("Note information", "<Control><Alt>i"),
            ("Toggle Markdown preview", "<Control>p"),
        ],
    );
    section(
        "Navigation",
        &[("Search notes", "<Control>f"), ("Toggle sidebar", "F9"), ("Sync now", "<Control>r")],
    );
    section(
        "General",
        &[
            ("Preferences", "<Control>comma"),
            ("Keyboard shortcuts", "<Control>question"),
            ("Close window", "<Control>w"),
            ("Quit", "<Control>q"),
        ],
    );
    dialog.present(Some(parent));
}
