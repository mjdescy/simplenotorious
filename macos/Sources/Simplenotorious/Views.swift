import SwiftUI
import WebKit

// MARK: - Root

struct RootView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        Group {
            if model.loggedIn {
                MainView()
            } else {
                LoginView()
            }
        }
        .preferredColorScheme(model.colorScheme)
        .frame(minWidth: 760, minHeight: 480)
        .overlay(alignment: .bottom) { ToastView() }
    }
}

struct ToastView: View {
    @EnvironmentObject var model: AppModel
    var body: some View {
        if let t = model.toast {
            Text(t)
                .padding(.horizontal, 14).padding(.vertical, 8)
                .background(.regularMaterial, in: Capsule())
                .padding(.bottom, 40)
                .task(id: t) {
                    try? await Task.sleep(nanoseconds: 3_000_000_000)
                    model.toast = nil
                }
                .transition(.opacity)
        }
    }
}

// MARK: - First run / login

struct LoginView: View {
    @EnvironmentObject var model: AppModel
    @State private var credential = ""

    var body: some View {
        VStack(spacing: 18) {
            Image(systemName: "note.text").font(.system(size: 54)).foregroundStyle(.tint)
            Text("Welcome to Simplenotorious").font(.largeTitle.bold())
            Text("Log in to SimpleNote to view and sync your notes.").foregroundStyle(.secondary)
            VStack(spacing: 12) {
                Button("Open SimpleNote Login Page") {
                    if let u = model.loginURL { NSWorkspace.shared.open(u) }
                }
                .buttonStyle(.borderedProminent).controlSize(.large)
                Text("After signing in, paste the code shown (it looks like you@example.com:ABC123).")
                    .font(.callout).foregroundStyle(.secondary).multilineTextAlignment(.center)
                TextField("email:CODE", text: $credential)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit { model.login(credential: credential) }
                Button(model.loggingIn ? "Logging in…" : "Log In") { model.login(credential: credential) }
                    .disabled(model.loggingIn || credential.isEmpty)
                    .controlSize(.large)
                if let e = model.loginError { Text(e).foregroundStyle(.red).font(.callout) }
            }
            .frame(maxWidth: 380)
        }
        .padding(40)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Main window

struct MainView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(spacing: 0) {
            if model.isOffline {
                Label("You're offline. Changes are saved on this Mac and will sync when the connection returns.", systemImage: "wifi.slash")
                    .font(.callout).padding(8).frame(maxWidth: .infinity)
                    .background(Color.orange.opacity(0.2))
            }
            NavigationSplitView(columnVisibility: $model.columnVisibility) {
                SidebarView()
                    .navigationSplitViewColumnWidth(min: 240, ideal: 290, max: 400)
            } detail: {
                DetailView()
            }
        }
        .navigationTitle(model.notes.first { $0.id == model.selection }?.title ?? "Simplenotorious")
    }
}

struct SidebarView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        VStack(spacing: 0) {
            List(selection: $model.selection) {
                ForEach(model.notes) { n in
                    NoteRow(note: n).tag(n.id)
                }
            }
            .listStyle(.sidebar)
            Divider()
            HStack {
                Text(model.statusText).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 12).padding(.vertical, 6)
        }
        .searchable(text: $model.query, placement: .sidebar, prompt: "Search")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Menu {
                    Picker("Show", selection: $model.filter) {
                        Text("All Notes").tag(NoteFilter.all)
                        Text("Trash").tag(NoteFilter.trash)
                        if !model.tags.isEmpty { Divider() }
                        ForEach(model.tags, id: \.self) { Text("#\($0)").tag(NoteFilter.tag($0)) }
                    }
                    .pickerStyle(.inline)
                } label: { Image(systemName: "line.3.horizontal.decrease.circle") }
                .help("Filter notes")
            }
            if model.columnVisibility != .detailOnly {
                ToolbarItem(placement: .automatic) { NewNoteButton() }
            }
        }
    }
}

/// Shown in the sidebar toolbar, or in the detail toolbar while the sidebar is hidden.
struct NewNoteButton: View {
    @EnvironmentObject var model: AppModel
    var body: some View {
        Button { model.newNote() } label: { Image(systemName: "square.and.pencil") }
            .help("New note (⌘N)")
    }
}

