from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import release_evidence as evidence


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "repo"
        self.root.mkdir()
        self.cargo_home = self.base / "cargo-home"
        self.registry = self.cargo_home / "registry" / "src" / "fixture-index"
        self.registry.mkdir(parents=True)
        environment = mock.patch.dict(os.environ, {"CARGO_HOME": str(self.cargo_home)})
        environment.start()
        self.addCleanup(environment.stop)
        (self.root / ".gitignore").write_text("target/\nartifacts/\n", encoding="utf-8")
        (self.root / "LICENSE-MIT").write_text("Synthetic test license text.\n", encoding="utf-8")
        (self.root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.97.0"\n', encoding="utf-8"
        )
        (self.root / "Cargo.toml").write_text('[workspace]\n', encoding="utf-8")
        packages, nodes, entries = [], [], []
        for name, source in [
            ("stemplayer", None),
            ("normal", evidence.REGISTRY),
            ("builder", evidence.REGISTRY),
            ("dev-only", evidence.REGISTRY),
        ]:
            package_dir = self.root if source is None else self.registry / f"{name}-0.1.0"
            package_dir.mkdir(exist_ok=True)
            (package_dir / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
            if source:
                (package_dir / "LICENSE").write_text(
                    f"Synthetic {name} redistribution text.\n", encoding="utf-8"
                )
            packages.append(
                {
                    "id": name,
                    "name": name,
                    "version": "0.1.0",
                    "source": source,
                    "manifest_path": str(package_dir / "Cargo.toml"),
                    "license": "MIT",
                    "license_file": None,
                }
            )
            nodes.append(
                {
                    "id": name,
                    "deps": [],
                    "features": ["default", "product"] if source is None else [],
                }
            )
            entry = f'[[package]]\nname = "{name}"\nversion = "0.1.0"\n'
            if source:
                entry += f'source = "{source}"\nchecksum = "{"a" * 64}"\n'
            entries.append(entry)
        nodes[0]["deps"] = [
            {"pkg": name, "dep_kinds": [{"kind": kind, "target": None}]}
            for name, kind in [("normal", None), ("builder", "build"), ("dev-only", "dev")]
        ]
        (self.root / "Cargo.lock").write_text(
            "version = 4\n\n" + "\n".join(entries), encoding="utf-8"
        )
        self.metadata = {
            "version": 1,
            "workspace_root": str(self.root),
            "packages": packages,
            "resolve": {"nodes": nodes, "root": "stemplayer"},
        }
        self.git("init", "-q")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "fixture")
        self.info = {
            "schema_version": 1,
            "package": "stemplayer",
            "version": "0.1.0",
            "source_revision": self.git("rev-parse", "HEAD"),
            "source_dirty": False,
            "rustc": "rustc 1.97.0 (synthetic fixture)",
            "toolchain": "1.97.0",
            "toolchain_sha256": evidence.digest(
                (self.root / "rust-toolchain.toml").read_bytes()
            ),
            "cargo_lock_sha256": evidence.digest((self.root / "Cargo.lock").read_bytes()),
            "target": "x86_64-pc-windows-msvc",
            "profile": "release",
            "features": ["default", "product"],
        }
        self.info_path = self.base / "build-info.json"
        self.meta_path = self.base / "metadata.json"
        self.info_path.write_bytes(evidence.canonical(self.info))
        self.meta_path.write_bytes(evidence.canonical(self.metadata))
        (self.root / "target").mkdir()
        self.binary = self.root / "target" / "stemplayer.exe"
        self.support = self.root / "target" / "stemplayer-support.exe"
        for path in (self.binary, self.support):
            path.write_bytes(b"MZ\x00synthetic\x00" + evidence.canonical(self.info))
        self.output = self.root / "artifacts" / "one"

    def git(self, *args):
        return subprocess.check_output(
            ["git", "-C", str(self.root), *args], stderr=subprocess.DEVNULL
        ).decode().strip()

    def collect(self, output=None):
        evidence.collect(
            self.root,
            self.info_path,
            self.meta_path,
            self.binary,
            self.support,
            output or self.output,
        )

    def save_metadata(self):
        self.meta_path.write_bytes(evidence.canonical(self.metadata))

    def test_collect_verify_and_repeat_are_deterministic(self):
        self.collect()
        evidence.verify(self.output)
        second = self.root / "artifacts" / "two"
        self.collect(second)
        self.assertEqual(
            (self.output / "SHA256SUMS").read_bytes(),
            (second / "SHA256SUMS").read_bytes(),
        )
        bom = evidence.read_json(self.output / "sbom.cdx.json")
        self.assertEqual({component["name"] for component in bom["components"]}, {"normal", "builder"})
        self.assertEqual(bom["specVersion"], "1.6")
        manifest = evidence.read_json(self.output / "release-manifest.json")
        self.assertFalse(manifest["provenance"]["attested"])
        self.assertEqual(manifest["qualification"], "not-qualified")
        scan = evidence.read_json(self.output / "artifact-scan.json")
        self.assertNotIn("artifact-scan.json", scan["checked_files"])
        self.assertNotIn("SHA256SUMS", scan["checked_files"])
        self.assertIn(
            "Synthetic normal redistribution text",
            (self.output / "THIRD-PARTY-NOTICES.txt").read_text(),
        )

    def test_dirty_source_is_rejected(self):
        (self.root / "untracked.txt").write_text("changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "dirty"):
            self.collect()
        self.assertFalse(self.output.exists())

    def test_wrong_lock_or_revision_is_rejected(self):
        for field, value in (("cargo_lock_sha256", "b" * 64), ("source_revision", "b" * 40)):
            with self.subTest(field=field):
                self.info_path.write_bytes(evidence.canonical({**self.info, field: value}))
                with self.assertRaises(evidence.EvidenceError):
                    self.collect()

    def test_stale_executable_is_rejected(self):
        self.binary.write_bytes(b"MZ old build")
        with self.assertRaisesRegex(evidence.EvidenceError, "identity mismatch"):
            self.collect()

    def test_existing_output_is_not_replaced(self):
        self.output.mkdir(parents=True)
        sentinel = self.output / "keep"
        sentinel.write_text("keep")
        with self.assertRaises(evidence.EvidenceError):
            self.collect()
        self.assertEqual(sentinel.read_text(), "keep")

    def test_license_text_is_required(self):
        (self.registry / "normal-0.1.0" / "LICENSE").unlink()
        with self.assertRaisesRegex(evidence.EvidenceError, "license text missing: normal"):
            self.collect()

    def test_registry_manifest_path_cannot_exfiltrate_arbitrary_files(self):
        outside = self.base / "outside" / "normal-0.1.0"
        outside.mkdir(parents=True)
        (outside / "Cargo.toml").write_text("[package]\n")
        (outside / "LICENSE").write_text("should never be copied")
        self.metadata["packages"][1]["manifest_path"] = str(outside / "Cargo.toml")
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "registry source path rejected"):
            self.collect()

    def test_registry_package_leaf_must_match_name_and_version(self):
        wrong = self.registry / "other-9.9.9"
        wrong.mkdir()
        (wrong / "Cargo.toml").write_text("[package]\n")
        (wrong / "LICENSE").write_text("wrong package")
        self.metadata["packages"][1]["manifest_path"] = str(wrong / "Cargo.toml")
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "registry source path rejected"):
            self.collect()

    def test_license_directories_include_full_alternative_texts(self):
        directory = self.registry / "normal-0.1.0" / "LICENSES"
        directory.mkdir()
        (directory / "MIT.txt").write_text("Full synthetic alternative text")
        self.collect()
        self.assertIn(
            "Full synthetic alternative text",
            (self.output / "THIRD-PARTY-NOTICES.txt").read_text(),
        )

    def test_missing_registry_checksum_is_rejected(self):
        import tomllib

        lock = tomllib.loads((self.root / "Cargo.lock").read_text())
        del lock["package"][1]["checksum"]
        with self.assertRaisesRegex(evidence.EvidenceError, "checksum missing"):
            evidence.inventory(self.metadata, lock, self.root, self.info)

    def test_license_path_escape_is_rejected(self):
        self.metadata["packages"][1]["license_file"] = "../../outside"
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "escapes"):
            self.collect()

    def test_registry_and_external_path_dependencies_are_rejected(self):
        self.metadata["packages"][1]["source"] = "git+https://example.invalid/repo"
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "unapproved"):
            self.collect()

    def test_feature_resolution_must_match_compilation(self):
        self.metadata["resolve"]["nodes"][0]["features"].append("development")
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "feature mismatch"):
            self.collect()

    def test_unresolved_graph_is_rejected(self):
        self.metadata["resolve"] = None
        self.save_metadata()
        with self.assertRaisesRegex(evidence.EvidenceError, "resolved"):
            self.collect()

    def test_unapproved_build_identities_are_rejected(self):
        for patch in (
            {"source_dirty": None},
            {"profile": "debug"},
            {"source_revision": None},
            {"target": "x86_64-unknown-linux-gnu"},
            {"schema_version": True},
            {"features": ["development", "product"]},
            {"private_path": "redacted"},
        ):
            with self.subTest(patch=patch), self.assertRaises(evidence.EvidenceError):
                evidence.validate_identity({**self.info, **patch})

    def test_content_signatures_cover_utf16_and_do_not_disclose_matches(self):
        samples = {
            "private-key": "-----BEGIN " + "PRIVATE KEY-----",
            "github-token": "gh" + "p_" + "a" * 36,
            "aws-access-key": "AK" + "IA" + "A" * 16,
            "private-path": "/" + "home/fixture/file",
            "debug-material": "RS" + "DS",
        }
        for rule, text in samples.items():
            for data in (text.encode(), text.encode("utf-16le"), b"!" + text.encode("utf-16le")):
                with self.subTest(rule=rule):
                    self.assertIn(rule, evidence.scan_bytes(data))
                    self.assertNotIn(text, str(evidence.scan_bytes(data)))
        self.assertEqual(evidence.scan_bytes(b"ordinary native bytes"), [])

    def test_explicit_build_roots_are_scanned(self):
        self.assertIn(
            "build-root",
            evidence.scan_bytes(b"prefix Z:\\agent\\work suffix", ("Z:/agent/work",)),
        )

    def test_tamper_is_rejected(self):
        self.collect()
        with (self.output / "bin" / "stemplayer.exe").open("ab") as handle:
            handle.write(b"changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "checksum"):
            evidence.verify(self.output)

    def test_rechecksummed_inconsistent_manifest_is_rejected(self):
        self.collect()
        path = self.output / "release-manifest.json"
        manifest = evidence.read_json(path)
        manifest["artifacts"][0]["sha256"] = "b" * 64
        path.write_bytes(evidence.canonical(manifest))
        (self.output / "SHA256SUMS").write_bytes(evidence.checksums(self.output))
        with self.assertRaisesRegex(evidence.EvidenceError, "manifest artifact"):
            evidence.verify(self.output)

    def test_rechecksummed_artifact_scan_tamper_is_rejected(self):
        self.collect()
        path = self.output / "artifact-scan.json"
        report = evidence.read_json(path)
        report["status"] = "tampered"
        path.write_bytes(evidence.canonical(report))
        (self.output / "SHA256SUMS").write_bytes(evidence.checksums(self.output))
        with self.assertRaisesRegex(evidence.EvidenceError, "artifact scan report mismatch"):
            evidence.verify(self.output)

    def test_unexpected_runtimes_media_archives_and_debug_files_are_rejected(self):
        self.collect()
        for name in (
            "python.exe",
            "node.exe",
            "runtime.dll",
            "private.wav",
            "payload.zip",
            "stemplayer.pdb",
        ):
            path = self.output / "bin" / name
            path.write_bytes(b"unexpected")
            with self.subTest(name=name), self.assertRaises(evidence.EvidenceError):
                evidence.verify(self.output)
            path.unlink()

    def test_staging_enumeration_is_bounded_before_sorting_or_recursion(self):
        directory = self.base / "many"
        directory.mkdir()
        for index in range(evidence.MAX_STAGE_ROOT_ENTRIES + 1):
            (directory / f"extra-{index}").write_bytes(b"x")
        with self.assertRaisesRegex(evidence.EvidenceError, "entry count limit"):
            evidence.staged_files(directory)

    def test_nested_staging_directories_are_rejected_without_recursion(self):
        directory = self.base / "nested"
        directory.mkdir()
        (directory / "unexpected").mkdir()
        (directory / "unexpected" / "deep").mkdir()
        with self.assertRaisesRegex(evidence.EvidenceError, "unexpected artifact"):
            evidence.staged_files(directory)

    def test_symlink_and_oversized_reads_are_rejected(self):
        path = self.base / "small"
        path.write_bytes(b"0123456789")
        with self.assertRaisesRegex(evidence.EvidenceError, "size limit"):
            evidence.read_regular(path, 4)
        link = self.base / "link"
        try:
            link.symlink_to(path)
        except OSError:
            self.skipTest("symlink creation is not permitted")
        with self.assertRaisesRegex(evidence.EvidenceError, "non-regular"):
            evidence.read_regular(link, 100)

    def test_duplicate_json_keys_are_rejected(self):
        self.info_path.write_text('{"schema_version":1,"schema_version":2}')
        with self.assertRaisesRegex(evidence.EvidenceError, "duplicate JSON"):
            evidence.read_json(self.info_path)

    def test_scan_failure_leaves_no_output_or_staging_directory(self):
        with self.binary.open("ab") as handle:
            handle.write(("gh" + "p_" + "z" * 36).encode())
        with self.assertRaisesRegex(evidence.EvidenceError, "github-token"):
            self.collect()
        self.assertFalse(self.output.exists())
        self.assertEqual(list(self.output.parent.glob(".stemplayer-evidence-*")), [])


if __name__ == "__main__":
    unittest.main()
