import XCTest

final class InstalledApplicationTests: XCTestCase {
    func testDestinationChoiceIsScopedToTheListForStaticTextAndCellWebKitProviders() {
        let tree = ["app": ["node", "wrong-list", "destination"], "wrong-list": ["other"], "destination": ["option"]]
        let labels = ["node": ["Recorder"], "other": ["Recorder"], "option": ["Recorder"], "wrong-list": ["Other"], "destination": ["Destination"]]
        for optionRole in [kAXStaticTextRole, kAXCellRole, kAXRowRole] {
            let roles = ["node": kAXStaticTextRole, "other": optionRole, "option": optionRole, "wrong-list": kAXListRole, "destination": kAXListRole]
            XCTAssertEqual(InstalledApplication.choice(root: "app", label: "Recorder", listLabel: "Destination", children: { tree[$0] ?? [] }, role: { roles[$0] ?? "" }, labels: { labels[$0] ?? [] }), "option")
            XCTAssertNil(InstalledApplication.choice(root: "app", label: "Compressor", listLabel: "Destination", children: { tree[$0] ?? [] }, role: { roles[$0] ?? "" }, labels: { labels[$0] ?? [] }))
        }
    }
    func testSearchStaysInsideSelectedEditorTree() {
        let trees = ["editor": ["MixRelay", "Add node"], "runtime": ["Quit"]]
        XCTAssertEqual(InstalledApplication.first(root: "editor", children: { trees[$0] ?? [] }, matches: { $0 == "Add node" }), "Add node")
        XCTAssertNil(InstalledApplication.first(root: "runtime", children: { trees[$0] ?? [] }, matches: { $0 == "Add node" }))
    }

    func testSearchStopsAtBrokenProviderCycle() {
        var visits = 0
        XCTAssertNil(InstalledApplication.first(root: 1, children: { [$0] }, matches: { _ in visits += 1; return false }))
        XCTAssertEqual(visits, 10_000)
    }

    func testPermissionObserverUsesGuiOwnersAndOnlyApprovesQualificationPrompts() {
        XCTAssertFalse(PermissionPrompt.isOwner("com.apple.tccd"))
        XCTAssertTrue(PermissionPrompt.isOwner("com.apple.UserNotificationCenter"))
        XCTAssertLessThan(PermissionPrompt.priority("com.apple.UserNotificationCenter"), PermissionPrompt.priority("com.apple.controlcenter"))
        XCTAssertTrue(PermissionPrompt.belongsToQualification(["Microphone", "mixrelay-engine-service wants access"]))
        XCTAssertTrue(PermissionPrompt.belongsToQualification(["MRRoundTripProbe wants access"]))
        XCTAssertFalse(PermissionPrompt.belongsToQualification(["Another app wants access", "OK"]))
        XCTAssertTrue(PermissionPrompt.isApproval(["OK"]))
        XCTAssertTrue(PermissionPrompt.isApproval(["Allow"]))
        XCTAssertFalse(PermissionPrompt.isApproval(["Don't Allow"]))
    }
}
