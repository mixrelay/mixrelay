import importlib.util
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch


class Node(list):
    def __init__(self, name, role, children=(), showing=True):
        super().__init__(children)
        self.name = name
        self.role = role
        self.showing = showing

    @property
    def childCount(self):
        return len(self)

    def getRoleName(self):
        return self.role

    def getState(self):
        return types.SimpleNamespace(contains=lambda state: self.showing)


class InstalledEditorTest(unittest.TestCase):
    def setUp(self):
        self.desktop = Node("desktop", "desktop")
        registry = types.SimpleNamespace(getDesktop=lambda index: self.desktop)
        self.atspi = types.SimpleNamespace(Registry=registry, STATE_SHOWING=1)
        spec = importlib.util.spec_from_file_location(
            "linux_ui", Path(__file__).with_name("linux-ui.py"))
        self.ui = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, pyatspi=self.atspi):
            spec.loader.exec_module(self.ui)
        self.add = Node("Add node", "combo box")
        self.editor = Node("mixrelay-desktop", "application", [
            Node("MixRelay", "frame", [self.add])])
        self.runtime = Node("mixrelay-desktop", "application")

    def test_headless_runtime_does_not_hide_editor_in_either_registration_order(self):
        for apps in ([self.runtime, self.editor], [self.editor, self.runtime]):
            with self.subTest(runtime_first=apps[0] is self.runtime):
                self.desktop[:] = apps
                self.assertIs(self.ui.application(), self.editor)
                self.assertIs(self.ui.find("Add node"), self.add)

    def test_other_apps_and_hidden_editors_cannot_supply_controls(self):
        other = Node("other-product", "application", [Node("MixRelay", "frame", [self.add])])
        hidden = Node("mixrelay-desktop", "application", [
            Node("MixRelay", "frame", [self.add], showing=False)])
        self.desktop[:] = [other, hidden, self.runtime]
        with self.assertRaisesRegex(LookupError, "visible accessibility editor"):
            self.ui.application()

    def test_null_and_defunct_children_do_not_hide_live_controls_or_diagnostics(self):
        class Defunct(Node):
            def getRoleName(self):
                raise RuntimeError("AT-SPI object disappeared")

        class Changing(Node):
            def __getitem__(self, index):
                if index == 1:
                    raise RuntimeError("AT-SPI child disappeared")
                return super().__getitem__(index)

        self.editor.insert(0, None)
        self.editor.append(Changing("panel", "panel", [None, None, Defunct("gone", "button")]))
        self.editor[-1].append(Node("Compressor", "button"))
        self.desktop[:] = [None, Defunct("gone", "application"), self.runtime, self.editor]
        self.assertIs(self.ui.find("Add node"), self.add)
        self.assertEqual(self.ui.find("Compressor").name, "Compressor")
        tree = self.ui.tree_text(self.desktop)
        self.assertIn("combo box Add node", tree)
        self.assertIn("button Compressor", tree)
        self.assertNotIn("gone", tree)
        self.assertEqual(list(self.ui.descendants(None)), [])
        diagnostic = self.ui.tree_text(self.desktop, diagnostic=True)
        self.assertIn("AT-SPI child 0 returned null", diagnostic)
        self.assertIn("AT-SPI child 1 failed: AT-SPI child disappeared", diagnostic)
        self.assertIn("AT-SPI description failed: AT-SPI object disappeared", diagnostic)
        self.assertIn("button Compressor", diagnostic)
        self.assertIn("AT-SPI childCount=0", diagnostic)
        hidden = self.ui.tree_text(Node("Retry connection", "button", showing=False), diagnostic=True)
        self.assertIn("button Retry connection [showing=False]", hidden)

    def test_diagnostics_report_an_unreadable_child_count(self):
        class Unreadable(Node):
            @property
            def childCount(self):
                raise RuntimeError("query timed out")

        tree = self.ui.tree_text(Unreadable("panel", "panel"), diagnostic=True)
        self.assertIn("panel panel", tree)
        self.assertIn("AT-SPI childCount failed: query timed out", tree)

    def test_missing_hidden_or_noninteractive_controls_still_fail(self):
        self.desktop[:] = [self.runtime, self.editor]
        for role, showing in [("combo box", False), ("label", True)]:
            with self.subTest(role=role, showing=showing):
                self.add.role, self.add.showing = role, showing
                with patch.object(self.ui.time, "monotonic", side_effect=[0, 1, 31]), \
                        patch.object(self.ui.time, "sleep"):
                    with self.assertRaisesRegex(AssertionError, "Missing installed UI control"):
                        self.ui.find("Add node")

    def test_activation_posts_one_pointer_click_without_an_uncertain_accessibility_action(self):
        self.desktop[:] = [self.editor]
        self.add.queryComponent = lambda: types.SimpleNamespace(
            getExtents=lambda coords: types.SimpleNamespace(x=10, y=20, width=30, height=40))
        self.atspi.DESKTOP_COORDS = 0
        events = []
        self.atspi.Registry.generateMouseEvent = lambda *args: events.append(args)
        def uncertain_action():
            raise RuntimeError("AT-SPI action timed out after dispatch")
        self.add.queryAction = uncertain_action
        self.ui.click("Add node")
        self.assertEqual(events, [(25, 40, "abs"), (25, 40, "b1c")])
        events.clear()
        self.ui.click("Add node", right=True)
        self.assertEqual(events, [(25, 40, "abs"), (25, 40, "b3c")])

    def test_submenu_receives_focus_and_right_arrow_instead_of_a_leaf_action(self):
        self.desktop[:] = [self.editor]
        self.atspi.DESKTOP_COORDS = 0
        self.atspi.STATE_FOCUSED = 2
        self.atspi.KEY_SYM = 3
        events = []
        self.atspi.Registry.generateMouseEvent = lambda *args: events.append(args)
        self.atspi.Registry.generateKeyboardEvent = lambda *args: events.append(args)
        component = types.SimpleNamespace(
            getExtents=lambda coords: types.SimpleNamespace(x=10, y=20, width=30, height=40),
            grabFocus=lambda: events.append("focus") or True)
        self.add.queryComponent = lambda: component
        self.ui.open_submenu("Add node")
        self.assertEqual(events, [(25, 40, "abs"), "focus", (0xff53, None, 3)])
        events.clear()
        component.grabFocus = lambda: False
        self.add.getState = lambda: types.SimpleNamespace(contains=lambda state: state == self.atspi.STATE_SHOWING)
        with patch.object(self.ui.time, "monotonic", side_effect=[0, 0, 0, 31]):
            with self.assertRaisesRegex(AssertionError, "never received focus"):
                self.ui.open_submenu("Add node")
        self.assertEqual(events, [(25, 40, "abs")])


if __name__ == "__main__":
    unittest.main()
