import ApplicationServices

enum PermissionPrompt {
    static func priority(_ bundle: String) -> Int {
        // Microphone consent is presented here. Inspect it before unrelated
        // notification/control-center trees with potentially slow AX providers.
        bundle == "com.apple.UserNotificationCenter" ? 0 : 1
    }
    static func isOwner(_ bundle: String) -> Bool {
        ["com.apple.notificationcenterui", "com.apple.UserNotificationCenter", "com.apple.SecurityAgent", "com.apple.controlcenter"].contains(bundle)
    }

    static func belongsToQualification(_ labels: [String]) -> Bool {
        labels.contains { text in
            ["mixrelay", "mrroundtripprobe"].contains { text.localizedCaseInsensitiveContains($0) }
        }
    }

    static func isApproval(_ labels: [String]) -> Bool {
        labels.contains { ["Allow", "OK"].contains($0) }
    }

    static func approval(in dialog: InstalledElement) -> (buttons: [InstalledElement], labels: [String]) {
        var labels: [String] = []
        var buttons: [InstalledElement] = []
        _ = InstalledApplication.first(root: dialog, children: { $0.children }) { element in
            labels.append(contentsOf: element.labels)
            if element.role == kAXButtonRole && isApproval(element.labels) { buttons.append(element) }
            return false
        }
        return (belongsToQualification(labels) ? buttons : [], labels)
    }
}
