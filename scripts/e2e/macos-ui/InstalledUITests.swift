import XCTest
import AppKit
import ApplicationServices

final class InstalledUITests: XCTestCase {
    private struct MissingControl: Error { let label: String }
    private var app = AXUIElementCreateSystemWide()

    private func attribute(_ node: AXUIElement, _ name: String) -> CFTypeRef? {
        var value: CFTypeRef?
        return AXUIElementCopyAttributeValue(node, name as CFString, &value) == .success ? value : nil
    }

    private func text(_ node: AXUIElement) -> String {
        [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
            .compactMap { attribute(node, $0) as? String }.filter { !$0.isEmpty }.joined(separator: " | ")
    }

    private func descendants(_ root: AXUIElement) -> [(AXUIElement, Int)] {
        var queue = [(root, 0)], result: [(AXUIElement, Int)] = []
        while let (node, depth) = queue.popLast(), result.count < 20_000 {
            result.append((node, depth))
            if depth < 60, let children = attribute(node, kAXChildrenAttribute) as? [AXUIElement] {
                queue.append(contentsOf: children.reversed().map { ($0, depth + 1) })
            }
        }
        return result
    }

    private func tree(_ root: AXUIElement) -> String {
        descendants(root).map { node, depth in
            String(repeating: " ", count: depth) + (attribute(node, kAXRoleAttribute) as? String ?? "?") + " " + text(node)
        }.joined(separator: "\n")
    }

    private func find(_ label: String, contains: Bool = false, interactive: Bool = true) throws -> AXUIElement {
        let deadline = Date().addingTimeInterval(30)
        let roles = [kAXButtonRole, kAXPopUpButtonRole, kAXMenuItemRole, kAXComboBoxRole, kAXRowRole]
        repeat {
            for (node, _) in descendants(app) {
                let labels = [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].compactMap { attribute(node, $0) as? String }
                let matches = labels.contains { contains ? $0.contains(label) : $0 == label }
                let role = attribute(node, kAXRoleAttribute) as? String ?? ""
                if matches && (!interactive || roles.contains(role))
                    && attribute(node, kAXEnabledAttribute) as? Bool != false
                    && attribute(node, kAXHiddenAttribute) as? Bool != true { return node }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        throw MissingControl(label: label)
    }

    private func click(_ node: AXUIElement, right: Bool = false) throws {
        guard let position = attribute(node, kAXPositionAttribute), CFGetTypeID(position) == AXValueGetTypeID(),
              let size = attribute(node, kAXSizeAttribute), CFGetTypeID(size) == AXValueGetTypeID() else {
            throw MissingControl(label: "Visible bounds: \(text(node))")
        }
        var point = CGPoint.zero, dimensions = CGSize.zero
        AXValueGetValue(unsafeBitCast(position, to: AXValue.self), .cgPoint, &point)
        AXValueGetValue(unsafeBitCast(size, to: AXValue.self), .cgSize, &dimensions)
        guard dimensions.width > 0 && dimensions.height > 0 else { throw MissingControl(label: "Empty bounds") }
        point.x += dimensions.width / 2; point.y += dimensions.height / 2
        let down = CGEvent(mouseEventSource: nil, mouseType: right ? .rightMouseDown : .leftMouseDown,
                           mouseCursorPosition: point, mouseButton: right ? .right : .left)
        let up = CGEvent(mouseEventSource: nil, mouseType: right ? .rightMouseUp : .leftMouseUp,
                         mouseCursorPosition: point, mouseButton: right ? .right : .left)
        try XCTUnwrap(down).post(tap: .cghidEventTap)
        try XCTUnwrap(up).post(tap: .cghidEventTap)
    }

    private func press(_ label: String) throws { try click(find(label)) }

    private func createGraph(in directory: URL) throws {
        guard AXIsProcessTrusted() else { throw MissingControl(label: "UI test runner accessibility permission") }
        let editor = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: "io.mixrelay.desktop")
            .first(where: { $0.activationPolicy == .regular }))
        app = AXUIElementCreateApplication(editor.processIdentifier)
        guard editor.activate(options: [.activateAllWindows]) else { throw MissingControl(label: "Activate installed editor") }
        try press("Add node"); try press("Processor"); try press("Compressor")
        try press("Add node"); try press("Output"); try press("Recorder")
        try click(find("Compressor", interactive: false), right: true)
        try press("Connect channels…")
        try click(find("Destination", contains: true))
        try press("Recorder"); try press("Connect")
        _ = try find("Compressor CH 1 → Recorder CH 1", interactive: false)
        try XCUIScreen.main.screenshot().pngRepresentation.write(to: directory.appendingPathComponent("installed-ui.png"))
        try tree(app).write(to: directory.appendingPathComponent("installed-ui-tree.txt"), atomically: true, encoding: .utf8)
    }

    func testCreateConnectAndAuthorizeNativeAudio() throws {
        continueAfterFailure = true
        let folder = try XCTUnwrap(ProcessInfo.processInfo.environment["MIXRELAY_UI_EVIDENCE"])
        let directory = URL(fileURLWithPath: folder)
        do { try createGraph(in: directory) }
        catch {
            let detail = "\(error)\n\(tree(app))"
            XCTFail(detail)
            try detail.write(to: directory.appendingPathComponent("installed-ui.error"), atomically: true, encoding: .utf8)
        }
        try Data("ready\n".utf8).write(to: directory.appendingPathComponent("installed-ui.ready"))
        let deadline = Date().addingTimeInterval(300)
        var grants: [[String: String]] = []
        let owners = ["com.apple.notificationcenterui", "com.apple.UserNotificationCenter", "com.apple.SecurityAgent", "com.apple.tccd", "com.apple.systemuiserver"]
        while !FileManager.default.fileExists(atPath: directory.appendingPathComponent("installed-ui.finish").path) {
            if Date() >= deadline { XCTFail("Native acceptance did not finish"); break }
            for owner in NSWorkspace.shared.runningApplications where owners.contains(owner.bundleIdentifier ?? "") {
                let root = AXUIElementCreateApplication(owner.processIdentifier)
                let windows = attribute(root, kAXWindowsAttribute) as? [AXUIElement] ?? []
                for window in windows where ["mixrelay", "mrroundtripprobe"].contains(where: tree(window).localizedCaseInsensitiveContains) {
                    for (node, _) in descendants(window) where attribute(node, kAXRoleAttribute) as? String == kAXButtonRole {
                        let labels = [kAXTitleAttribute, kAXDescriptionAttribute].compactMap { attribute(node, $0) as? String }
                        guard let action = labels.first(where: ["Allow", "OK"].contains) else { continue }
                        guard AXUIElementPerformAction(node, kAXPressAction as CFString) == .success else { continue }
                        grants.append(["owner": owner.bundleIdentifier ?? "", "action": action, "at": ISO8601DateFormatter().string(from: Date())])
                        try JSONSerialization.data(withJSONObject: grants, options: [.prettyPrinted, .sortedKeys])
                            .write(to: directory.appendingPathComponent("audio-permission-actions.json"), options: .atomic)
                    }
                }
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
    }
}
