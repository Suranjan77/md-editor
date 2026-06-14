#!/usr/bin/env python3
"""Check V3 GUI tooling and inspect the live AT-SPI tree."""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


YDOTOOL_SOCKET_CANDIDATES = (
    Path(os.environ["YDOTOOL_SOCKET"])
    if "YDOTOOL_SOCKET" in os.environ
    else None,
    Path(f"/run/user/{os.getuid()}/.ydotool_socket"),
    Path("/tmp/.ydotool_socket"),
)


def command_output(command: list[str]) -> str:
    result = subprocess.run(
        command,
        check=False,
        capture_output=True,
        text=True,
    )
    return (result.stdout or result.stderr).strip()


def load_tree() -> Any:
    try:
        from dogtail import tree
    except (ImportError, SystemExit) as error:
        print(f"Dogtail unavailable: {error}", file=sys.stderr)
        raise SystemExit(1) from error
    return tree


def check_socket() -> str:
    failures = []
    for candidate in YDOTOOL_SOCKET_CANDIDATES:
        if candidate is None or not candidate.exists():
            continue
        environment = os.environ.copy()
        environment["YDOTOOL_SOCKET"] = str(candidate)
        result = subprocess.run(
            ["ydotool", "mousemove", "-x", "0", "-y", "0"],
            check=False,
            capture_output=True,
            text=True,
            env=environment,
        )
        if result.returncode == 0:
            return f"usable ({candidate})"
        failures.append(f"{candidate}: {(result.stderr or result.stdout).strip()}")
    if failures:
        return f"unusable ({'; '.join(failures)})"
    return "missing"


def check_tools() -> int:
    failed = False
    for binary in ("ydotool", "cosmic-screenshot"):
        path = shutil.which(binary)
        print(f"{binary}: {path or 'missing'}")
        failed |= path is None

    try:
        import cv2

        print(f"opencv: {cv2.__version__}")
    except ImportError as error:
        print(f"opencv: missing ({error})")
        failed = True

    accessibility = command_output(
        ["gsettings", "get", "org.gnome.desktop.interface", "toolkit-accessibility"]
    )
    print(f"toolkit-accessibility: {accessibility or 'unknown'}")
    failed |= accessibility != "true"

    daemon = command_output(["systemctl", "is-active", "ydotool.service"])
    print(f"ydotool.service: {daemon or 'unknown'}")
    failed |= daemon != "active"

    socket_state = check_socket()
    print(f"ydotool socket: {socket_state}")
    failed |= not socket_state.startswith("usable")

    tree = load_tree()
    applications = [node.name for node in tree.root.children]
    print(f"AT-SPI applications: {applications}")
    return int(failed)


def safe_value(node: Any, attribute: str, default: Any) -> Any:
    try:
        return getattr(node, attribute)
    except Exception:
        return default


def print_node(node: Any, depth: int, max_depth: int) -> None:
    indent = "  " * depth
    name = safe_value(node, "name", "")
    role = safe_value(node, "roleName", "unknown")
    position = safe_value(node, "position", None)
    size = safe_value(node, "size", None)
    actions = safe_value(node, "actions", [])
    print(
        f"{indent}{role}: {name!r} pos={position} size={size} "
        f"actions={list(actions)}"
    )
    if depth >= max_depth:
        return
    for child in safe_value(node, "children", []):
        print_node(child, depth + 1, max_depth)


def print_tree(app_pattern: str | None, depth: int) -> int:
    tree = load_tree()
    applications = list(tree.root.children)
    if app_pattern:
        pattern = re.compile(app_pattern, re.IGNORECASE)
        applications = [node for node in applications if pattern.search(node.name)]
        if not applications:
            print(f"No AT-SPI application matches {app_pattern!r}", file=sys.stderr)
            return 1
    for application in applications:
        print_node(application, 0, depth)
    return 0


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("check", help="verify installed GUI test tools")
    tree_parser = subparsers.add_parser("tree", help="dump live AT-SPI nodes")
    tree_parser.add_argument("--app", help="case-insensitive application regex")
    tree_parser.add_argument("--depth", type=int, default=3)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.command == "check":
        return check_tools()
    return print_tree(args.app, max(0, args.depth))


if __name__ == "__main__":
    raise SystemExit(main())
