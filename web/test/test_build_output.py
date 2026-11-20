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
        shutil.copyfile(REPOSITORY / "THIRD-PARTY-NOTICES.md", self.root / "THIRD-PARTY-NOTICES.md")
        (self.root / "assets/fonts").mkdir(parents=True)
        self.font = self.root / "assets/fonts/keep.ttf"
        self.font.write_bytes(b"source font must survive")
        self.tools = self.root / "tools"
        self.tools.mkdir()
        for script in ("stamp.sh", "build.sh"):
            shutil.copyfile(REPOSITORY / "tools" / script, self.tools / script)
        self.server = self.root / "bota"
        self.server.mkdir()
        self.stamps = self.root / "stamp.log"
        for command in ("cargo", "wasm-bindgen", "rustc"):
            tool = self.tools / command
            tool.write_text("#!/bin/sh\nexit 0\n")
            tool.chmod(0o755)

    def run_build(self, script, **settings):
        environment = {
            **os.environ, "PATH": f"{self.tools}:{os.environ['PATH']}",
            "GIT_CEILING_DIRECTORIES": str(self.root.parent),
            "PROFILE": "release", "STAMPS_LOG": str(self.stamps), **settings,
        }
        environment.pop("DBOTA_BUILD", None)
        return subprocess.run(
            ["bash", script], cwd=self.root, capture_output=True, text=True,
            env=environment, timeout=15,
        )

    def build(self, destination):
        return self.run_build("web/build.sh", APIS="webgpu webgl2", OUT=destination, WASM_OPT="0")

    def build_desktop(self):
        return self.run_build("tools/build.sh", DBOTA_GATE_BOTA=str(self.server))

    def commit_wrapper(self, script):
        def git(*arguments):
            return subprocess.check_output(["git", *arguments], cwd=self.root, text=True).strip()

        git("init", "--quiet", "--template=")
        git("add", script)
        git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit",
            "--quiet", "--no-gpg-sign", "-m", "Build fixture")
        return git("rev-parse", "--short=12", "HEAD")

    def record_cargo(self):
        (self.tools / "cargo").write_text(
            '#!/bin/sh\nprintf "%s %s\\n" "${DBOTA_BUILD-unset}" "$*" >> "$STAMPS_LOG"\n'
        )

    def test_browser_build_stamps_only_the_client(self):
        revision = self.commit_wrapper("web/build.sh")
        with (self.root / "web/build.sh").open("a") as script:
            script.write("\n# Dirty checkout\n")
        self.record_cargo()
        result = self.build("out")
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = self.stamps.read_text().splitlines()
        self.assertEqual([line.split()[0] for line in lines], [revision + "+", revision + "+", "unset"])
        self.assertEqual([line.split()[-1] for line in lines], ["dbota", "dbota", "dbota_import_web"])

    def test_desktop_build_stamps_only_the_client(self):
        revision = self.commit_wrapper("tools/build.sh")
        self.record_cargo()
        result = self.build_desktop()
        self.assertEqual(result.returncode, 0, result.stderr)
        client, server = self.stamps.read_text().splitlines()
        self.assertEqual(client.split()[0], revision)
        self.assertEqual(server.split()[0], "unset")
        self.assertIn("-p bota_server", server)

    def test_build_wrappers_use_dev_outside_git(self):
        self.record_cargo()
        for build, stamps in [
            (self.build_desktop, ["dev", "unset"]),
            (lambda: self.build("out"), ["dev", "dev", "unset"]),
        ]:
            with self.subTest(stamps=stamps):
                self.stamps.write_text("")
                result = build()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual([line.split()[0] for line in self.stamps.read_text().splitlines()], stamps)

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

    def test_output_ships_the_notice_the_page_links_to(self):
        result = self.build("out")
        self.assertEqual(result.returncode, 0, result.stderr)
        notice = (self.root / "out/THIRD-PARTY-NOTICES.md").read_text()
        self.assertIn("Font Awesome Free", notice)
        self.assertIn("CC BY 4.0", notice)
        self.assertIn('href="THIRD-PARTY-NOTICES.md"', (self.root / "out/index.html").read_text())

    def test_owned_output_rebuilds_and_copies_fonts(self):
        for _ in range(2):
            result = self.build("out")
            self.assertEqual(result.returncode, 0, result.stderr)
            copied = self.root / "out/assets/fonts/keep.ttf"
            self.assertFalse(copied.is_symlink())
            self.assertEqual(copied.read_bytes(), self.font.read_bytes())


if __name__ == "__main__":
    unittest.main()
