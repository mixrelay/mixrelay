import XCTest
import AppKit

final class InstalledUITests: XCTestCase {
    private let app = XCUIApplication(url: URL(fileURLWithPath: "/Applications/MixRelay/MixRelay.app"))

    private func press(_ label: String) {
        let types = [XCUIElement.ElementType.button, .popUpButton, .menuItem, .cell].map(\.rawValue)
        let item = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@ AND elementType IN %@", label, types)).firstMatch
        XCTAssertTrue(item.waitForExistence(timeout: 30), "Missing installed UI action: \(label)\n\(app.debugDescription)")
        item.click()
    }

    func testCreateConnectAndAuthorizeNativeAudio() throws {
        continueAfterFailure = false
        let folder = try XCTUnwrap(ProcessInfo.processInfo.environment["MIXRELAY_UI_EVIDENCE"])
        let directory = URL(fileURLWithPath: folder)
        let editor = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "io.mixrelay.desktop")
            .first(where: { $0.activationPolicy == .regular }))
        XCTAssertTrue(editor.activate(options: [.activateAllWindows]))
        XCTAssertTrue(app.windows["MixRelay"].waitForExistence(timeout: 20), "Installed editor PID \(editor.processIdentifier)\n\(app.debugDescription)")
        press("Add node")
        press("Processor")
        press("Compressor")
        press("Add node")
        press("Output")
        press("Recorder")
        let compressor = app.staticTexts["Compressor"].firstMatch
        XCTAssertTrue(compressor.waitForExistence(timeout: 30))
        compressor.rightClick()
        press("Connect channels…")
        let destination = app.descendants(matching: .any).matching(NSPredicate(
            format: "(elementType == %d OR elementType == %d) AND label CONTAINS %@",
            XCUIElement.ElementType.button.rawValue, XCUIElement.ElementType.popUpButton.rawValue, "Destination"
        )).firstMatch
        XCTAssertTrue(destination.waitForExistence(timeout: 10))
        destination.click()
        press("Recorder")
        press("Connect")
        let shot = app.screenshot()
        try shot.pngRepresentation.write(to: directory.appendingPathComponent("installed-ui.png"))
        try app.debugDescription.write(to: directory.appendingPathComponent("installed-ui-tree.txt"), atomically: true, encoding: .utf8)
        try Data("ready\n".utf8).write(to: directory.appendingPathComponent("installed-ui.ready"))

        // The same disposable session handles the real first-use privacy dialogs.
        // No privacy database or application binary is changed by this observer.
        let deadline = Date().addingTimeInterval(300)
        while !FileManager.default.fileExists(atPath: directory.appendingPathComponent("installed-ui.finish").path) {
            XCTAssertLessThan(Date(), deadline, "Native acceptance did not finish")
            for bundle in ["com.apple.UserNotificationCenter", "com.apple.SecurityAgent", "com.apple.tccd"] {
                let owner = XCUIApplication(bundleIdentifier: bundle)
                let dialogs = owner.dialogs.allElementsBoundByIndex + owner.alerts.allElementsBoundByIndex
                for dialog in dialogs where dialog.exists {
                    guard ["mixrelay", "mrroundtripprobe"].contains(where: dialog.debugDescription.localizedCaseInsensitiveContains) else { continue }
                    for label in ["Allow", "OK"] where dialog.buttons[label].exists {
                        dialog.buttons[label].click()
                    }
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
    }
}