struct NoteRow: View {
    let note: NoteSummary
    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 4) {
                if note.pinned { Image(systemName: "pin.fill").font(.caption).foregroundStyle(.tint) }
                Text(note.title).fontWeight(.semibold).lineLimit(1)
            }
            if !note.snippet.isEmpty {
                Text(note.snippet).font(.callout).foregroundStyle(.secondary).lineLimit(2)
            }
        }
        .padding(.vertical, 3)
    }
}

struct DetailView: View {
    @EnvironmentObject var model: AppModel
    @Environment(\.colorScheme) private var scheme
    @State private var newTag = ""
    @State private var showInfo = false

    var body: some View {
        Group {
            if let note = model.openNote {
                VStack(spacing: 0) {
                    if model.showPreview {
                        PreviewView(html: model.previewHTML(dark: scheme == .dark))
                    } else {
                        TextEditor(text: model.textBinding)
                            .font(.system(size: 15))
                            .scrollContentBackground(.hidden)
                            .padding(.horizontal, 30).padding(.vertical, 16)
                            .disabled(note.deleted)
                    }
                    Divider()
                    TagBar(note: note, newTag: $newTag)
                }
            } else {
                ContentUnavailableView("No Note Selected", systemImage: "note.text",
                                       description: Text("Select a note or press ⌘N to create one."))
            }
        }
        .toolbar {
            if model.columnVisibility == .detailOnly {
                ToolbarItem(placement: .navigation) { NewNoteButton() }
            }
            ToolbarItemGroup(placement: .primaryAction) {
                Button { model.togglePin() } label: {
                    Image(systemName: model.openNote?.pinned == true ? "pin.fill" : "pin")
                }.help("Pin note (⌃⌥P)").keyboardShortcut("p", modifiers: [.control, .option]).disabled(model.openNote == nil)
                Button { model.trashOrRestore() } label: {
                    Image(systemName: model.openNote?.deleted == true ? "arrow.uturn.backward" : "trash")
                }.help(model.openNote?.deleted == true ? "Restore note (⌃⌥D)" : "Move to trash (⌃⌥D)")
                    .keyboardShortcut("d", modifiers: [.control, .option]).disabled(model.openNote == nil)
                Button { showInfo.toggle() } label: { Image(systemName: "info.circle") }
                    .help("Note information (⌃⌥I)").keyboardShortcut("i", modifiers: [.control, .option]).disabled(model.openNote == nil)
                    .popover(isPresented: $showInfo) { InfoView(note: model.openNote, text: model.text).padding() }
                Toggle(isOn: $model.showPreview) { Image(systemName: "eye") }
                    .toggleStyle(.button).help("Toggle Markdown preview (⌘P)").disabled(model.openNote == nil)
            }
        }
    }
}

struct InfoView: View {
    let note: Note?
    let text: String
    var body: some View {
        if let n = note {
            VStack(alignment: .leading, spacing: 4) {
                Text("Created: " + Date(timeIntervalSince1970: Double(n.created) / 1000).formatted(date: .abbreviated, time: .shortened))
                Text("Modified: " + Date(timeIntervalSince1970: Double(n.modified) / 1000).formatted(date: .abbreviated, time: .shortened))
                Text("\(text.split { $0.isWhitespace || $0.isNewline }.count) words · \(text.count) characters")
            }
        }
    }
}

struct TagBar: View {
    @EnvironmentObject var model: AppModel
    let note: Note
    @Binding var newTag: String

    var body: some View {
        HStack(spacing: 6) {
            ForEach(note.tags, id: \.self) { t in
                Button {
                    model.setTags(note.tags.filter { $0 != t })
                } label: {
                    Text(t).padding(.horizontal, 10).padding(.vertical, 2)
                        .background(Color.accentColor.opacity(0.2), in: RoundedRectangle(cornerRadius: 6))
                }
                .buttonStyle(.plain).foregroundStyle(Color.accentColor).help("Click to remove this tag")
            }
            TextField("Add tag…", text: $newTag)
                .textFieldStyle(.plain).frame(width: 110)
                .onSubmit {
                    let t = newTag.trimmingCharacters(in: CharacterSet(charactersIn: "# ").union(.whitespaces))
                    newTag = ""
                    if !t.isEmpty { model.setTags(note.tags + [t]) }
                }
            Spacer()
        }
        .padding(.horizontal, 16).padding(.vertical, 8)
    }
}

// MARK: - Read-only HTML preview (WKWebView, JavaScript off)

