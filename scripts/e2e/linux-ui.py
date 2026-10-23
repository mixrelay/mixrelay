"""Exercise the installed WebKit UI through its native accessibility tree."""
from pathlib import Path
import sys
import time

import pyatspi


def descendants(root):
    queue = [(root, 0)]
    count = 0
    while queue:
        node, depth = queue.pop()
        count += 1
        if count > 20000:
            raise RuntimeError("Installed accessibility tree exceeded its bound")
        yield node, depth
        if depth < 60:
            try:
                queue.extend((node[index], depth + 1) for index in reversed(range(node.childCount)))
            except Exception:
                continue


def application():
    for app in pyatspi.Registry.getDesktop(0):
        if "mixrelay" in app.name.lower():
            return app
    raise LookupError("Installed MixRelay has no accessibility application")


def find(label, contains=False, interactive=True):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            for node, _ in descendants(application()):
                role = node.getRoleName()
                matched = label in node.name if contains else node.name == label
                if matched and (not interactive or role in ("push button", "menu item", "combo box", "list item")):
                    if node.getState().contains(pyatspi.STATE_SHOWING):
                        return node
        except (LookupError, RuntimeError):
            pass
        time.sleep(0.1)
    raise AssertionError(f"Missing installed UI control: {label}")


def click(label, contains=False, right=False):
    node = find(label, contains=contains, interactive=not right)
    bounds = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    assert bounds.width > 0 and bounds.height > 0, f"Invisible UI control: {label}"
    pyatspi.Registry.generateMouseEvent(bounds.x + bounds.width // 2, bounds.y + bounds.height // 2,
                                       "b3c" if right else "b1c")


def main(output):
    click("Add node")
    click("Processor")
    click("Compressor")
    click("Add node")
    click("Output")
    click("Recorder")
    click("Compressor", right=True)
    click("Connect channels…")
    click("Destination", contains=True)
    click("Recorder")
    click("Connect")
    tree = [f"{' ' * depth}{node.getRoleName()} {node.name}" for node, depth in descendants(application())]
    (output / "installed-ui-tree.txt").write_text("\n".join(tree), encoding="utf-8")
    import gi
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk
    window = Gdk.get_default_root_window()
    image = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
    assert image is not None, "Installed X11 screenshot unavailable"
    image.savev(str(output / "installed-ui.png"), "png", [], [])
    (output / "installed-ui.ready").write_text("ready\n", encoding="utf-8")


if __name__ == "__main__":
    folder = Path(sys.argv[1]).resolve()
    try:
        main(folder)
    except Exception:
        try:
            tree = [f"{' ' * depth}{node.getRoleName()} {node.name}" for node, depth in descendants(application())]
            (folder / "installed-ui-failure.txt").write_text("\n".join(tree), encoding="utf-8")
        finally:
            raise
