import XCTest

final class InstalledApplicationTests: XCTestCase {
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
        XCTAssertTrue(PermissionPrompt.belongsToQualification(["Microphone", "mixrelay-engine-service wants access"]))
        XCTAssertTrue(PermissionPrompt.belongsToQualification(["MRRoundTripProbe wants access"]))
        XCTAssertFalse(PermissionPrompt.belongsToQualification(["Another app wants access", "OK"]))
        XCTAssertTrue(PermissionPrompt.isApproval(["OK"]))
        XCTAssertTrue(PermissionPrompt.isApproval(["Allow"]))
        XCTAssertFalse(PermissionPrompt.isApproval(["Don't Allow"]))
    }
}
