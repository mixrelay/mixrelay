import XCTest
import AppKit

final class InstalledUITests: XCTestCase {
    private var app = XCUIApplication(url: URL(fileURLWithPath: "/Applications/MixRelay/MixRelay.app"))
    private struct MissingControl: Error { let label: String }

    private func press(_ label: String) throws {
        let types = [XCUIElement.ElementType.button, .popUpButton, .menuItem, .cell].map(\.rawValue)
        let item = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ AND elementType IN %@", label, types)).firstMatch
        guard item.waitForExistence(timeout: 30) else { throw MissingControl(label: label) }
        item.click()
    }

    private func createGraph(in directory: URL) throws {
        let editor = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "io.mixrelay.desktop")
            .first(where: { $0.activationPolicy == .regular }))
        app = try XCTUnwrap(MRInstalledApplication(editor.processIdentifier), "XCTest cannot select the exact installed editor PID")
        guard editor.activate(options: [.activateAllWindows]), app.windows["MixRelay"].waitForExistence(timeout: 20) else {
            throw MissingControl(label: "Installed editor PID \(editor.processIdentifier)")
        }
        try press("Add node")
        try press("Processor")
        try press("Compressor")
        try press("Add node")
        try press("Output")
        try press("Recorder")
        let compressor = app.staticTexts["Compressor"].firstMatch
        guard compressor.waitForExistence(timeout: 30) else { throw MissingControl(label: "Compressor node") }
        compressor.rightClick()
        try press("Connect channels…")
        let destination = app.descendants(matching: .any).matching(NSPredicate(
            format: "(elementType == %d OR elementType == %d) AND label CONTAINS %@",
            XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.popUpButton.rawValue, "Destination"
        )).firstMatch
        guard destination.waitForExistence(timeout: 10) else { throw MissingControl(label: "Destination") }
        destination.click()
        try press("Recorder")
        try press("Connect")
        let connection = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Compressor CH 1 → Recorder CH 1")).firstMatch
        guard connection.waitForExistence(timeout: 30) else { throw MissingControl(label: "Authored channel connection") }
        let shot = app.screenshot()
        try shot.pngRepresentation.write(to: directory.appendingPathComponent("installed-ui.png"))
        try app.debugDescription.write(to: directory.appendingPathComponent("installed-ui-tree.txt"), atomically: true, encoding: .utf8)
    }

    func testCreateConnectAndAuthorizeNativeAudio() throws {
        continueAfterFailure = true
        let folder = try XCTUnwrap(ProcessInfo.processInfo.environment["MIXRELAY_UI_EVIDENCE"])
        let directory = URL(fileURLWithPath: folder)
        do { try createGraph(in: directory) }
        catch {
            let detail = "\(error)\n\(app.debugDescription)"
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
            for bundle in ["com.apple.notificationcenterui", "com.apple.UserNotificationCenter", "com.apple.SecurityAgent", "com.apple.tccd"] {
                let owner = XCUIApplication(bundleIdentifier: bundle)
                guard owner.state != .notRunning else { continue }
                let dialogs = owner.dialogs.allElementsBoundByIndex + owner.alerts.allElementsBoundByIndex + owner.windows.allElementsBoundByIndex
                for dialog in dialogs where dialog.exists {
                    guard ["mixrelay", "mrroundtripprobe"].contains(where: dialog.debugDescription.localizedCaseInsensitiveContains) else { continue }
                    for label in ["Allow", "OK"] where dialog.buttons[label].exists {
                        dialog.buttons[label].click()
                        grants.append(["owner": bundle, "action": label, "at": ISO8601DateFormatter().string(from: Date())])
                        try JSONSerialization.data(withJSONObject: grants, options: [.prettyPrinted, .sortedKeys])
                            .write(to: directory.appendingPathComponent("audio-permission-actions.json"), options: .atomic)
                    }
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
    }
}
