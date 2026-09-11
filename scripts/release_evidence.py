"""Offline, unsigned Windows build evidence. This is CI tooling, not a player dependency."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path
from urllib.parse import quote

MAX_JSON = 16 * 1024 * 1024
MAX_BINARY = 128 * 1024 * 1024
MAX_LICENSE = 1024 * 1024
MAX_NOTICES = 32 * 1024 * 1024
MAX_PACKAGES = 4096
REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
SAFE_NAME = re.compile(r"[A-Za-z0-9_.+-]{1,128}\Z")
FEATURE = re.compile(r"[a-z0-9_-]{1,64}\Z")
IDENTITY_KEYS = {
    "schema_version", "package", "version", "source_revision", "source_dirty",
    "rustc", "toolchain", "toolchain_sha256", "cargo_lock_sha256", "target",
    "profile", "features",
}
PAYLOAD_NAMES = {"stemplayer.exe", "stemplayer-support.exe"}
EVIDENCE_NAMES = {
    "build-info.json", "sbom.cdx.json", "THIRD-PARTY-NOTICES.txt",
    "release-manifest.json", "artifact-scan.json", "SHA256SUMS",
}
PATTERNS = {
    "private-key": rb"-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----",
    "github-token": rb"(?:gh[pousr]_[A-Za-z0-9]{36,255}|github_pat_[A-Za-z0-9_]{40,255})",
    "aws-access-key": rb"(?:AKIA|ASIA)[A-Z0-9]{16}",
    "private-path": rb"(?i)(?:/[Hh]ome/|/[Uu]sers/|/private/var/|[A-Z]:[\\/](?:Users|a)[\\/])",
    "debug-material": rb"(?:RSDS|NB10|\.debug_info\x00|\.zdebug_info\x00)",
}


class EvidenceError(ValueError):
    """Messages must contain rule names, never source paths or matched secrets."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise EvidenceError(message)


def canonical(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode()


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_regular(path: Path, limit: int) -> bytes:
    """Reject symlinks/reparse points and mutation; do not follow a final link."""
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and not path.is_symlink(), "non-regular input")
    require(not getattr(before, "st_file_attributes", 0) & 0x400, "reparse-point input")
    require(before.st_size <= limit, "input size limit exceeded")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path, flags)
    with os.fdopen(fd, "rb") as handle:
        opened = os.fstat(handle.fileno())
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
                (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns), "input changed")
        data = handle.read(limit + 1)
        after = os.fstat(handle.fileno())
    require(len(data) <= limit, "input size limit exceeded")
    require((opened.st_size, opened.st_mtime_ns) == (after.st_size, after.st_mtime_ns), "input changed")
    require(len(data) == after.st_size, "input changed")
    return data


def no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def read_json(path: Path, limit: int = MAX_JSON) -> dict:
    value = json.loads(read_regular(path, limit), object_pairs_hook=no_duplicate_keys)
    require(isinstance(value, dict), "JSON object required")
    return value


def validate_identity(info: dict) -> None:
    require(set(info) == IDENTITY_KEYS, "build identity schema mismatch")
    require(type(info["schema_version"]) is int and info["schema_version"] == 1, "build identity version")
    require(info["package"] == "stemplayer", "wrong build package")
    require(info["profile"] == "release", "release profile required")
    require(info["source_dirty"] is False, "clean source identity required")
    require(isinstance(info["source_revision"], str) and re.fullmatch(r"[0-9a-f]{40}", info["source_revision"]) is not None, "known source revision required")
    for key in ("cargo_lock_sha256", "toolchain_sha256"):
        require(isinstance(info[key], str) and HEX64.fullmatch(info[key]) is not None, "invalid input hash")
    for key in ("version", "target", "toolchain"):
        require(isinstance(info[key], str) and SAFE_NAME.fullmatch(info[key]) is not None, "invalid build identifier")
    require(info["target"].endswith("-pc-windows-msvc"), "only Windows MSVC evidence is supported")
    require(isinstance(info["rustc"], str) and re.fullmatch(r"rustc [A-Za-z0-9 .()_-]{1,120}", info["rustc"]) is not None, "invalid compiler identity")
    features = info["features"]
    require(isinstance(features, list) and len(features) <= 64, "invalid feature list")
    require(all(isinstance(v, str) and FEATURE.fullmatch(v) for v in features), "invalid feature name")
    require(features == sorted(set(features)), "features must be sorted and unique")
    require("product" in features and "development" not in features, "production features required")


