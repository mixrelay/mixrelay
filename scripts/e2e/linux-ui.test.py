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
        self.desktop = []
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

    def test_missing_hidden_or_noninteractive_controls_still_fail(self):
        self.desktop[:] = [self.runtime, self.editor]
        for role, showing in [("combo box", False), ("label", True)]:
            with self.subTest(role=role, showing=showing):
                self.add.role, self.add.showing = role, showing
                with patch.object(self.ui.time, "monotonic", side_effect=[0, 1, 31]), \
                        patch.object(self.ui.time, "sleep"):
                    with self.assertRaisesRegex(AssertionError, "Missing installed UI control"):
                        self.ui.find("Add node")


if __name__ == "__main__":
    unittest.main()
