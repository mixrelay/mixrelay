"""Exercise the installed WebKit UI through its native accessibility tree."""
from pathlib import Path
import sys
import time
import traceback

import pyatspi


def children(node, report=None):
    if node is None:
        return
    try:
        count = node.childCount
    except Exception as error:
        if report is not None:
            report(f"[AT-SPI childCount failed: {error}]")
        return
    if report is not None:
        report(f"[AT-SPI childCount={count}]")
    for index in range(count):
        try:
            child = node[index]
        except Exception as error:
            if report is not None:
                report(f"[AT-SPI child {index} failed: {error}]")
            continue
        if child is not None:
            yield child
        elif report is not None:
            report(f"[AT-SPI child {index} returned null]")


def describe(node, report=None):
    try:
        return node.getRoleName(), node.name or "", node.getState().contains(pyatspi.STATE_SHOWING)
    except Exception as error:
        # AT-SPI proxies can disappear between enumeration and inspection.
        if report is not None:
            report(f"[AT-SPI description failed: {error}]")
        return None


def tree_text(root, diagnostic=False):
    lines = []
    report = lines.append if diagnostic else None
    for node, depth in descendants(root, report):
        description = describe(node, report)
        if description is not None:
            role, name, showing = description
            state = f" [showing={showing}]" if diagnostic else ""
            lines.append(f"{' ' * depth}{role} {name}{state}")
    return "\n".join(lines)


def descendants(root, report=None):
    queue = [(root, 0)]
    count = 0
    while queue:
        node, depth = queue.pop()
        if node is None:
            continue
        count += 1
        if count > 20000:
            raise RuntimeError("Installed accessibility tree exceeded its bound")
        yield node, depth
        if depth < 60:
            queue.extend((child, depth + 1) for child in reversed(list(children(node, report))))


def application():
    for app in children(pyatspi.Registry.getDesktop(0)):
        description = describe(app)
        if description is None or "mixrelay" not in description[1].lower():
            continue
        # The editor and headless runtime have the same AT-SPI application name.
        # Registration order depends on startup timing; select the visible editor.
        for window in children(app):
            description = describe(window)
            if description is None:
                continue
            role, name, showing = description
            if (role == "frame" and name == "MixRelay"
                    and showing):
                return app
    raise LookupError("Installed MixRelay has no visible accessibility editor")


def find(label, contains=False, interactive=True):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        try:
            for node, _ in descendants(application()):
                description = describe(node)
                if description is None:
                    continue
                role, name, showing = description
                matched = label in name if contains else name == label
                if matched and (not interactive or role in ("button", "push button", "menu item", "combo box", "list item")):
                    if showing:
                        return node
        except (LookupError, RuntimeError):
            pass
        time.sleep(0.1)
    raise AssertionError(f"Missing installed UI control: {label}")


def click(label, contains=False, right=False):
    started = time.monotonic()
    print(f"Finding installed UI control: {label}", flush=True)
    node = find(label, contains=contains, interactive=not right)
    bounds = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
    assert bounds.width > 0 and bounds.height > 0, f"Invisible UI control: {label}"
    if right:
        pyatspi.Registry.generateMouseEvent(bounds.x + bounds.width // 2, bounds.y + bounds.height // 2, "b3c")
    else:
        action = node.queryAction()
        assert action.nActions > 0, f"Installed UI control has no accessible action: {label}"
        assert action.doAction(0), f"Installed UI control rejected its accessible action: {label}"
    print(f"Activated installed UI control: {label} ({time.monotonic() - started:.2f}s)", flush=True)


def open_submenu(label):
    node = find(label)
    component = node.queryComponent()
    bounds = component.getExtents(pyatspi.DESKTOP_COORDS)
    assert bounds.width > 0 and bounds.height > 0, f"Invisible UI control: {label}"
    # A submenu trigger has no leaf action. Move off the previous hovered item,
    # then use the menu's focus + Right Arrow keyboard contract.
    pyatspi.Registry.generateMouseEvent(bounds.x + bounds.width // 2, bounds.y + bounds.height // 2, "abs")
    assert component.grabFocus(), f"Installed submenu rejected focus: {label}"
    deadline = time.monotonic() + 30
    while not node.getState().contains(pyatspi.STATE_FOCUSED):
        assert time.monotonic() < deadline, f"Installed submenu never received focus: {label}"
        time.sleep(0.1)
    pyatspi.Registry.generateKeyboardEvent(0xff53, None, pyatspi.KEY_SYM)
    print(f"Opened installed submenu with Right Arrow: {label}", flush=True)


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
    open_submenu("Processor")
    click("Compressor")
    click("Add node")
    open_submenu("Output")
    click("Recorder")
    click("Compressor", right=True)
    click("Connect channels…")
    click("Destination", contains=True)
    click("Recorder")
    click("Connect")
    find("Compressor CH 1 → Recorder CH 1", interactive=False)
    (output / "installed-ui-tree.txt").write_text(tree_text(application()), encoding="utf-8")
    screenshot(output / "installed-ui.png")
    (output / "installed-ui.ready").write_text("ready\n", encoding="utf-8")


if __name__ == "__main__":
    folder = Path(sys.argv[1]).resolve()
    try:
        main(folder)
    except Exception:
        traceback.print_exc()
        try:
            (folder / "installed-ui-failure.txt").write_text(
                tree_text(pyatspi.Registry.getDesktop(0), diagnostic=True), encoding="utf-8")
            screenshot(folder / "installed-ui-failure.png")
        except Exception:
            traceback.print_exc()
        try:
            # Keep the original observation above. Refresh only the failure diagnostic,
            # so a stale client cache can be distinguished from missing WebKit content.
            desktop = pyatspi.Registry.getDesktop(0)
            desktop.clearCache()
            (folder / "installed-ui-a11y-refreshed.txt").write_text(
                tree_text(desktop, diagnostic=True), encoding="utf-8")
        except Exception:
            traceback.print_exc()
        sys.exit(1)