def git(root: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True, timeout=15, check=False)
    require(result.returncode == 0 and len(result.stdout) <= MAX_JSON, "Git identity unavailable")
    return result.stdout.decode("utf-8").strip()


def check_source(root: Path, info: dict) -> None:
    require(Path(git(root, "rev-parse", "--show-toplevel")).resolve() == root.resolve(), "wrong source root")
    require(git(root, "rev-parse", "HEAD") == info["source_revision"], "source revision mismatch")
    require(not git(root, "status", "--porcelain=v1", "--untracked-files=normal"), "source tree is dirty")
    require(digest(read_regular(root / "Cargo.lock", MAX_JSON)) == info["cargo_lock_sha256"], "lock hash mismatch")
    require(digest(read_regular(root / "rust-toolchain.toml", 4096)) == info["toolchain_sha256"], "toolchain hash mismatch")


def package_ref(package: dict) -> str:
    name, version = package["name"], package["version"]
    require(isinstance(name, str) and SAFE_NAME.fullmatch(name) is not None, "invalid package name")
    require(isinstance(version, str) and SAFE_NAME.fullmatch(version) is not None, "invalid package version")
    return f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}"


def graph(metadata: dict, info: dict) -> tuple[dict, dict, str, set[str]]:
    require(metadata.get("version") == 1, "Cargo metadata version mismatch")
    records = metadata.get("packages", [])
    require(0 < len(records) <= MAX_PACKAGES, "package count limit")
    packages = {p["id"]: p for p in records}
    require(len(packages) == len(records), "duplicate package identity")
    resolution = metadata.get("resolve")
    require(isinstance(resolution, dict), "resolved Cargo graph required")
    nodes = {n["id"]: n for n in resolution["nodes"]}
    require(len(nodes) <= MAX_PACKAGES, "node count limit")
    roots = [p["id"] for p in records if p["name"] == "stemplayer" and p["source"] is None]
    require(len(roots) == 1, "unique workspace product required")
    root = roots[0]
    require(packages[root]["version"] == info["version"], "package version mismatch")
    require(sorted(nodes[root]["features"]) == info["features"], "resolved feature mismatch")
    reached, pending = set(), [root]
    while pending:
        key = pending.pop()
        if key in reached:
            continue
        require(key in packages and key in nodes, "incomplete dependency graph")
        reached.add(key)
        for edge in nodes[key]["deps"]:
            kinds = edge.get("dep_kinds")
            require(isinstance(kinds, list) and bool(kinds), "dependency kind missing")
            require(all(k["kind"] in (None, "build", "dev") for k in kinds), "unknown dependency kind")
            if any(k["kind"] in (None, "build") for k in kinds):
                pending.append(edge["pkg"])
    refs = [package_ref(packages[key]) for key in reached]
    require(len(refs) == len(set(refs)), "ambiguous package source")
    return packages, nodes, root, reached


