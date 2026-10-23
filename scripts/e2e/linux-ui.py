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
        if "mixrelay" not in (app.name or "").lower():
            continue
        # The editor and headless runtime have the same AT-SPI application name.
        # Registration order depends on startup timing; select the visible editor.
        for window in app:
            if (window.getRoleName() == "frame" and window.name == "MixRelay"
                    and window.getState().contains(pyatspi.STATE_SHOWING)):
                return app
    raise LookupError("Installed MixRelay has no visible accessibility editor")


def find(label, contains=False, interactive=True):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            for node, _ in descendants(application()):
                role = node.getRoleName()
                matched = label in node.name if contains else node.name == label
                if matched and (not interactive or role in ("button", "push button", "menu item", "combo box", "list item")):
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


def screenshot(path):
    import gi
    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk
    window = Gdk.get_default_root_window()
    previous = None
    stable = 0
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        Gdk.Display.get_default().sync()
        image = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
        assert image is not None, "Installed X11 screenshot unavailable"
        pixels = image.get_pixels()
        stable = stable + 1 if pixels == previous else 0
        if stable >= 3:
            image.savev(str(path), "png", [], [])
            return
        previous = pixels
        time.sleep(0.1)
    raise AssertionError("Installed screen did not settle after the UI action")


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
    find("Compressor CH 1 → Recorder CH 1", interactive=False)
    tree = [f"{' ' * depth}{node.getRoleName()} {node.name}" for node, depth in descendants(application())]
    (output / "installed-ui-tree.txt").write_text("\n".join(tree), encoding="utf-8")
    screenshot(output / "installed-ui.png")
    (output / "installed-ui.ready").write_text("ready\n", encoding="utf-8")


if __name__ == "__main__":
    folder = Path(sys.argv[1]).resolve()
    try:
        main(folder)
    except Exception:
        try:
            tree = [f"{' ' * depth}{node.getRoleName()} {node.name}" for node, depth in descendants(pyatspi.Registry.getDesktop(0))]
            (folder / "installed-ui-failure.txt").write_text("\n".join(tree), encoding="utf-8")
            screenshot(folder / "installed-ui-failure.png")
        finally:
            raise
