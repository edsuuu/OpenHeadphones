// Updates the app itself from the GitHub releases, as src/update.rs does for Windows and Linux: the
// latest tag against this build's version, then the release's zip unpacked in place of this .app.

import Foundation

enum Update {
    static let repo = "https://github.com/edsuuu/OpenHeadphones"

    /// Updating, as the App sheet shows it.
    enum Status: Equatable {
        case idle, checking, upToDate, failed
        /// A newer version, not installed (automatic updates off).
        case available(String)
        case installing(String)
    }

    /// This build's version (Info.plist); "dev" when run from mac/.build.
    static let current = Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "dev"

    /// Only an app bundle replaces itself (a dev build has none).
    static var bundled: Bool { Bundle.main.bundleURL.pathExtension == "app" }

    /// Whether `latest` (e.g. "0.4.0") is newer than `than`, number by number ("0.3" = "0.3.0").
    static func newer(_ latest: String, than: String = current) -> Bool {
        let parse = { (v: String) in v.split(separator: ".").map { Int($0) ?? 0 } }
        var (a, b) = (parse(latest), parse(than))
        let n = max(a.count, b.count)
        a += Array(repeating: 0, count: n - a.count)
        b += Array(repeating: 0, count: n - b.count)
        return b.lexicographicallyPrecedes(a)
    }

    /// The latest release's version: /releases/latest redirects to /releases/tag/v<version>
    /// (no API, no token).
    static func latest() async -> String? {
        var r = URLRequest(url: URL(string: "\(repo)/releases/latest")!)
        r.httpMethod = "HEAD"
        guard let (_, response) = try? await URLSession.shared.data(for: r), let url = response.url,
              url.deletingLastPathComponent().lastPathComponent == "tag", url.lastPathComponent.hasPrefix("v") else { return nil }
        let v = String(url.lastPathComponent.dropFirst())
        return v.first?.isNumber == true ? v : nil
    }

    struct Failure: Error, CustomStringConvertible { let description: String }

    private static func run(_ tool: String, _ args: String...) throws {
        let p = try Process.run(URL(fileURLWithPath: tool), arguments: args)
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { throw Failure(description: "\(tool) \(args): \(p.terminationStatus)") }
    }

    /// Downloads the latest release and puts its .app where this one is; the caller restarts.
    static func install() async throws {
        guard bundled else { throw Failure(description: "not running from an .app") }
        let fm = FileManager.default
        let tmp = fm.temporaryDirectory.appendingPathComponent("jbl-headphones-update")
        try? fm.removeItem(at: tmp)
        try fm.createDirectory(at: tmp, withIntermediateDirectories: true)
        let (download, response) = try await URLSession.shared.download(from: URL(string: "\(repo)/releases/latest/download/jbl-headphones-mac.zip")!)
        guard (response as? HTTPURLResponse)?.statusCode == 200 else { throw Failure(description: "download: \(response)") }
        let zip = tmp.appendingPathComponent("update.zip")
        try fm.moveItem(at: download, to: zip)
        try run("/usr/bin/ditto", "-x", "-k", zip.path, tmp.path)
        guard let new = try fm.contentsOfDirectory(at: tmp, includingPropertiesForKeys: nil).first(where: { $0.pathExtension == "app" }) else {
            throw Failure(description: "the zip has no .app")
        }
        // aside first, back on failure: never leave no app
        let app = Bundle.main.bundleURL
        let old = app.deletingLastPathComponent().appendingPathComponent(".\(app.lastPathComponent).old")
        try? fm.removeItem(at: old)
        try fm.moveItem(at: app, to: old)
        do {
            try fm.moveItem(at: new, to: app)
        } catch {
            try? fm.moveItem(at: old, to: app)
            throw error
        }
        try? fm.removeItem(at: old)
        try? fm.removeItem(at: tmp)
    }

    /// Opens the (new) app and leaves.
    static func restart() {
        try? run("/usr/bin/open", "-n", Bundle.main.bundleURL.path)
        exit(0)
    }
}