struct PreviewView: NSViewRepresentable {
    let html: String

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let cfg = WKWebViewConfiguration()
        cfg.defaultWebpagePreferences.allowsContentJavaScript = false
        let v = WKWebView(frame: .zero, configuration: cfg)
        v.navigationDelegate = context.coordinator
        v.setValue(false, forKey: "drawsBackground")
        return v
    }

    func updateNSView(_ v: WKWebView, context: Context) {
        if context.coordinator.lastHTML != html {
            context.coordinator.lastHTML = html
            v.loadHTMLString(html, baseURL: nil)
        }
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var lastHTML = ""
        func webView(_ w: WKWebView, decidePolicyFor action: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            if action.navigationType == .linkActivated, let url = action.request.url {
                NSWorkspace.shared.open(url)
                decisionHandler(.cancel)
            } else {
                decisionHandler(.allow)
            }
        }
    }
}

// MARK: - Settings (⌘,)

struct SettingsView: View {
    @EnvironmentObject var model: AppModel
    @State private var confirmLogout = false

    var body: some View {
        Form {
            Section("Appearance") {
                Picker("Color scheme", selection: Binding(
                    get: { model.settings.theme },
                    set: { model.updateSetting(["theme": $0.rawValue]) })) {
                    ForEach(ThemeSetting.allCases) { Text($0.label).tag($0) }
                }
                .pickerStyle(.segmented)
            }
            Section("Markdown") {
                Picker("Italic marker", selection: Binding(
                    get: { model.settings.italicMarker },
                    set: { model.updateSetting(["italic_marker": $0.rawValue]) })) {
                    Text("*text*").tag(EmphasisMarker.asterisk)
                    Text("_text_").tag(EmphasisMarker.underscore)
                }
                Picker("Bold marker", selection: Binding(
                    get: { model.settings.boldMarker },
                    set: { model.updateSetting(["bold_marker": $0.rawValue]) })) {
                    Text("**text**").tag(EmphasisMarker.asterisk)
                    Text("__text__").tag(EmphasisMarker.underscore)
                }
                Text("Characters used when formatting text as italic or bold.").font(.caption).foregroundStyle(.secondary)
            }
            Section("SimpleNote account") {
                HStack {
                    Text(model.loggedIn ? "Logged in" : "Not logged in")
                    Spacer()
                    Button("Sync Now") { model.syncNow() }.disabled(!model.loggedIn)
                    Button("Log Out…", role: .destructive) { confirmLogout = true }.disabled(!model.loggedIn)
                }
                Text(model.statusText).font(.caption).foregroundStyle(.secondary)
            }
            Section("Storage") {
                LabeledContent("Notes folder") {
                    HStack {
                        Text(model.settings.notesFolder ?? "Default (app support folder)").lineLimit(1).truncationMode(.middle)
                        Button("Choose…") { chooseFolder() }
                    }
                }
                Text("Notes are kept as plain .md files so other tools can sync them.").font(.caption).foregroundStyle(.secondary)
                Toggle("Keep notes.json in sync", isOn: Binding(
                    get: { model.settings.jsonSyncEnabled },
                    set: { model.updateSetting(["json_sync_enabled": $0]) }))
                LabeledContent("notes.json location") {
                    HStack {
                        Text(model.settings.jsonPath ?? "Default (app support folder)").lineLimit(1).truncationMode(.middle)
                        Button("Choose…") { chooseJSON() }
                    }
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 520)
        .padding()
        .preferredColorScheme(model.colorScheme)
        .confirmationDialog("Log out of SimpleNote?", isPresented: $confirmLogout) {
            Button("Log Out") { model.logout(deleteLocal: false) }
            Button("Log Out and Remove Local Notes", role: .destructive) { model.logout(deleteLocal: true) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Notes stay in your notes folder unless you also remove the local copy. Notes not yet synced would be lost if removed.")
        }
    }

    private func chooseFolder() {
        let p = NSOpenPanel()
        p.canChooseDirectories = true; p.canChooseFiles = false; p.canCreateDirectories = true
        if p.runModal() == .OK, let url = p.url {
            // adopts existing .md files instead of trashing/duplicating notes
            model.updateSetting(["notes_folder": url.path])
            model.syncNow()
        }
    }

    private func chooseJSON() {
        let p = NSSavePanel()
        p.nameFieldStringValue = "notes.json"
        if p.runModal() == .OK, let url = p.url { model.updateSetting(["json_path": url.path]) }
    }
}
