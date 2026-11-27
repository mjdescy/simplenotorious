import AppKit
import SwiftUI

enum NoteFilter: Hashable {
    case all, trash, tag(String)
}

@MainActor
final class AppModel: ObservableObject {
    let core: CoreClient?

    @Published var notes: [NoteSummary] = []
    @Published var tags: [String] = []
    @Published var filter: NoteFilter = .all { didSet { refresh() } }
    @Published var query = "" { didSet { refresh() } }
    @Published var selection: String? { didSet { if !suppressSelection { selectionChanged() } } }
    /// Editor buffer; only changed by the user through `textBinding`.
    @Published private(set) var text = ""
    @Published private(set) var openNote: Note?
    @Published var showPreview = false { didSet { if showPreview { saveNow() } } }
    @Published var status: SyncState = .idle
    @Published var loggedIn = false
    @Published var settings = AppSettings()
    @Published var loginError: String?
    @Published var loggingIn = false
    @Published var toast: String?
    @Published var confirmTrash = false
    @Published var columnVisibility: NavigationSplitViewVisibility = .all

    private var openID: String?
    private var suppressSelection = false
    private var pendingSave = false
    private var saveGeneration = 0

    init() {
        core = CoreClient()
        guard let core else {
            toast = "Could not open the notes database."
            return
        }
        core.setEventHandler { [weak self] event in
            DispatchQueue.main.async { self?.handle(event) }
        }
        loadSettings()
        loggedIn = (try? core.call("is_logged_in", as: Bool.self)) ?? false
        status = (try? core.call("status", as: SyncState.self)) ?? .idle
        try? core.run("start_background")
        refresh()
    }

    // MARK: events / status

    private func handle(_ e: [String: Any]) {
        switch e["event"] as? String {
        case "notes_changed":
            refresh()
        case "status":
            if let s = e["status"] as? [String: Any], let st = s["state"] as? String {
                status = SyncState(state: st, message: s["message"] as? String)
                loggedIn = st != "logged_out"
                if st == "error", let m = status.message { toast = m }
            }
        default: break
        }
    }

    var statusText: String {
        switch status.state {
        case "logged_out": return "Not logged in"
        case "syncing": return "Syncing…"
        case "synced":
            if let ms = lastSyncMs {
                let d = Date(timeIntervalSince1970: Double(ms) / 1000)
                return "Synced " + d.formatted(date: .omitted, time: .shortened)
            }
            return "Synced"
        case "offline": return "Offline — changes saved locally"
        case "error": return "Sync problem: " + (status.message ?? "")
        default: return "Ready"
        }
    }

    private var lastSyncMs: Int64? {
        guard let core else { return nil }
        return (try? core.call("last_sync", as: Int64?.self)) ?? nil
    }

    var isOffline: Bool { status.state == "offline" && loggedIn }

    // MARK: settings

    func loadSettings() {
        if let s = try? core?.call("settings", as: AppSettings.self) { settings = s }
    }

    func updateSetting(_ patch: [String: Any]) {
        if let s = try? core?.call("update_settings", patch, as: AppSettings.self) { settings = s }
    }

    var colorScheme: ColorScheme? {
        switch settings.theme {
        case .system: return nil
        case .light: return .light
        case .dark: return .dark
        }
    }

    // MARK: account

    var loginURL: URL? {
        (try? core?.call("login_url", as: String.self)).flatMap(URL.init(string:))
    }

    func login(credential: String) {
        guard let core, !credential.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        loggingIn = true
        loginError = nil
        Task.detached {
            let result = Result { try core.run("login", ["credential": credential.trimmingCharacters(in: .whitespacesAndNewlines)]) }
            await MainActor.run {
                self.loggingIn = false
                switch result {
                case .success:
                    self.loggedIn = true
                    self.refresh()
                case .failure(let e):
                    self.loginError = "Login failed: \(e.localizedDescription)"
                }
            }
        }
    }

    func logout(deleteLocal: Bool) {
        saveNow()
        do {
            try core?.run("logout", ["delete_local_notes": deleteLocal])
            loggedIn = false
            selection = nil
            refresh()
        } catch { toast = "Log out failed: \(error.localizedDescription)" }
    }

    func syncNow() { try? core?.run("sync_now") }

    // MARK: list

