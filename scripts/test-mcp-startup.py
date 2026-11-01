"""Visible MCP startup checks. Use an isolated executable/config; no TUI may be running.

The launch phase opens a real Windows Terminal window and deliberately leaves
the TUI open for human/desktop verification. Close that TUI normally afterward.
Negative fixtures use private child environments and never alter OS settings.
"""
import argparse
import concurrent.futures
import ctypes
from ctypes import wintypes
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import time
import uuid

spec = importlib.util.spec_from_file_location("mcp_system", Path(__file__).with_name("test-mcp-system.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
Bridge = module.Bridge

kernel = ctypes.WinDLL("kernel32", use_last_error=True)
kernel.CreateMutexW.argtypes = [ctypes.c_void_p, wintypes.BOOL, wintypes.LPCWSTR]
kernel.CreateMutexW.restype = wintypes.HANDLE
kernel.OpenMutexW.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.LPCWSTR]
kernel.OpenMutexW.restype = wintypes.HANDLE
kernel.CloseHandle.argtypes = [wintypes.HANDLE]
INSTANCE = r"Local\TX230.winproc-tui.SingleInstance"
LAUNCH = r"Local\TX230.winproc-tui.McpLaunch"

def present(name):
    handle = kernel.OpenMutexW(0x100000, False, name)
    if handle:
        kernel.CloseHandle(handle)
        return True
    error = ctypes.get_last_error()
    assert error == 2, error
    return False

def fixture_environment(root, mode):
    env = os.environ.copy()
    env["LOCALAPPDATA"] = str(root)
    env["WINPROC_LAUNCH_FIXTURE_MODE"] = mode
    env["WINPROC_LAUNCH_FIXTURE_OUTPUT"] = str(root / "calls.jsonl")
    return env

def build_launcher_fixture(root):
    output = root / "Microsoft/WindowsApps/wt.exe"
    output.parent.mkdir(parents=True, exist_ok=True)
    source = root / "launcher.rs"
    source.write_text(r'''
use std::{env, fs::OpenOptions, io::Write};
fn main() {
    let mut file = OpenOptions::new().create(true).append(true)
        .open(env::var_os("WINPROC_LAUNCH_FIXTURE_OUTPUT").unwrap()).unwrap();
    writeln!(file, "{:?}", env::args_os().collect::<Vec<_>>()).unwrap();
    // Noise must never appear on the bridge's MCP stdout or stderr.
    println!("LAUNCHER STDOUT"); eprintln!("LAUNCHER STDERR");
    if env::var("WINPROC_LAUNCH_FIXTURE_MODE").unwrap() == "fail" {
        std::process::exit(7);
    }
}
''', encoding="utf-8")
    subprocess.run(["rustc", str(source), "-o", str(output)], check=True, capture_output=True)
    return root / "calls.jsonl"


def stop_owned_tui(exe, pid):
    """Send a console termination signal, not UI input; verify exact image first."""
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.QueryFullProcessImageNameW.argtypes = [wintypes.HANDLE, wintypes.DWORD, wintypes.LPWSTR, ctypes.POINTER(wintypes.DWORD)]
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.AttachConsole.argtypes = [wintypes.DWORD]
    kernel.GenerateConsoleCtrlEvent.argtypes = [wintypes.DWORD, wintypes.DWORD]
    process = kernel.OpenProcess(0x101000, False, pid)
    assert process, ctypes.WinError(ctypes.get_last_error())
    try:
        buffer = ctypes.create_unicode_buffer(32768)
        size = wintypes.DWORD(len(buffer))
        assert kernel.QueryFullProcessImageNameW(process, 0, buffer, ctypes.byref(size))
        assert os.path.samefile(buffer.value, exe), "Refuse to signal a different executable"
        kernel.FreeConsole()
        assert kernel.AttachConsole(pid), ctypes.WinError(ctypes.get_last_error())
        callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.DWORD)
        handler = callback_type(lambda event: True)
        kernel.SetConsoleCtrlHandler.argtypes = [callback_type, wintypes.BOOL]
        assert kernel.SetConsoleCtrlHandler(handler, True)
        try:
            assert kernel.GenerateConsoleCtrlEvent(1, 0)  # CTRL_BREAK, normal app cleanup
            assert kernel.WaitForSingleObject(process, 8000) == 0, "TUI did not exit cleanly"
        finally:
            kernel.FreeConsole()
            kernel.SetConsoleCtrlHandler(handler, False)
        return "Owned TUI exited through its console-control cleanup"
    finally:
        kernel.CloseHandle(process)

