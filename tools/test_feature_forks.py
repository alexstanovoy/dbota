"""Exercise the feature check with real Cargo graphs and no registry dependencies."""

import contextlib
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import feature_forks


class FeatureForkTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix="dbota-feature-forks-")
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)

    def package(self, name, manifest=""):
        root = self.root if name == "dbota" else self.root / name
        (root / "src").mkdir(parents=True)
        (root / "src/lib.rs").touch()
        (root / "Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2024"\n{manifest}'
        )

    def workspace(self, normal_features="", importer=""):
        self.package("dbota", f"""
[workspace]
members = ["dbota_import", "dbota_import_web", "dbota_particles", "shared"]
default-members = [".", "dbota_import", "dbota_particles"]

[dependencies]
shared = {{ path = "shared" {normal_features} }}

[dev-dependencies]
shared = {{ path = "shared", features = ["extra"] }}
""")
        self.package("dbota_import")
        self.package("dbota_particles")
        self.package("dbota_import_web", importer)
        self.package("shared", '\n[features]\nextra = []\n')
        tools = self.root / "tools"
        tools.mkdir()
        (tools / "test_groups.toml").write_text(
            '[[group]]\ntests = [{package = "dbota_particles"}, {package = "dbota_import"}]\n'
        )
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline"], cwd=self.root, check=True,
            capture_output=True, text=True, timeout=30,
        )

    def check(self):
        output = io.StringIO()
        with mock.patch.object(feature_forks, "ROOT", self.root):
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = feature_forks.main()
        return status, output.getvalue()

    def test_a_dev_feature_fork_fails_without_printing_path_packages(self):
        self.workspace()
        status, output = self.check()
        self.assertEqual(status, 1, output)
        self.assertIn("shared v0.1.0\n", output)
        self.assertIn("feature forks: 1 packages", output)
        self.assertNotIn(str(self.root), output)

    def test_aligned_build_and_test_features_pass(self):
        self.workspace(', features = ["extra"]')
        status, output = self.check()
        self.assertEqual(status, 0, output)
        self.assertIn("feature forks: 0 packages", output)

    def test_the_wasm_importer_refuses_naga_and_winit(self):
        self.package("naga")
        self.package("winit")
        self.workspace(', features = ["extra"]', """
[target.'cfg(target_arch = "wasm32")'.dependencies]
naga = { path = "../naga" }
winit = { path = "../winit" }
""")
        status, output = self.check()
        self.assertEqual(status, 1, output)
        self.assertIn("browser importer pulls in graphics dependencies: naga v0.1.0, winit v0.1.0", output)
        self.assertNotIn(str(self.root), output)
