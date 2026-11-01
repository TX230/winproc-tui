"""Verify Windows Terminal environment isolation with an owned, short-lived probe.

Only boolean checks for color/terminal variables and a test marker are recorded.
No global environment, Terminal settings, or application configuration is changed.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import uuid
import winreg


def normal_no_color():
    for hive, key in [
        (winreg.HKEY_CURRENT_USER, r"Environment"),
        (winreg.HKEY_LOCAL_MACHINE, r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment"),
    ]:
        try:
            with winreg.OpenKey(hive, key) as handle:
                value, _ = winreg.QueryValueEx(handle, "NO_COLOR")
                return bool(value)
        except FileNotFoundError:
            pass
    return False


def run(output):
    output.parent.mkdir(parents=True, exist_ok=True)
    root = output.parent.resolve() / ("mcp-environment-" + uuid.uuid4().hex)
    root.mkdir()
    source = root / "probe.rs"
    executable = root / "probe.exe"
    result = root / "probe-result.txt"
    source.write_text(r'''
use std::{env, fs};
fn main() {
    let directory = env::current_exe().unwrap().parent().unwrap().to_owned();
    let flags = [
        env::var("NO_COLOR").is_ok_and(|s| !s.is_empty()),
        env::var("TERM").is_ok_and(|s| s == "dumb"),
        env::var("COLORTERM").is_ok_and(|s| s.is_empty()),
        env::var_os("WINPROC_MCP_ENV_TEST").is_some(),
    ];
    fs::write(directory.join("probe-result.txt"),
        flags.map(|b| if b { "1" } else { "0" }).join(" ")).unwrap();
}
''', encoding="utf-8")
    subprocess.run(["rustc", str(source), "-o", str(executable)], check=True, capture_output=True)
    terminal = Path(os.environ["LOCALAPPDATA"]) / "Microsoft/WindowsApps/wt.exe"
    env = os.environ.copy()
    env.update(NO_COLOR="1", TERM="dumb", COLORTERM="", WINPROC_MCP_ENV_TEST="1")
    evidence = {"expected_user_no_color": normal_no_color(), "checks": {}}
    for option in ["--inheritEnvironment", "--reloadEnvironment"]:
        if result.exists():
            result.unlink()
        command = [str(terminal), "-w", "new", "new-tab", "--title",
            "WinProc environment test", "--suppressApplicationTitle", option,
            "-d", str(root).replace(";", r"\;"), "--", str(executable).replace(";", r"\;")]
        launcher = subprocess.Popen(command, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic()+12
            while not result.exists() and time.monotonic() < deadline:
                time.sleep(.05)
            assert result.exists(), "Terminal probe did not finish"
            values = result.read_text(encoding="utf-8").strip().split()
            assert len(values) == 4
            flags = dict(zip(["no_color", "term_dumb", "empty_colorterm", "client_marker"],
                [value == "1" for value in values]))
            evidence["checks"][option] = flags
            if option == "--inheritEnvironment":
                assert all(flags.values()), flags
            else:
                assert not flags["client_marker"], flags
                assert flags["no_color"] == evidence["expected_user_no_color"], flags
            print("PASS", option, json.dumps(flags), flush=True)
        finally:
            launcher.wait(timeout=12)
            output.write_text(json.dumps(evidence, indent=2), encoding="utf-8")
    evidence["passed"] = True
    output.write_text(json.dumps(evidence, indent=2), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    run(parser.parse_args().output)