def run(exe, output, phase, pid=None):
    exe = exe.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    evidence = {"phase": phase, "checks": [], "exe": str(exe)}
    bridges = []
    def bridge(**kwargs):
        item = Bridge(exe, timeout=30, **kwargs)
        bridges.append(item)
        return item
    def check(name, action):
        start = time.monotonic()
        try:
            details = action()
            evidence["checks"].append({"name": name, "passed": True,
                "seconds": round(time.monotonic()-start, 3), "details": details})
            print("PASS", name, flush=True)
            return details
        except Exception as error:
            evidence["checks"].append({"name": name, "passed": False, "error": str(error)})
            raise
        finally:
            output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
    def unavailable(item):
        result = item.tool("get_status", error="BACKEND_UNAVAILABLE")
        return result["error"]["message"]
    try:
        if phase == "alias":
            assert not present(INSTANCE)
            root = exe.parent / ("alias-fixture-" + uuid.uuid4().hex)
            assert not root.exists(), "Use a fresh alias fixture directory"
            (root / "real").mkdir(parents=True)
            (root / "links").mkdir()
            real_exe = root / "real/winproc-tui.exe"
            alias_exe = root / "links/winproc-tui.exe"
            shutil.copyfile(exe, real_exe)
            alias_exe.symlink_to(real_exe)
            legacy = alias_exe.with_name("winproc-tui.toml")
            active = real_exe.with_name("winproc-tui.toml")
            item = Bridge(alias_exe, timeout=30); bridges.append(item)
            def legacy_disabled():
                raw = "[general]\nmcp_connections_disabled = true\n"
                legacy.write_text(raw, encoding="utf-8")
                message = unavailable(item)
                assert "disabled" in message and not present(INSTANCE)
                assert legacy.read_text(encoding="utf-8") == raw and not active.exists()
                return message
            check("disabled_legacy_config_prevents_launch_without_migration", legacy_disabled)
            def legacy_enabled():
                legacy.write_text('[general]\nmouse = false\n[investigation]\nstartup = "resume_last"\ntracked_names = ["winproc-tui.exe"]\n', encoding="utf-8")
                result = item.tool("get_tracking")
                assert result["data"]["tracked_names"] == ["winproc-tui.exe"], result
                assert active.exists() and not legacy.exists(), "Ordinary TUI startup must migrate the legacy config"
                evidence["backend_instance_id"] = result["backend_instance_id"]
                evidence["launched_exe"] = str(real_exe)
                return result
            check("alias_launch_preserves_normal_config_migration_and_tracking", legacy_enabled)
        elif phase == "shutdown":
            assert pid, "--pid is required for shutdown"
            check("normal_console_control_shutdown", lambda: stop_owned_tui(exe, pid))
            assert not present(INSTANCE)
            assert exe.with_name("winproc-tui.toml").exists(), "Successful exit must persist config"
        elif phase in ("launch", "negative"):
            assert not present(INSTANCE), "An existing TUI must be left untouched; close it manually first."
            assert not present(LAUNCH), "Another bridge is starting a TUI."
            config = exe.with_name("winproc-tui.toml")
            assert not config.exists(), "Use a fresh isolated executable directory."
            fixture = exe.parent / "launcher-fixture"
            calls = build_launcher_fixture(fixture)
            b = bridge()
            def discovery():
                assert len(b.call("tools/list", {})["tools"]) == 26
                b.tool("get_status", {"unexpected": True}, error="INVALID_ARGUMENT")
                b.call("ping", {})
                assert not present(INSTANCE) and not present(LAUNCH)
                assert not config.exists()
                return "No application marker or config created"
            check("discovery_and_invalid_calls_do_not_launch", discovery)
            def disabled():
                raw = '[general]\nmcp_connections_disabled = true\n'
                config.write_text(raw, encoding="utf-8")
                message = unavailable(b)
                assert "disabled" in message
                assert config.read_text(encoding="utf-8") == raw
                assert not present(INSTANCE)
                config.unlink()
                return message
            check("saved_disabled_prevents_launch_and_writes", disabled)
            def malformed():
                config.write_text("invalid = [", encoding="utf-8")
                message = unavailable(b)
                assert "configuration" in message
                assert config.read_text(encoding="utf-8") == "invalid = ["
                config.unlink()
                return message
            check("malformed_config_does_not_launch", malformed)
            def missing():
                missing_root = fixture / "missing"
                item = bridge(env=fixture_environment(missing_root, "fail"))
                message = unavailable(item); item.close()
                assert "Windows Terminal" in message and not present(INSTANCE)
                relative = bridge(env=fixture_environment(Path("relative-app-data"), "fail"))
                relative_message = unavailable(relative); relative.close()
                assert relative_message.startswith("Windows Terminal is unavailable.")
                return {"missing_alias": message, "relative_localappdata": relative_message}
            check("missing_terminal_returns_actionable_error", missing)
            def failure():
                item = bridge(env=fixture_environment(fixture, "fail"))
                message = unavailable(item); item.close()
                assert "rejected" in message
                assert len(calls.read_text(encoding="utf-8").splitlines()) == 1
                assert not present(INSTANCE)
                assert b"LAUNCHER" not in item.process.stderr.read().encode()
                calls.unlink()
                return message
            check("launcher_nonzero_exit_and_stdio_isolation", failure)
            def timeout():
                item = bridge(env=fixture_environment(fixture, "noop"))
                start = time.monotonic()
                message = unavailable(item); item.close()
                elapsed = time.monotonic()-start
                assert 11 <= elapsed <= 15, elapsed
                assert "12 seconds" in message
                assert len(calls.read_text(encoding="utf-8").splitlines()) == 1
                assert not present(INSTANCE) and not present(LAUNCH)
                calls.unlink()
                return message
            check("launcher_success_is_not_readiness_and_does_not_loop", timeout)
            def marker(name):
                handle = kernel.CreateMutexW(None, False, name)
                assert handle and ctypes.get_last_error() != 183
                try:
                    item = bridge(env=fixture_environment(fixture, "fail"))
                    message = unavailable(item); item.close()
                    assert not calls.exists(), "Existing/coordinating instance must prevent launch"
                    return message
                finally:
                    kernel.CloseHandle(handle)
            check("existing_application_without_pipe_is_not_replaced", lambda: marker(INSTANCE))
            check("another_launch_coordinator_is_not_bypassed", lambda: marker(LAUNCH))
            def begin_status(item):
                item.sequence += 1
                request_id = item.sequence
                item.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": request_id,
                    "method": "tools/call", "params": {"name": "winproc_get_status", "arguments": {}}}) + "\n")
                item.process.stdin.flush()
                return request_id
            def cancellation_before_launch():
                handle = kernel.CreateMutexW(None, False, LAUNCH)
                assert handle and ctypes.get_last_error() != 183
                item = bridge(env=fixture_environment(fixture, "fail"))
                try:
                    request_id = begin_status(item)
                    time.sleep(.2)
                    item.notify("notifications/cancelled", {"requestId": request_id, "reason": "system test"})
                    time.sleep(.2)
                    kernel.CloseHandle(handle); handle = None
                    item.call("ping", {})
                    time.sleep(.3)
                    assert not calls.exists() and not present(INSTANCE)
                    return "Cancelled coordinator waiter did not launch after the gate was released"
                finally:
                    if handle: kernel.CloseHandle(handle)
                    item.close()
            check("cancel_before_launch_prevents_launch", cancellation_before_launch)
            def cancellation_after_launch(eof=False):
                item = bridge(env=fixture_environment(fixture, "noop"))
                request_id = begin_status(item)
                deadline = time.monotonic()+3
                while not calls.exists() and time.monotonic() < deadline: time.sleep(.05)
                assert calls.exists()
                if eof:
                    item.close()
                    assert item.process.returncode == 0, "EOF must exit without harness termination"
                    assert not present(LAUNCH)
                else:
                    item.notify("notifications/cancelled", {"requestId": request_id, "reason": "system test"})
                    item.call("ping", {})
                    time.sleep(.2)
                    assert present(LAUNCH), "Cancellation must not immediately release an in-flight launch"
                    deadline = time.monotonic()+13
                    while present(LAUNCH) and time.monotonic() < deadline: time.sleep(.1)
                    assert not present(LAUNCH)
                    item.close()
                assert len(calls.read_text(encoding="utf-8").splitlines()) == 1
                calls.unlink()
                return "One launch, no replay, coordinator released"
            check("cancel_after_launch_keeps_coordination_until_deadline", cancellation_after_launch)
            check("bridge_eof_during_startup_releases_coordination", lambda: cancellation_after_launch(True))
            b.close()
            assert not config.exists()
            if phase == "negative":
                return
            # Two independent stdio clients race the first actual tool call.
            one, two = bridge(), bridge()
            def simultaneous():
                with concurrent.futures.ThreadPoolExecutor(2) as pool:
                    results = list(pool.map(lambda item: item.tool("get_status"), [one, two]))
                assert results[0]["backend_instance_id"] == results[1]["backend_instance_id"]
                assert present(INSTANCE)
                evidence["backend_instance_id"] = results[0]["backend_instance_id"]
                return results[0]["backend_instance_id"]
            check("concurrent_bridges_connect_to_one_visible_backend", simultaneous)
            def reuse():
                for item in [bridge(), bridge()]:
                    result = item.tool("get_status")
                    assert result["backend_instance_id"] == evidence["backend_instance_id"]
                extra = bridge()
                message = unavailable(extra); extra.close()
                assert "capacity" in message
                return "Four clients share the backend; fifth is rejected without replacement"
            check("existing_backend_reuse_and_connection_capacity", reuse)
            for item in bridges: item.close()
            time.sleep(.5)
            def survives():
                assert present(INSTANCE)
                item = bridge()
                result = item.tool("get_status")
                assert result["backend_instance_id"] == evidence["backend_instance_id"]
                item.close()
                return result
            check("backend_survives_all_bridge_eof", survives)
        elif phase == "disabled":
            assert present(INSTANCE)
            message = check("runtime_disabled_application_is_not_replaced",
                lambda: unavailable(bridge()))
            assert "running" in message
        elif phase == "reconnect":
            assert present(INSTANCE)
            result = check("reconnect_after_runtime_enable", lambda: bridge().tool("get_status"))
            evidence["backend_instance_id"] = result["backend_instance_id"]
        elif phase == "restart":
            item = bridge()
            previous = None
            if pid:
                previous = item.tool("get_status")["backend_instance_id"]
                check("normal_console_control_shutdown", lambda: stop_owned_tui(exe, pid))
                check("lost_connection_is_not_automatically_replayed",
                    lambda: item.tool("get_status", error="OUTCOME_UNKNOWN"))
            assert not present(INSTANCE)
            result = check("visible_restart_after_normal_tui_exit", lambda: item.tool("get_status"))
            evidence["backend_instance_id"] = result["backend_instance_id"]
            if previous:
                assert result["backend_instance_id"] != previous
                evidence["previous_backend_instance_id"] = previous
    finally:
        for item in bridges:
            item.close()
        evidence["application_left_running"] = present(INSTANCE)
        output.write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("exe", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--phase", choices=["launch", "negative", "disabled", "reconnect", "restart", "shutdown", "alias"], default="launch")
    parser.add_argument("--pid", type=int)
    args = parser.parse_args()
    run(args.exe, args.output, args.phase, args.pid)
