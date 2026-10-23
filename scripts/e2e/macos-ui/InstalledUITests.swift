import XCTest
import AppKit

final class InstalledUITests: XCTestCase {
    private var app: InstalledApplication?
    private struct MissingControl: Error { let label: String }

    private func press(_ label: String) throws {
        let app = try XCTUnwrap(app)
        try app.wait(label, roles: [kAXButtonRole, kAXPopUpButtonRole, kAXMenuItemRole, kAXCellRole]).click()
    }

    private func createGraph(in directory: URL) throws {
        guard AXIsProcessTrusted() else { throw MissingControl(label: "Accessibility access for installed UI runner") }
        let installed = URL(fileURLWithPath: "/Applications/MixRelay/MixRelay.app").standardizedFileURL
        let editors = NSRunningApplication.runningApplications(withBundleIdentifier: "io.mixrelay.desktop")
            .filter { $0.activationPolicy == .regular && $0.bundleURL?.standardizedFileURL == installed }
        guard editors.count == 1, let editor = editors.first else {
            throw MissingControl(label: "Exactly one installed editor")
        }
        let app = InstalledApplication(pid: editor.processIdentifier)
        self.app = app
        guard editor.activate(options: [.activateAllWindows]) else {
            throw MissingControl(label: "Installed editor PID \(editor.processIdentifier)")
        }
        _ = try app.wait("MixRelay", roles: [kAXWindowRole], timeout: 20)
        try press("Add node")
        try press("Processor")
        try press("Compressor")
        try press("Add node")
        try press("Output")
        try press("Recorder")
        try app.wait("Compressor", roles: [kAXStaticTextRole]).click(right: true)
        try press("Connect channels…")
        try app.wait("Destination", roles: [kAXButtonRole, kAXPopUpButtonRole], contains: true, timeout: 10).click()
        try press("Recorder")
        try press("Connect")
        _ = try app.wait("Compressor CH 1 → Recorder CH 1")
        let shot = XCUIScreen.main.screenshot()
        try shot.pngRepresentation.write(to: directory.appendingPathComponent("installed-ui.png"))
        try app.debugDescription.write(to: directory.appendingPathComponent("installed-ui-tree.txt"), atomically: true, encoding: .utf8)
    }

    func testCreateConnectAndAuthorizeNativeAudio() throws {
        continueAfterFailure = true
        let folder = try XCTUnwrap(ProcessInfo.processInfo.environment["MIXRELAY_UI_EVIDENCE"])
        let directory = URL(fileURLWithPath: folder)
        do { try createGraph(in: directory) }
        catch {
            let detail = "\(error)\n\(app?.debugDescription ?? "Editor was not selected")"
            XCTFail(detail)
            try detail.write(to: directory.appendingPathComponent("installed-ui.error"), atomically: true, encoding: .utf8)
        }
        try Data("ready\n".utf8).write(to: directory.appendingPathComponent("installed-ui.ready"))

        // The same disposable session handles the real first-use privacy dialogs.
        // No privacy database or application binary is changed by this observer.
        let deadline = Date().addingTimeInterval(300)
        var grants: [[String: String]] = []
        while !FileManager.default.fileExists(atPath: directory.appendingPathComponent("installed-ui.finish").path) {
            if Date() >= deadline { XCTFail("Native acceptance did not finish"); break }
            // tccd is a daemon, not a launchable GUI application. Asking XCTest for
            // its state raises an Objective-C exception and kills this observer.
            for owner in NSWorkspace.shared.runningApplications {
                guard let bundle = owner.bundleIdentifier, PermissionPrompt.isOwner(bundle) else { continue }
                let gui = InstalledApplication(pid: owner.processIdentifier)
                let dialogs = gui.root.children.filter { [kAXWindowRole, kAXSheetRole].contains($0.role) }
                for dialog in dialogs {
                    let buttons = PermissionPrompt.buttons(in: dialog)
                    for button in buttons {
                        try button.click()
                        grants.append(["owner": bundle, "action": button.labels.joined(separator: "/"), "at": ISO8601DateFormatter().string(from: Date())])
                        try JSONSerialization.data(withJSONObject: grants, options: [.prettyPrinted, .sortedKeys])
                            .write(to: directory.appendingPathComponent("audio-permission-actions.json"), options: .atomic)
                    }
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
    }
}