def license_texts(package: dict, root: Path) -> list[tuple[str, str]]:
    base = Path(package["manifest_path"]).parent.resolve()
    if package["source"] is None:
        require(base.is_relative_to(root.resolve()), "external path dependency")
        base = root.resolve()
    candidates = []
    pending = [(base, 0)]
    directories = 0
    while pending:
        directory, depth = pending.pop()
        require(not directory.is_symlink(), "license directory symlink")
        require(not getattr(directory.lstat(), "st_file_attributes", 0) & 0x400, "license directory reparse point")
        directories += 1
        require(directories <= 32 and depth <= 3, "license directory limit")
        for path in directory.iterdir():
            selected = depth > 0 or re.match(r"(?i)^(?:LICEN[CS]ES?|COPYING|NOTICE|UNLICENSE|COPYRIGHT)(?:$|[-_.])", path.name)
            if not selected:
                continue
            require(not path.is_symlink(), "license symlink")
            if path.is_dir():
                pending.append((path, depth + 1))
            else:
                candidates.append(path)
                require(len(candidates) <= 64, "license file count limit")
    declared = package.get("license_file")
    if declared:
        path = Path(package["manifest_path"]).parent / declared
        require(path.resolve().is_relative_to(base), "license file escapes package")
        candidates.append(path)
    result = []
    for path in sorted(set(candidates)):
        require(len(result) < 64, "license file count limit")
        text = read_regular(path, MAX_LICENSE).decode("utf-8")
        require(bool(text.strip()), "empty license text")
        result.append((path.name, text))
    # A declaration is not a substitute for the redistribution texts.
    require(bool(result), f"license text missing: {package['name']}@{package['version']}")
    return result


def inventory(metadata: dict, lock: dict, root: Path, info: dict) -> tuple[dict, bytes]:
    packages, nodes, product, reached = graph(metadata, info)
    locked = {(p["name"], p["version"], p.get("source")): p for p in lock["package"]}
    components, edges, notices = [], [], [
        "THIRD-PARTY SOURCE NOTICES\n",
        "Includes resolved normal and build dependencies; excludes dev-only edges.\n",
        "Declared alternatives are not a legal selection. Review distribution obligations before release.\n",
    ]
    total = sum(len(t.encode()) for t in notices)
    for key in sorted(reached, key=lambda item: package_ref(packages[item])):
        package = packages[key]
        ref = package_ref(package)
        source = package["source"]
        require(source is None or source == REGISTRY, "unapproved dependency source")
        record = locked.get((package["name"], package["version"], source))
        require(record is not None, "dependency missing from lock")
        expression = package.get("license")
        require(isinstance(expression, str) and 0 < len(expression) <= 512, "license declaration missing")
        require(re.fullmatch(r"[A-Za-z0-9 .():+/_-]+", expression) is not None, "invalid license declaration")
        expression = expression.replace("/", " OR ")
        component = {
            "type": "application" if key == product else "library", "bom-ref": ref,
            "name": package["name"], "version": package["version"], "purl": ref,
            "licenses": [{"expression": expression}],
            "properties": [{"name": "stemplayer:source", "value": "workspace" if source is None else "crates.io"}],
        }
        if source is not None:
            checksum = record.get("checksum", "")
            require(HEX64.fullmatch(checksum) is not None, "registry checksum missing")
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        components.append(component)
        children = sorted({package_ref(packages[e["pkg"]]) for e in nodes[key]["deps"]
                           if any(k["kind"] in (None, "build") for k in e["dep_kinds"])})
        edges.append({"ref": ref, "dependsOn": children})
        header = f"\n{'=' * 72}\n{package['name']} {package['version']}\nDeclared: {expression}\n"
        texts = [header]
        for name, text in license_texts(package, root):
            texts.append(f"\n--- {name} ---\n{text}\n")
        total += sum(len(t.encode()) for t in texts)
        require(total <= MAX_NOTICES, "notice size limit exceeded")
        notices.extend(texts)
    root_component = next(c for c in components if c["bom-ref"] == package_ref(packages[product]))
    bom = {
        "bomFormat": "CycloneDX", "specVersion": "1.6", "version": 1,
        "metadata": {"component": root_component, "properties": [
            {"name": "stemplayer:inventory-scope", "value": "resolved normal and build source dependencies; not a linked-code inventory"},
            {"name": "stemplayer:target", "value": info["target"]},
        ]},
        "components": [c for c in components if c is not root_component], "dependencies": edges,
    }
    return bom, "".join(notices).encode("utf-8")


