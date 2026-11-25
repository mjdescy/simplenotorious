import SwiftUI

@main
struct SimplenotoriousApp: App {
    @StateObject private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environmentObject(model)
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    model.saveNow()
                }
        }
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("New Note") { model.newNote() }.keyboardShortcut("n")
            }
            CommandGroup(replacing: .printItem) {
                Button(model.showPreview ? "Edit Note" : "Preview Note") { model.showPreview.toggle() }
                    .keyboardShortcut("p")
                Divider()
                Button("Sync Now") { model.syncNow() }.keyboardShortcut("r")
                Button("Import notes.json…") { model.importJSON() }
                Button("Export notes.json…") { model.exportJSON() }
            }
        }

        // Settings scene: macOS binds ⌘, to it automatically.
        Settings {
            SettingsView().environmentObject(model)
        }
    }
}
