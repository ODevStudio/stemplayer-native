import subprocess
import tempfile
import unittest
from pathlib import Path

import check_repository


class RepositoryChecksTests(unittest.TestCase):
    def test_git_inventory_excludes_ignored_but_includes_force_added_markdown(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            (root / ".gitignore").write_text("/artifacts/\n", encoding="utf-8")
            (root / "README.md").write_text("[missing](missing.md)\n", encoding="utf-8")
            (root / "artifacts").mkdir()
            private = root / "artifacts" / "local notes.md"
            private.write_text("[private](missing.md)\n", encoding="utf-8")

            self.assertEqual(check_repository.find_issues(root), ["README.md: broken link: missing.md"])

            subprocess.run(["git", "-C", str(root), "add", "--force", "--", str(private)], check=True)

            self.assertCountEqual(
                check_repository.find_issues(root),
                ["README.md: broken link: missing.md", "artifacts/local notes.md: broken link: missing.md"],
            )

    def test_valid_markdown_links_and_authority_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "AGENTS.md").write_text("STATUS.md is the only authority for current phase status.\n", encoding="utf-8")
            (root / "STATUS.md").write_text("# Status\n", encoding="utf-8")
            (root / "docs" / "README.md").write_text("[Status](../STATUS.md)\n", encoding="utf-8")

            self.assertEqual(check_repository.find_issues(root), [])

    def test_markdown_security_and_link_failures_are_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            (root / "AGENTS.md").write_text("STATUS.md is the only authority for current phase status.\n", encoding="utf-8")
            (root / "STATUS.md").write_text("# Status\n", encoding="utf-8")
            bad = "[missing](missing.md) docs/archive/old.md Ãƒ C:\\Users\\name\\secret.wav api_key=abc\n"
            (root / "docs" / "a.md").write_text(bad, encoding="utf-8")
            (root / "docs" / "b.md").write_text("STATUS.md is the only authority for current phase status.\n", encoding="utf-8")

            issues = "\n".join(check_repository.find_issues(root))
            self.assertIn("broken link", issues)
            self.assertIn("mojibake", issues)
            self.assertIn("deleted docs/archive", issues)
            self.assertIn("private path", issues)
            self.assertIn("credential-like", issues)
            self.assertIn("multiple current-status", issues)

    def test_workflow_actions_must_be_immutable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            workflow = root / ".github" / "workflows"
            workflow.mkdir(parents=True)
            (workflow / "ci.yml").write_text(
                """name: test
permissions:
concurrency:
timeout-minutes: 5
fail-fast: false
- uses: actions/checkout@v4
cargo clippy --workspace
cargo build --locked --release -p stemplayer
""",
                encoding="utf-8",
            )

            issues = "\n".join(check_repository.find_issues(root))
            self.assertIn("not pinned to a full SHA", issues)
            self.assertIn("release tag comment", issues)


if __name__ == "__main__":
    unittest.main()