def scan_bytes(data: bytes, private_roots: tuple[str, ...] = ()) -> list[str]:
    """Conservative signatures, not a comprehensive secret detector or PE auditor."""
    found = set()
    # Also inspect both alignments of UTF-16LE strings in Windows binaries.
    views = (data, data[::2], data[1::2])
    for rule, expression in PATTERNS.items():
        if any(re.search(expression, view) for view in views):
            found.add(rule)
    for root in private_roots:
        require(len(root) >= 4, "private-root marker is too short")
        variants = {root, root.replace("\\", "/"), root.replace("/", "\\")}
        for marker in variants:
            if any(marker.encode().lower() in view.lower() for view in views):
                found.add("build-root")
    return sorted(found)


def scan_directory(directory: Path, private_roots: tuple[str, ...] = ()) -> dict:
    require(directory.is_dir() and not directory.is_symlink(), "invalid staging directory")
    findings, checked = [], []
    for path in sorted(directory.rglob("*")):
        require(not path.is_symlink(), "staging symlink")
        require(not getattr(path.lstat(), "st_file_attributes", 0) & 0x400, "staging reparse point")
        if path.is_dir():
            require(path == directory / "bin", "unexpected staging directory")
            continue
        relative = path.relative_to(directory).as_posix()
        require(len(checked) < 16, "staging file count limit")
        allowed = relative in EVIDENCE_NAMES or (path.parent == directory / "bin" and path.name in PAYLOAD_NAMES)
        require(allowed, "unexpected artifact (runtime, fixture, archive, or debug material)")
        data = read_regular(path, MAX_BINARY if path.parent.name == "bin" else MAX_NOTICES)
        for rule in scan_bytes(data, private_roots):
            findings.append({"file": relative, "rule": rule})
        checked.append(relative)
    require(not findings, "artifact scan rejected: " + ", ".join(f"{f['file']}:{f['rule']}" for f in findings))
    return {"schema_version": 1, "status": "pass", "checked_files": checked,
            "rules": sorted([*PATTERNS, "build-root", "payload-allowlist", "size-and-link-limits"]),
            "limitations": "Heuristic content scan; no signature, installer, malware, or clean-system qualification."}


def checksums(directory: Path) -> bytes:
    entries = []
    for path in sorted(directory.rglob("*")):
        if path.is_file() and path.name != "SHA256SUMS":
            require(len(entries) < 16, "staging file count limit")
            data = read_regular(path, MAX_BINARY if path.parent.name == "bin" else MAX_NOTICES)
            entries.append(f"{digest(data)}  {path.relative_to(directory).as_posix()}\n")
    return "".join(entries).encode()


def verify(directory: Path, private_roots: tuple[str, ...] = ()) -> None:
    scan_directory(directory, private_roots)
    recorded = read_regular(directory / "SHA256SUMS", 16 * 1024)
    require(recorded == checksums(directory), "checksum mismatch or unexpected file")
    expected = EVIDENCE_NAMES | {f"bin/{name}" for name in PAYLOAD_NAMES}
    actual = {p.relative_to(directory).as_posix() for p in directory.rglob("*") if not p.is_dir()}
    require(actual == expected, "incomplete evidence package")
    info = read_json(directory / "build-info.json", 16 * 1024)
    validate_identity(info)
    manifest = read_json(directory / "release-manifest.json", 64 * 1024)
    require(manifest.get("schema_version") == 1 and manifest.get("build") == info, "manifest identity mismatch")
    require(manifest.get("kind") == "unsigned-build-evidence" and manifest.get("license_review") == "required-before-distribution", "invalid release claim")
    require(manifest.get("qualification") == "not-qualified" and manifest.get("signing") == "not-performed", "invalid qualification claim")
    require(manifest.get("provenance") == {"kind": "local-build-record", "attested": False}, "invalid provenance claim")
    artifacts = []
    for name in sorted(PAYLOAD_NAMES):
        data = read_regular(directory / "bin" / name, MAX_BINARY)
        require(data.startswith(b"MZ") and canonical(info).rstrip(b"\n") in data, "executable/build identity mismatch")
        artifacts.append({"name": f"bin/{name}", "bytes": len(data), "sha256": digest(data)})
    require(manifest.get("artifacts") == artifacts, "manifest artifact mismatch")
    for field, name in (("sbom_sha256", "sbom.cdx.json"), ("notices_sha256", "THIRD-PARTY-NOTICES.txt")):
        require(manifest.get(field) == digest(read_regular(directory / name, MAX_NOTICES)), "manifest document mismatch")


