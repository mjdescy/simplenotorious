import Foundation
import CSimplenotorious

struct CoreError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

// MARK: - Models (mirror the Rust JSON, snake_case decoded automatically)

struct NoteSummary: Codable, Identifiable, Hashable {
    let id: String
    let title: String
    let snippet: String
    let pinned: Bool
    let deleted: Bool
    let modified: Int64
    let tags: [String]
}

struct Note: Codable, Identifiable {
    let id: String
    var content: String
    let created: Int64
    let modified: Int64
    var pinned: Bool
    var markdown: Bool
    var tags: [String]
    var deleted: Bool
    let version: UInt64?
    let dirty: Bool
}

struct SyncState: Codable, Equatable {
    var state: String
    var message: String?
    static let loggedOut = SyncState(state: "logged_out", message: nil)
    static let idle = SyncState(state: "idle", message: nil)
}

enum ThemeSetting: String, Codable, CaseIterable, Identifiable {
    case system, light, dark
    var id: String { rawValue }
    var label: String {
        switch self {
        case .system: return "Follow System"
        case .light: return "Light"
        case .dark: return "Dark"
        }
    }
}

enum EmphasisMarker: String, Codable, CaseIterable, Identifiable {
    case asterisk, underscore
    var id: String { rawValue }
}

struct AppSettings: Codable, Equatable {
    var theme: ThemeSetting = .system
    var notesFolder: String? = nil
    var jsonSyncEnabled: Bool = false
    var jsonPath: String? = nil
    var sidebarCollapsed: Bool = false
    var syncIntervalSecs: Int = 60
    var italicMarker: EmphasisMarker = .asterisk
    var boldMarker: EmphasisMarker = .underscore
}

// MARK: - Bridge to the Rust core

/// Thin, thread-safe wrapper over the C ABI (JSON in, JSON out).
final class CoreClient: @unchecked Sendable {
    private let handle: OpaquePointer
    private var onEvent: (([String: Any]) -> Void)?

    init?() {
        guard let h = sn_open("{}") else { return nil }
        handle = h
    }

    deinit { sn_close(handle) }

    /// Events arrive on a background thread.
    func setEventHandler(_ f: @escaping ([String: Any]) -> Void) {
        onEvent = f
        let user = Unmanaged.passUnretained(self).toOpaque()
        sn_set_callback(handle, { json, user in
            guard let json, let user else { return }
            let me = Unmanaged<CoreClient>.fromOpaque(user).takeUnretainedValue()
            if let obj = try? JSONSerialization.jsonObject(with: Data(String(cString: json).utf8)) as? [String: Any] {
                me.onEvent?(obj)
            }
        }, user)
    }

    private func raw(_ method: String, _ args: [String: Any]) throws -> Any {
        let data = try JSONSerialization.data(withJSONObject: args)
        guard let p = sn_call(handle, method, String(decoding: data, as: UTF8.self)) else {
            throw CoreError(message: "core returned no response")
        }
        defer { sn_free_string(p) }
        let reply = try JSONSerialization.jsonObject(with: Data(String(cString: p).utf8)) as? [String: Any] ?? [:]
        if let err = reply["error"] as? String { throw CoreError(message: err) }
        return reply["ok"] ?? NSNull()
    }

    func call<T: Decodable>(_ method: String, _ args: [String: Any] = [:], as type: T.Type = T.self) throws -> T {
        let ok = try raw(method, args)
        let data = try JSONSerialization.data(withJSONObject: ok, options: .fragmentsAllowed)
        let dec = JSONDecoder()
        dec.keyDecodingStrategy = .convertFromSnakeCase
        return try dec.decode(T.self, from: data)
    }

    func run(_ method: String, _ args: [String: Any] = [:]) throws {
        _ = try raw(method, args)
    }
}
