#!/usr/bin/env python3
"""Add AGPL SPDX headers to source files that lack them."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

HEADER_LINES = [
    "// SPDX-License-Identifier: AGPL-3.0-or-later",
    "// Copyright (C) 2026 Mycelium Project",
    "",
]

EXTENSIONS = {".rs", ".kt", ".kts", ".js", ".svelte", ".udl"}

SKIP_PARTS = (
    "/gen/schemas/",
    "package-lock.json",
    "/uniffi/mycelium/mycelium.kt",
    "/node_modules/",
)


def should_process(path: Path) -> bool:
    if path.suffix not in EXTENSIONS:
        return False
    s = str(path)
    return not any(part in s for part in SKIP_PARTS)


def has_spdx(text: str) -> bool:
    return "SPDX-License-Identifier" in text


def add_header(text: str) -> str:
    header = "\n".join(HEADER_LINES)
    stripped = text.lstrip("\ufeff")
    if stripped.startswith("#!"):
        first_nl = stripped.find("\n")
        if first_nl == -1:
            return stripped + "\n" + header
        return stripped[: first_nl + 1] + header + stripped[first_nl + 1 :]
    return header + stripped


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT
    changed = 0
    for path in sorted(root.rglob("*")):
        if not path.is_file() or not should_process(path):
            continue
        original = path.read_text(encoding="utf-8")
        if has_spdx(original):
            continue
        path.write_text(add_header(original), encoding="utf-8")
        changed += 1
        print(path.relative_to(root))
    print(f"Updated {changed} files under {root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
