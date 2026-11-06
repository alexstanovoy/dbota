"""Exercise the build's destination handling without compiling the game."""
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

REPOSITORY = pathlib.Path(__file__).resolve().parents[2]


class BuildOutputTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="dbota-build-output-")
        self.addCleanup(self.scratch.cleanup)
        self.root = pathlib.Path(self.scratch.name)
        (self.root / "web").mkdir()
        for source in REPOSITORY.joinpath("web").iterdir():
            if source.is_file():
                shutil.copyfile(source, self.root / "web" / source.name)
        (self.root / "assets/fonts").mkdir(parents=True)
        self.font = self.root / "assets/fonts/keep.ttf"
        self.font.write_bytes(b"source font must survive")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        for command in ("cargo", "wasm-bindgen", "rustc"):
            tool = self.tools / command
            tool.write_text("#!/bin/sh\nexit 0\n")
            tool.chmod(0o755)

    def build(self, destination):
        return subprocess.run(
            ["bash", "web/build.sh"], cwd=self.root, capture_output=True, text=True,
            env={**os.environ, "PATH": f"{self.tools}:{os.environ['PATH']}",
                 "OUT": destination, "PROFILE": "release"}, timeout=15,
        )

    def test_hostile_destinations_preserve_source_fonts(self):
        (self.root / "alias").symlink_to(self.root, target_is_directory=True)
        for destination in (".", "web/..", "alias", "assets/fonts/output"):
            with self.subTest(destination=destination):
                result = self.build(destination)
                self.assertEqual(self.font.read_bytes() if self.font.exists() else None,
                                 b"source font must survive", result.stderr)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unsafe web output", result.stderr)

    def test_unowned_assets_and_symlink_targets_are_preserved(self):
        for linked in (False, True):
            with self.subTest(linked=linked):
                destination = self.root / ("linked-output" if linked else "unowned-output")
                destination.mkdir()
                if linked:
                    (destination / "assets").symlink_to(self.root / "assets", target_is_directory=True)
                    keep = self.font
                else:
                    (destination / "assets").mkdir()
                    keep = destination / "assets/keep"
                    keep.write_bytes(b"unrelated")
                before = keep.read_bytes()
                result = self.build(str(destination))
                self.assertEqual(keep.read_bytes() if keep.exists() else None, before, result.stderr)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("unowned web output", result.stderr)

    def test_owned_output_rejects_hardlinks_to_source_files(self):
        result = self.build("out")
        self.assertEqual(result.returncode, 0, result.stderr)
        os.link(self.font, self.root / "out/dbota.js")
        result = self.build("out")
        self.assertEqual(self.font.read_bytes(), b"source font must survive")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unowned web output", result.stderr)

    def test_owned_output_rebuilds_and_copies_fonts(self):
        for _ in range(2):
            result = self.build("out")
            self.assertEqual(result.returncode, 0, result.stderr)
            copied = self.root / "out/assets/fonts/keep.ttf"
            self.assertFalse(copied.is_symlink())
            self.assertEqual(copied.read_bytes(), self.font.read_bytes())


if __name__ == "__main__":
    unittest.main()
