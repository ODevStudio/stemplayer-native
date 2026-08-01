from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from urllib.parse import unquote


MAX_AGENTS_BYTES = 4096
MAX_AGENTS_LINES = 80
MOJIBAKE = ("Ãƒ", "Ã‚", "Ã¢â‚¬")
MARKDOWN_LINK = re.compile(r"(?<!!)\[[^\]]+\]\(\s*(<[^>]+>|[^)\s]+)")
ACTION_REFERENCE = re.compile(r"^\s*-\s*uses:\s*([^\s@]+)@([^\s#]+)")
AUTHORITY_CLAIM = re.compile(
    r"(?i)(?:\bSTATUS\.md\b.{0,40}\b(?:only|sole|single)\b.{0,30}\b(?:authority|authoritative)\b|\b(?:only|sole|single)\b.{0,30}\b(?:authority|authoritative)\b.{0,40}\bSTATUS\.md\b)"
)
PRIVATE_PATH = re.compile(r"(?i)(?:\b[a-z]:[\\/]|/users/|/home/|/private/var/|\\\\)")
CREDENTIAL_VALUE = re.compile(
    r"(?i)\b(?:api[_ -]?key|access[_ -]?token|password|secret)\b\s*[:=]\s*[\"']?[^\s\"']+"
)


def markdown_files(root: Path) -> list[Path]:
    return sorted(
        path
        for path in root.rglob("*.md")
        if not {".git", "target"}.intersection(path.parts)
    )


def read_markdown(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_markdown(root: Path) -> list[str]:
    root = root.resolve()
    issues: list[str] = []
    authority_files: list[Path] = []

    for path in markdown_files(root):
        relative = path.relative_to(root).as_posix()
        text = read_markdown(path)
        if any(sequence in text for sequence in MOJIBAKE):
            issues.append(f"{relative}: contains mojibake")
        if "docs/archive/" in text.lower():
            issues.append(f"{relative}: references deleted docs/archive material")
        if PRIVATE_PATH.search(text):
            issues.append(f"{relative}: contains an absolute or private path")
        if CREDENTIAL_VALUE.search(text):
            issues.append(f"{relative}: contains credential-like material")
        if AUTHORITY_CLAIM.search(text):
            authority_files.append(path)

        for match in MARKDOWN_LINK.finditer(text):
            target = match.group(1).strip("<>")
            target = unquote(target.split("#", 1)[0].split("?", 1)[0])
            if not target or target.startswith(("http:", "https:", "mailto:", "tel:", "//")):
                continue
            candidate = (path.parent / target).resolve()
            try:
                candidate.relative_to(root)
            except ValueError:
                issues.append(f"{relative}: link escapes repository: {target}")
                continue
            if not candidate.exists():
                issues.append(f"{relative}: broken link: {target}")

    if len(authority_files) > 1:
        names = ", ".join(path.relative_to(root).as_posix() for path in authority_files)
        issues.append(f"multiple current-status authority claims: {names}")

    agents = root / "AGENTS.md"
    if agents.exists():
        content = agents.read_text(encoding="utf-8")
        if len(content.encode("utf-8")) > MAX_AGENTS_BYTES:
            issues.append(f"AGENTS.md: exceeds {MAX_AGENTS_BYTES}-byte budget")
        if len(content.splitlines()) > MAX_AGENTS_LINES:
            issues.append(f"AGENTS.md: exceeds {MAX_AGENTS_LINES}-line budget")

    return issues


def check_workflows(root: Path) -> list[str]:
    root = root.resolve()
    workflow_root = root / ".github" / "workflows"
    if not workflow_root.exists():
        return []

    issues: list[str] = []
    for path in sorted(workflow_root.glob("*.y*ml")):
        relative = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()
        for line_number, line in enumerate(lines, 1):
            match = ACTION_REFERENCE.match(line)
            if not match or match.group(1).startswith("./"):
                continue
            action, reference = match.groups()
            if not re.fullmatch(r"[0-9a-f]{40}", reference):
                issues.append(f"{relative}:{line_number}: action {action} is not pinned to a full SHA")
            comment = line.split("#", 1)[1] if "#" in line else ""
            if action not in comment or not re.search(r"\bv\d", comment):
                issues.append(f"{relative}:{line_number}: action {action} needs its release tag comment")

        required = {
            "name:": "workflow name",
            "permissions:": "least-privilege permissions",
            "concurrency:": "concurrency cancellation",
            "timeout-minutes:": "job timeout",
            "fail-fast: false": "non-failing matrix behavior",
            "--workspace": "workspace-wide Cargo coverage",
            "cargo build --locked --release -p stemplayer": "Windows release build",
        }
        for needle, description in required.items():
            if needle not in text:
                issues.append(f"{relative}: missing {description}")
        if "cargo deny list" in text:
            issues.append(f"{relative}: cargo-deny inventory command is not an enforcing check")

    return issues


def find_issues(root: Path) -> list[str]:
    resolved_root = root.resolve()
    return check_markdown(resolved_root) + check_workflows(resolved_root)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    args = parser.parse_args()
    root = args.root.resolve()
    issues = find_issues(root)
    if issues:
        print("repository checks failed:")
        print("\n".join(f"- {issue}" for issue in issues))
        return 1
    print("repository checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