def collect(root: Path, info_path: Path, metadata_path: Path, binary: Path,
            support: Path, output: Path, private_roots: tuple[str, ...] = ()) -> None:
    root = root.resolve()
    info = read_json(info_path, 16 * 1024)
    validate_identity(info)
    check_source(root, info)
    metadata = read_json(metadata_path)
    require(Path(metadata["workspace_root"]).resolve() == root, "metadata workspace mismatch")
    lock = tomllib.loads(read_regular(root / "Cargo.lock", MAX_JSON).decode())
    bom, notices = inventory(metadata, lock, root, info)
    info_bytes = canonical(info)
    payloads = {}
    for path, name in ((binary, "stemplayer.exe"), (support, "stemplayer-support.exe")):
        require(path.name == name, "unexpected executable name")
        data = read_regular(path, MAX_BINARY)
        require(data.startswith(b"MZ"), "Windows executable required")
        # build.rs embeds identical canonical JSON as RCDATA in both Windows binaries.
        require(info_bytes.rstrip(b"\n") in data, "executable/build identity mismatch")
        payloads[name] = data
    require(not output.exists() and not output.is_symlink(), "output must not exist")
    output.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".stemplayer-evidence-", dir=output.parent))
    try:
        (staging / "bin").mkdir()
        for name, data in payloads.items():
            (staging / "bin" / name).write_bytes(data)
        (staging / "build-info.json").write_bytes(info_bytes)
        (staging / "sbom.cdx.json").write_bytes(canonical(bom))
        (staging / "THIRD-PARTY-NOTICES.txt").write_bytes(notices)
        manifest = {
            "schema_version": 1, "kind": "unsigned-build-evidence", "build": info,
            "qualification": "not-qualified", "signing": "not-performed",
            "provenance": {"kind": "local-build-record", "attested": False},
            "license_review": "required-before-distribution",
            "artifacts": [{"name": f"bin/{name}", "bytes": len(data), "sha256": digest(data)} for name, data in sorted(payloads.items())],
            "sbom_sha256": digest(canonical(bom)), "notices_sha256": digest(notices),
        }
        (staging / "release-manifest.json").write_bytes(canonical(manifest))
        markers = tuple(dict.fromkeys((*private_roots, str(root), str(Path.home()))))
        report = scan_directory(staging, markers)
        (staging / "artifact-scan.json").write_bytes(canonical(report))
        (staging / "SHA256SUMS").write_bytes(checksums(staging))
        verify(staging, markers)
        # Check source again to catch mutation while collecting evidence.
        check_source(root, info)
        require(not output.exists(), "output appeared during collection")
        staging.rename(output)
    finally:
        if staging.exists():
            shutil.rmtree(staging)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    make = sub.add_parser("collect")
    make.add_argument("--root", type=Path, default=Path.cwd())
    for flag in ("build-info", "metadata", "binary", "support-binary", "output"):
        make.add_argument(f"--{flag}", type=Path, required=True)
    check = sub.add_parser("verify")
    check.add_argument("directory", type=Path)
    for command in (make, check):
        command.add_argument("--private-root", action="append", default=[])
    args = parser.parse_args()
    try:
        if args.command == "collect":
            collect(args.root, args.build_info, args.metadata, args.binary, args.support_binary,
                    args.output, tuple(args.private_root))
        else:
            verify(args.directory, tuple(args.private_root))
    except EvidenceError as error:
        print(f"release evidence rejected: {error}", file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, RecursionError, subprocess.SubprocessError):
        # Do not print upstream exceptions: they may contain paths or metadata.
        print("release evidence rejected: invalid or unavailable input", file=sys.stderr)
        return 1
    print("unsigned build evidence verified; no release qualification claimed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
