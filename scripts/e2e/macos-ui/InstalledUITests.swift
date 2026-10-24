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
        let expected = try JSONSerialization.jsonObject(with: Data(contentsOf:
            directory.appendingPathComponent("installed-ui-runner-expected.json"))) as? [String: String]
        let actual = ["bundle": Bundle.main.bundleIdentifier ?? "", "path": Bundle.main.bundleURL.path,
                      "pid": String(ProcessInfo.processInfo.processIdentifier), "trusted": String(AXIsProcessTrusted())]
        try JSONSerialization.data(withJSONObject: actual, options: [.prettyPrinted, .sortedKeys])
            .write(to: directory.appendingPathComponent("installed-ui-runner-actual.json"))
        guard actual["bundle"] == expected?["bundle"], actual["path"] == expected?["path"] else {
            throw MissingControl(label: "Granted XCTest runner identity: \(actual)")
        }
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
        _ = try app.wait("Recorder", roles: [kAXGroupRole])
        try app.wait("Compressor", roles: [kAXGroupRole]).showContextMenu()
        try press("Connect channels…")
        try app.wait("Destination", roles: [kAXButtonRole, kAXPopUpButtonRole], contains: true, timeout: 10).click()
        try app.waitChoice("Recorder", in: "Destination").click()
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
            try? XCUIScreen.main.screenshot().pngRepresentation.write(to: directory.appendingPathComponent("installed-ui-failure.png"))
            let detail = "\(error)\n\(app?.debugDescription ?? "Editor was not selected")"
            XCTFail(detail)
            try detail.write(to: directory.appendingPathComponent("installed-ui.error"), atomically: true, encoding: .utf8)
        }
        try Data("ready\n".utf8).write(to: directory.appendingPathComponent("installed-ui.ready"))

        // The same disposable session handles the real first-use privacy dialogs.
        // No privacy database or application binary is changed by this observer.
        let deadline = Date().addingTimeInterval(300)
        var grants: [[String: Any]] = []
        while !FileManager.default.fileExists(atPath: directory.appendingPathComponent("installed-ui.finish").path) {
            if Date() >= deadline { XCTFail("Native acceptance did not finish"); break }
            // tccd is a daemon, not a launchable GUI application. Asking XCTest for
            // its state raises an Objective-C exception and kills this observer.
            for owner in NSWorkspace.shared.runningApplications.sorted(by: {
                PermissionPrompt.priority($0.bundleIdentifier ?? "") < PermissionPrompt.priority($1.bundleIdentifier ?? "")
            }) {
                guard let bundle = owner.bundleIdentifier, PermissionPrompt.isOwner(bundle) else { continue }
                let gui = InstalledApplication(pid: owner.processIdentifier, messagingTimeout: 0.2)
                let dialogs = gui.root.children.filter { [kAXWindowRole, kAXSheetRole].contains($0.role) }
                for dialog in dialogs {
                    let approval = PermissionPrompt.approval(in: dialog)
                    for button in approval.buttons {
                        let action = button.labels.joined(separator: "/")
                        try button.click()
                        grants.append(["owner": bundle, "action": action, "subject": approval.labels, "at": ISO8601DateFormatter().string(from: Date())])
                        try JSONSerialization.data(withJSONObject: grants, options: [.prettyPrinted, .sortedKeys])
                            .write(to: directory.appendingPathComponent("audio-permission-actions.json"), options: .atomic)
                    }
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
    }
}
