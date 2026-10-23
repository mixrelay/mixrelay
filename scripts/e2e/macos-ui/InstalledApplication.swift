import AppKit
import ApplicationServices

// AX roots retain the selected process identity even when two roles share a bundle ID.
struct InstalledElement {
    let raw: AXUIElement

    func attribute(_ name: String) -> CFTypeRef? {
        var value: CFTypeRef?
        guard AXUIElementCopyAttributeValue(raw, name as CFString, &value) == .success else { return nil }
        return value
    }

    var children: [InstalledElement] {
        (attribute(kAXChildrenAttribute) as? [AXUIElement] ?? []).map { InstalledElement(raw: $0) }
    }

    var role: String { attribute(kAXRoleAttribute) as? String ?? "" }
    var labels: [String] {
        [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute, kAXIdentifierAttribute]
            .compactMap { attribute($0) as? String }
    }

    func matches(_ label: String, roles: [String] = [], contains: Bool = false) -> Bool {
        (roles.isEmpty || roles.contains(role)) && labels.contains {
            contains ? $0.contains(label) : $0 == label
        }
    }

    func click(right: Bool = false) throws {
        if !right && AXUIElementPerformAction(raw, kAXPressAction as CFString) == .success { return }
        guard let position = attribute(kAXPositionAttribute), CFGetTypeID(position) == AXValueGetTypeID(),
              let size = attribute(kAXSizeAttribute), CFGetTypeID(size) == AXValueGetTypeID() else {
            throw InstalledApplication.Failure.control("Element has no clickable bounds: \(labels)")
        }
        var point = CGPoint.zero
        var extent = CGSize.zero
        guard AXValueGetValue(position as! AXValue, .cgPoint, &point),
              AXValueGetValue(size as! AXValue, .cgSize, &extent), extent.width > 0, extent.height > 0 else {
            throw InstalledApplication.Failure.control("Element has empty bounds: \(labels)")
        }
        point.x += extent.width / 2
        point.y += extent.height / 2
        let button: CGMouseButton = right ? .right : .left
        for type: CGEventType in right ? [.rightMouseDown, .rightMouseUp] : [.leftMouseDown, .leftMouseUp] {
            guard let event = CGEvent(mouseEventSource: nil, mouseType: type, mouseCursorPosition: point, mouseButton: button) else {
                throw InstalledApplication.Failure.control("Could not create mouse event")
            }
            event.post(tap: .cghidEventTap)
        }
    }
}

final class InstalledApplication {
    enum Failure: Error { case control(String) }
    let root: InstalledElement
    let pid: pid_t

    init(pid: pid_t) {
        self.pid = pid
        root = InstalledElement(raw: AXUIElementCreateApplication(pid))
        AXUIElementSetMessagingTimeout(root.raw, 2)
    }

    // The traversal is shared with synthetic trees in the unit tests. A broken AX
    // provider cannot keep a search alive by returning cycles or unbounded children.
    static func first<Node>(root: Node, children: (Node) -> [Node], matches: (Node) -> Bool) -> Node? {
        var pending = [root]
        var count = 0
        while let node = pending.popLast(), count < 10_000 {
            count += 1
            if matches(node) { return node }
            pending.append(contentsOf: children(node).reversed())
        }
        return nil
    }

    func find(_ label: String, roles: [String] = [], contains: Bool = false) -> InstalledElement? {
        Self.first(root: root, children: { $0.children }) { $0.matches(label, roles: roles, contains: contains) }
    }

    func wait(_ label: String, roles: [String] = [], contains: Bool = false, timeout: TimeInterval = 30) throws -> InstalledElement {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if let element = find(label, roles: roles, contains: contains) { return element }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        } while Date() < deadline
        throw Failure.control("Missing control \(label) in editor PID \(pid)")
    }

    var debugDescription: String {
        var lines = ["Installed editor PID \(pid)"]
        _ = Self.first(root: root, children: { $0.children }) { element in
            lines.append("\(element.role): \(element.labels)")
            return false
        }
        return lines.joined(separator: "\n")
    }
}
