import XCTest
import AppKit
import CoreGraphics

final class InstalledUITests: XCTestCase {
    private var app: InstalledApplication?
    private var editorPid: pid_t?
    private struct MissingControl: Error { let label: String }

    private func press(_ label: String) throws {
        let app = try XCTUnwrap(app)
        let control = try app.wait(label, roles: [kAXButtonRole, kAXPopUpButtonRole, kAXMenuItemRole, kAXCellRole])
        if control.role == kAXMenuItemRole { try control.chooseMenuItem() }
        else { try control.click() }
    }

    private func createGraph(in directory: URL) throws {
        let expected = try JSONSerialization.jsonObject(with: Data(contentsOf:
            directory.appendingPathComponent("installed-ui-runner-expected.json"))) as? [String: String]
        let actual = ["bundle": Bundle.main.bundleIdentifier ?? "", "path": Bundle.main.bundleURL.path,
                      "pid": String(ProcessInfo.processInfo.processIdentifier), "trusted": String(AXIsProcessTrusted()),
                      "screenCapture": String(CGPreflightScreenCaptureAccess())]
        try JSONSerialization.data(withJSONObject: actual, options: [.prettyPrinted, .sortedKeys])
            .write(to: directory.appendingPathComponent("installed-ui-runner-actual.json"))
        guard actual["bundle"] == expected?["bundle"], actual["path"] == expected?["path"] else {
            throw MissingControl(label: "Granted XCTest runner identity: \(actual)")
        }
        guard AXIsProcessTrusted() else { throw MissingControl(label: "Accessibility access for installed UI runner") }
        guard CGPreflightScreenCaptureAccess() else { throw MissingControl(label: "Screen capture access for installed UI runner") }
        let installed = URL(fileURLWithPath: "/Applications/MixRelay/MixRelay.app").standardizedFileURL
        let editors = NSRunningApplication.runningApplications(withBundleIdentifier: "io.mixrelay.desktop")
            .filter { $0.activationPolicy == .regular && $0.bundleURL?.standardizedFileURL == installed }
        guard editors.count == 1, let editor = editors.first else {
            throw MissingControl(label: "Exactly one installed editor")
        }
        let app = InstalledApplication(pid: editor.processIdentifier)
        self.app = app
        self.editorPid = editor.processIdentifier
        guard editor.activate(options: [.activateAllWindows]) else {
            throw MissingControl(label: "Installed editor PID \(editor.processIdentifier)")
        }
        let activationDeadline = Date().addingTimeInterval(20)
        while !editor.isActive {
            guard Date() < activationDeadline else { throw MissingControl(label: "Installed editor did not become active") }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        app.editorWindow = try app.waitEditorWindow()
        _ = try app.wait("Add apps, devices, effects and more.", roles: [kAXStaticTextRole])
        try press("Add node")
        try app.wait("Processor", roles: [kAXMenuItemRole]).openSubmenu()
        try press("Compressor")
        _ = try app.wait("Compressor", roles: [kAXGroupRole])
        try press("Add node")
        try app.wait("Output", roles: [kAXMenuItemRole]).openSubmenu()
        try press("Recorder")
        _ = try app.wait("Recorder", roles: [kAXGroupRole])
        try app.wait("Compressor", roles: [kAXGroupRole]).openContextMenu()
        try press("Connect channels…")
        try app.wait("Destination", roles: [kAXButtonRole, kAXPopUpButtonRole], contains: true, timeout: 10).click()
        try app.waitChoice("Recorder", in: "Destination").click()
        try press("Connect")
        _ = try app.wait("Compressor CH 1 → Recorder CH 1")
        _ = try app.wait("Recorder: Record", roles: [kAXCheckBoxRole, kAXButtonRole], enabled: true)
        guard app.find("Recording error", roles: [kAXStaticTextRole], contains: true) == nil else {
            throw MissingControl(label: "Recorder must be ready without an error after graph creation")
        }
        try WindowScreenshot.capture(pid: editor.processIdentifier, to: directory.appendingPathComponent("installed-ui.png"))
        try app.debugDescription.write(to: directory.appendingPathComponent("installed-ui-tree.txt"), atomically: true, encoding: .utf8)
        let window = try XCTUnwrap(app.editorWindow)
        let acknowledgement = AXUIElementSetAttributeValue(window.raw, kAXMinimizedAttribute as CFString, kCFBooleanTrue)
        let deadline = Date().addingTimeInterval(20)
        while window.attribute(kAXMinimizedAttribute) as? Bool != true && Date() < deadline {
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        guard window.attribute(kAXMinimizedAttribute) as? Bool == true else {
            throw MissingControl(label: "Installed editor window did not minimize: acknowledgement \(acknowledgement.rawValue)")
        }
        try JSONSerialization.data(withJSONObject: ["editorPid": editor.processIdentifier, "editorMinimized": true,
            "acknowledgement": acknowledgement.rawValue], options: [.prettyPrinted, .sortedKeys])
            .write(to: directory.appendingPathComponent("installed-ui-audio-phase.json"))
    }

    func testCreateConnectAndAuthorizeNativeAudio() throws {
        continueAfterFailure = true
        let folder = try XCTUnwrap(ProcessInfo.processInfo.environment["MIXRELAY_UI_EVIDENCE"])
        let directory = URL(fileURLWithPath: folder)
        do { try createGraph(in: directory) }
        catch {
            if let pid = editorPid {
                try? WindowScreenshot.capture(pid: pid, to: directory.appendingPathComponent("installed-ui-failure.png"))
            }
            let detail = "\(error)\n\(app?.debugDescription ?? "Editor was not selected")"
            XCTFail(detail)
            try detail.write(to: directory.appendingPathComponent("installed-ui.error"), atomically: true, encoding: .utf8)
        }
        try Data("ready\n".utf8).write(to: directory.appendingPathComponent("installed-ui.ready"))

        // The same disposable session handles the real first-use privacy dialogs.
        // No privacy database or application binary is changed by this observer.
        let deadline = Date().addingTimeInterval(300)
        var grants: [[String: Any]] = []
        var submitted: [String: (index: Int, owner: pid_t, dialog: AXUIElement)] = [:]
        while !FileManager.default.fileExists(atPath: directory.appendingPathComponent("installed-ui.finish").path) {
            if Date() >= deadline { XCTFail("Native acceptance did not finish"); break }
            var observed: [pid_t: [InstalledElement]] = [:]
            var changed = false
            // tccd is a daemon, not a launchable GUI application. Asking XCTest for
            // its state raises an Objective-C exception and kills this observer.
            for owner in NSWorkspace.shared.runningApplications.sorted(by: {
                PermissionPrompt.priority($0.bundleIdentifier ?? "") < PermissionPrompt.priority($1.bundleIdentifier ?? "")
            }) {
                guard let bundle = owner.bundleIdentifier, PermissionPrompt.isOwner(bundle) else { continue }
                let gui = InstalledApplication(pid: owner.processIdentifier, messagingTimeout: 0.2)
                guard let children = gui.root.availableChildren else { continue }
                observed[owner.processIdentifier] = children
                let dialogs = children.filter { [kAXWindowRole, kAXSheetRole].contains($0.role) }
                for dialog in dialogs {
                    let approval = PermissionPrompt.approval(in: dialog)
                    guard PermissionPrompt.belongsToQualification(approval.labels) else { continue }
                    guard let identity = PermissionPrompt.identity(owner: owner.processIdentifier, labels: approval.labels) else { continue }
                    guard submitted[identity] == nil, let button = approval.buttons.first else { continue }
                    let action = button.labels.joined(separator: "/")
                    try button.pressNative()
                    submitted[identity] = (grants.count, owner.processIdentifier, dialog.raw)
                    grants.append(["owner": bundle, "action": action, "subject": approval.labels, "at": ISO8601DateFormatter().string(from: Date())])
                    changed = true
                }
            }
            for submission in submitted.values {
                guard let windows = observed[submission.owner], grants[submission.index]["dismissedAt"] == nil,
                      !windows.contains(where: { CFEqual($0.raw, submission.dialog) }) else { continue }
                grants[submission.index]["dismissedAt"] = ISO8601DateFormatter().string(from: Date())
                changed = true
            }
            if changed {
                try JSONSerialization.data(withJSONObject: grants, options: [.prettyPrinted, .sortedKeys])
                    .write(to: directory.appendingPathComponent("audio-permission-actions.json"), options: .atomic)
            }
            RunLoop.current.run(until: Date().addingTimeInterval(1))
        }
    }
}