    func refresh() {
        guard let core else { return }
        var args: [String: Any] = ["query": query]
        switch filter {
        case .all: break
        case .trash: args["trash"] = true
        case .tag(let t): args["tag"] = t
        }
        notes = (try? core.call("list", args, as: [NoteSummary].self)) ?? []
        let allTags = (try? core.call("tags", as: [[AnyDecodable]].self)) ?? []
        tags = allTags.compactMap { $0.first?.string }
        if case .tag(let t) = filter, !tags.contains(t) { filter = .all }

        // follow a note whose id changed after its first upload; pick up remote edits
        if let id = openID, let n = try? core.call("get", ["id": id], as: Note?.self) {
            if n.id != id {
                openID = n.id
                setSelection(n.id)
            }
            if !pendingSave, n.content != openNote?.content || n.content.replacingOccurrences(of: "\r\n", with: "\n") != text {
                text = n.content.replacingOccurrences(of: "\r\n", with: "\n")
            }
            openNote = n
        } else if openID != nil {
            openID = nil
            openNote = nil
            text = ""
            setSelection(nil)
        }
    }

    private func setSelection(_ id: String?) {
        suppressSelection = true
        selection = id
        suppressSelection = false
    }

    // MARK: editing

    private func selectionChanged() {
        saveNow()
        guard let id = selection, let core,
              let n = try? core.call("get", ["id": id], as: Note?.self) else {
            openID = nil; openNote = nil; text = ""
            return
        }
        openID = n.id
        openNote = n
        text = n.content.replacingOccurrences(of: "\r\n", with: "\n")
    }

    var textBinding: Binding<String> {
        Binding(
            get: { self.text },
            set: { new in
                guard new != self.text else { return }
                self.text = new
                self.scheduleSave()
            }
        )
    }

    private func scheduleSave() {
        pendingSave = true
        saveGeneration += 1
        let gen = saveGeneration
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 500_000_000)
            if gen == self.saveGeneration { self.saveNow() }
        }
    }

    func saveNow() {
        guard pendingSave, let id = openID, let core else { return }
        pendingSave = false
        do {
            openNote = try core.call("update_content", ["id": id, "content": text], as: Note.self)
            refresh()
        } catch { toast = "Could not save note: \(error.localizedDescription)" }
    }

    func newNote() {
        saveNow()
        query = ""
        filter = .all
        showPreview = false
        if let n = try? core?.call("create_note", as: Note.self) {
            refresh()
            selection = n.id
        }
    }

    func togglePin() {
        guard let n = openNote else { return }
        openNote = try? core?.call("set_pinned", ["id": n.id, "pinned": !n.pinned], as: Note.self)
        refresh()
    }

    /// Asks for confirmation first (when enabled) before moving a note to the trash.
    func requestTrashOrRestore() {
        guard let n = openNote else { return }
        if !n.deleted && settings.confirmTrash {
            confirmTrash = true
        } else {
            trashOrRestore()
        }
    }

    func trashOrRestore() {
        saveNow()
        guard let n = openNote else { return }
        do {
            try core?.run(n.deleted ? "restore" : "trash", ["id": n.id])
            if n.deleted {
                toast = "Note restored"
            } else {
                // An empty, never-synced note is discarded rather than trashed.
                let remaining = try? core?.call("get", ["id": n.id], as: Note?.self)
                toast = (remaining ?? nil) == nil ? "Empty note deleted" : "Note moved to trash"
            }
            setSelection(nil)
            openID = nil; openNote = nil; text = ""
            refresh()
        } catch { toast = error.localizedDescription }
    }

    func setTags(_ tags: [String]) {
        guard let n = openNote else { return }
        openNote = try? core?.call("set_tags", ["id": n.id, "tags": tags], as: Note.self)
        refresh()
    }

    func previewHTML(dark: Bool) -> String {
        guard let id = openID else { return "" }
        return (try? core?.call("preview", ["id": id, "dark": dark], as: String.self)) ?? ""
    }

    // MARK: import / export

    func importJSON() {
        let p = NSOpenPanel()
        p.allowedContentTypes = [.json]
        guard p.runModal() == .OK, let url = p.url, let s = try? String(contentsOf: url, encoding: .utf8) else { return }
        do {
            let n = try core?.call("import_json", ["json": s], as: Int.self) ?? 0
            toast = "Imported \(n) notes"
            refresh()
        } catch { toast = "Import failed: \(error.localizedDescription)" }
    }

    func exportJSON() {
        saveNow()
        let p = NSSavePanel()
        p.nameFieldStringValue = "notes.json"
        guard p.runModal() == .OK, let url = p.url else { return }
        do {
            try core?.run("export_json", ["path": url.path])
            toast = "Exported notes.json"
        } catch { toast = "Export failed: \(error.localizedDescription)" }
    }
}

/// Decodes the `[(String, usize)]` tag/count tuples without caring about the count.
struct AnyDecodable: Decodable {
    let string: String?
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        string = try? c.decode(String.self)
    }
}
