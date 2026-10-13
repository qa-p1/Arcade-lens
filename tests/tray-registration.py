#!/usr/bin/env python3
"""Real SNI lifecycle regression. Run ONLY via Link tools/e2e.py run --.

Usage: python3 tests/tray-registration.py APP_BINARY LINK_CLI find|look|lens|box|clipboard
Needs python3-dbus and python3-gi (Ubuntu packages), plus the Link runner's tools.
The watcher deliberately displays Passive items, like Caelestia's tray.
"""

import json
import os
from pathlib import Path
import subprocess
import sys
import time


def require_isolation():
    root = Path(os.environ.get("ARCADE_E2E_ROOT", "/")).resolve()
    if os.environ.get("ARCADE_E2E_INNER") != "1" or root == Path("/"):
        raise SystemExit("Use Link tools/e2e.py run -- python3 tests/tray-registration.py APP LINK_CLI find|look|lens|box|clipboard")
    for key in ("HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "ARCADE_HOME"):
        if not Path(os.environ.get(key, "/")).resolve().is_relative_to(root):
            raise SystemExit(f"Refusing non-isolated {key}")
    if any(os.environ.get(key) for key in ("WAYLAND_DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE")):
        raise SystemExit("Refusing a live Wayland/Hyprland session")
    if not os.environ.get("DBUS_SESSION_BUS_ADDRESS") or not os.environ.get("DISPLAY"):
        raise SystemExit("Link's private D-Bus and Xvfb are required")
    return root


ROOT = require_isolation()
import dbus  # noqa: E402 (never connect before the isolation guard)
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

WATCHER = "org.kde.StatusNotifierWatcher"
ITEM = "org.kde.StatusNotifierItem"
PROPERTIES = "org.freedesktop.DBus.Properties"
MENU = "com.canonical.dbusmenu"
DBusGMainLoop(set_as_default=True)


class Watcher(dbus.service.Object):
    def __init__(self):
        self.bus = dbus.SessionBus()
        self.name = dbus.service.BusName(WATCHER, self.bus, do_not_queue=True)
        super().__init__(self.name, "/StatusNotifierWatcher")
        self.items = {}
        self.registrations = 0
        self.maximum = 0
        self.bus.add_signal_receiver(self.owner_changed, signal_name="NameOwnerChanged",
                                     dbus_interface="org.freedesktop.DBus")

    @dbus.service.method(WATCHER, in_signature="s", out_signature="", sender_keyword="sender")
    def RegisterStatusNotifierItem(self, service, sender=None):
        name, path = (str(sender), str(service)) if service.startswith("/") else (str(service), "/StatusNotifierItem")
        key = name + path
        if key not in self.items:
            self.registrations += 1
        # libappindicator registers an object path, so its owner is the sender.
        owner = name if name.startswith(":") else str(self.bus.get_name_owner(name))
        self.items[key] = (name, path, owner)
        self.maximum = max(self.maximum, len(self.items))
        self.StatusNotifierItemRegistered(key)

    @dbus.service.method(WATCHER, in_signature="s", out_signature="")
    def RegisterStatusNotifierHost(self, _service):
        pass

    @dbus.service.signal(WATCHER, signature="s")
    def StatusNotifierItemRegistered(self, _item):
        pass

    @dbus.service.signal(WATCHER, signature="s")
    def StatusNotifierItemUnregistered(self, _item):
        pass

    def owner_changed(self, name, _old, new):
        for key, (service, _path, owner) in list(self.items.items()):
            if str(name) in (service, owner) and str(new) != owner:
                del self.items[key]
                self.StatusNotifierItemUnregistered(key)

    def properties(self):
        return {"RegisteredStatusNotifierItems": dbus.Array(sorted(self.items), signature="s"),
                "IsStatusNotifierHostRegistered": dbus.Boolean(True), "ProtocolVersion": dbus.Int32(0)}

    @dbus.service.method(PROPERTIES, in_signature="ss", out_signature="v")
    def Get(self, interface, name):
        assert interface == WATCHER
        return self.properties()[str(name)]

    @dbus.service.method(PROPERTIES, in_signature="s", out_signature="a{sv}")
    def GetAll(self, interface):
        assert interface == WATCHER
        return self.properties()


def pump():
    context = GLib.MainContext.default()
    while context.pending():
        context.iteration(False)


def until(predicate, message, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pump()
        result = predicate()
        if result:
            return result
        time.sleep(0.01)
    raise AssertionError(message)


def settle(seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        pump()
        time.sleep(0.01)


class Host:
    def __init__(self, cli, env):
        self.process = subprocess.Popen([str(cli), "tray", "host"], env=env, stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.buffer = b""
        os.set_blocking(self.process.stdout.fileno(), False)
        assert self.line() == "ready"

    def line(self):
        def read():
            try:
                self.buffer += os.read(self.process.stdout.fileno(), 65536)
            except BlockingIOError:
                pass
            return b"\n" in self.buffer
        until(read, "fake host response timed out")
        line, self.buffer = self.buffer.split(b"\n", 1)
        return line.decode()

    def commands(self, *commands):
        self.process.stdin.write(("\n".join(commands) + "\n").encode())
        self.process.stdin.flush()
        for command in commands:
            assert self.line() == "ok " + command

    def kill(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=5)
        self.process.stdin.close()
        self.process.stdout.close()


def main():
    binary, cli = (Path(arg).resolve(strict=True) for arg in sys.argv[1:3])
    app = sys.argv[3]
    assert app in ("find", "look", "lens", "box", "clipboard")
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT="1")
    if app == "find":
        config = Path(env["ARCADE_FIND_HOME"]) / "config/settings.json"
        config.parent.mkdir(parents=True, exist_ok=True)
        tree = ROOT / "tray-files"
        tree.mkdir()
        config.write_text(json.dumps({"schema": 1, "roots": [str(tree)]}))
    watcher = Watcher()
    processes, hosts, latencies = [], [], []
    log_path = ROOT / f"{app}-tray-registration.log"

    def start():
        with log_path.open("ab") as log:
            process = subprocess.Popen([str(binary), "--background"], env=env, stdout=log, stderr=log)
        processes.append(process)
        return process

    def host():
        result = Host(cli, env)
        hosts.append(result)
        return result

    def status():
        result = subprocess.run([str(cli), "status", app], env=env, capture_output=True, text=True, timeout=3)
        if result.returncode != 0:
            return {}
        answer = json.loads(result.stdout)
        # app.status carries pid beside the app's own status object.
        return {**answer.get("status", {}), "pid": answer.get("pid")}

    def ownership(state):
        until(lambda: status().get("tray") == state, f"status.tray did not become {state}")

    def items(count, timeout=1):
        try:
            until(lambda: len(watcher.properties()["RegisteredStatusNotifierItems"]) == count,
                  f"expected {count} registered items", timeout)
        except AssertionError as error:
            states = []
            for name, path, _ in watcher.items.values():
                try:
                    states.append(str(dbus.Interface(watcher.bus.get_object(name, path), PROPERTIES).Get(ITEM, "Status")))
                except dbus.DBusException as gone:
                    states.append(str(gone))
            raise AssertionError(f"{error}; registered={list(watcher.items)}, SNI Status={states}") from error

    def menu():
        items(1)
        name, path, _ = next(iter(watcher.items.values()))
        properties = dbus.Interface(watcher.bus.get_object(name, path), PROPERTIES)
        identity = str(properties.Get(ITEM, "Id"))
        # ksni uses the app's id; Tauri's libappindicator prefixes its tray id.
        assert identity in ("arcade-" + app, f"tray-icon tray app arcade-{app}-tray"), identity
        assert str(properties.Get(ITEM, "Status")) == "Active"
        assert str(properties.Get(ITEM, "Title")) == "Arcade " + app.title()
        try:
            icon = properties.Get(ITEM, "IconPixmap")
        except dbus.DBusException:
            icon = str(properties.Get(ITEM, "IconName"))  # libappindicator
        assert icon, "missing icon after re-registration"
        exported = dbus.Interface(watcher.bus.get_object(name, str(properties.Get(ITEM, "Menu"))), MENU)
        _, layout = exported.GetLayout(dbus.Int32(0), dbus.Int32(-1), dbus.Array([], signature="s"))
        labels = {str(node[1]["label"]): int(node[0]) for node in layout[2] if "label" in node[1]}
        assert {"Open Settings", "Restart Arcade " + app.title(), "Quit Arcade " + app.title()} <= labels.keys()
        return exported, labels

    def click(label):
        exported, labels = menu()
        # Restart/Quit may disconnect before the method reply is delivered.
        exported.Event(dbus.Int32(labels[label]), "clicked", dbus.Int32(0), dbus.UInt32(0),
                       reply_handler=lambda: None, error_handler=lambda _error: None)

    def recovery(tools):
        before = time.monotonic()
        tools.kill()
        items(1, max(0.001, 1 - (time.monotonic() - before)))
        latency = (time.monotonic() - before) * 1000
        assert latency < 1000, latency
        latencies.append(latency)
        ownership("own")

    try:
        process = start()
        ownership("own")
        original_menu = menu()[1]
        tools = host()
        ownership("hosted")
        items(0)  # The old Passive implementation fails here.
        tools.commands("exclude " + app)
        ownership("own")
        assert menu()[1] == original_menu
        tools.commands("include " + app)
        ownership("hosted")
        items(0)
        recovery(tools)

        # Both completed transitions and a burst exercise shutdown/spawn races.
        settle(0.3)
        pid = status()["pid"]
        baseline_threads = len(list(Path(f"/proc/{pid}/task").iterdir()))
        tools = host()
        ownership("hosted")
        items(0)
        for _ in range(12):
            tools.commands("exclude " + app)
            ownership("own")
            items(1)
            tools.commands("include " + app)
            ownership("hosted")
            items(0)
        tools.commands(*(["exclude " + app, "include " + app] * 20), "exclude " + app)
        ownership("own")
        assert menu()[1] == original_menu
        def threads():
            names = []
            for task in os.listdir(f"/proc/{pid}/task"):
                try:
                    names.append((Path(f"/proc/{pid}/task") / task / "comm").read_text().strip())
                except FileNotFoundError:
                    pass  # a short-lived thread exited meanwhile
            return sorted(names)
        try:
            # tokio's blocking pool keeps idle threads for 10 s, so allow that to pass.
            until(lambda: len(threads()) <= baseline_threads + 2, "tray cycles leaked worker threads", timeout=15)
        except AssertionError as error:
            raise AssertionError(f"{error}: {baseline_threads} before, now {threads()}") from error
        tools.commands("include " + app)
        ownership("hosted")
        items(0)

        registered_before = watcher.registrations
        tools.commands("restarting")
        tools.kill()
        settle(0.3)
        assert status()["tray"] == "hosted" and not watcher.items
        tools = host()
        ownership("hosted")
        settle(0.3)
        assert watcher.registrations == registered_before, "icon flashed during Tools restart grace"
        recovery(tools)

        click("Open Settings")
        def settings_visible():
            result = subprocess.run(["xdotool", "search", "--onlyvisible", "--name", "Arcade " + app.title()],
                                    env=env, capture_output=True, timeout=3)
            return result.returncode == 0
        until(settings_visible, "rebuilt tray Settings action did not open a window")
        old_pid = status()["pid"]
        until(lambda: status().get("phase", "ready") == "ready", "Find index not ready for Restart")
        click("Restart Arcade " + app.title())
        until(lambda: status().get("pid", old_pid) != old_pid, "rebuilt tray Restart action did not restart", timeout=15)
        process.wait(timeout=10)
        ownership("own")
        assert menu()[1] == original_menu
        click("Quit Arcade " + app.title())
        until(lambda: not status(), "rebuilt tray Quit action did not stop the app")
        items(0)

        # Tools first: no SNI registration at all until hand-back.
        tools = host()
        registered_before = watcher.registrations
        process = start()
        ownership("hosted")
        settle(0.3)
        assert not watcher.items and watcher.registrations == registered_before, "hosted startup registered an icon"
        recovery(tools)
        click("Quit Arcade " + app.title())
        process.wait(timeout=10)
        items(0)
        assert watcher.maximum == 1, f"overlapping SNI registrations: {watcher.maximum}"
        print(f"{app}: 9 tray checks passed (Own, Hosted removal, exclude/include, crash recovery, "
              f"rapid toggles/thread cleanup, restart grace, menu handlers, app restart, hosted startup); "
              f"hand-back ms: {', '.join(f'{value:.1f}' for value in latencies)}; max items={watcher.maximum}")
    except BaseException:
        print(log_path.read_text(errors="replace"), file=sys.stderr)
        raise
    finally:
        for tools in hosts:
            if tools.process.poll() is None:
                tools.kill()
        # A menu Restart successor is detached but still in this private profile.
        subprocess.run([str(binary), "--quit"], env=env, capture_output=True, timeout=5)
        for process in processes:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=5)
        watcher.remove_from_connection()


if __name__ == "__main__":
    main()
