"""The optimizer is an explicit, checksum-verified build choice."""
import hashlib
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = Path(__file__).resolve().parents[1] / "optimize.py"
SPEC = importlib.util.spec_from_file_location("web_optimizer", SCRIPT)
OPTIMIZER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(OPTIMIZER)
WASM = b"\0asm\1\0\0\0"


class OptimizeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.wasm = self.root / "client.wasm"
        self.wasm.write_bytes(WASM)
        self.tool = self.root / "wasm-opt"
        self.marker = self.root / "invoked"

    def optimizer(self, body):
        self.tool.write_text(f"#!{sys.executable}\nimport pathlib, sys\n" + body)
        self.tool.chmod(0o755)
        return str(self.tool)

    def run_optimizer(self, command):
        environment = dict(os.environ, PATH=str(self.root))
        environment.pop("WASM_OPT", None)
        if command is not None:
            environment["WASM_OPT"] = command
        return subprocess.run(
            [sys.executable, str(SCRIPT), str(self.wasm)],
            env=environment, capture_output=True, text=True, check=False,
        )

    def invoke_verified_fixture(self):
        digest = hashlib.sha256(self.tool.read_bytes()).hexdigest()
        with patch.object(OPTIMIZER, "SHA256", digest):
            OPTIMIZER.optimize(self.wasm, str(self.tool))

    def test_unset_choice_is_refused_instead_of_searching_path(self):
        self.optimizer(f"pathlib.Path({str(self.marker)!r}).write_text('executed')\n")
        result = self.run_optimizer(None)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Set WASM_OPT", result.stderr)
        self.assertFalse(self.marker.exists(), "the installed executable was not invoked")
        self.assertEqual(self.wasm.read_bytes(), WASM)

    def test_disabled_optimizer_does_not_run_an_installed_tool(self):
        self.optimizer(f"pathlib.Path({str(self.marker)!r}).write_text('executed')\n")
        result = self.run_optimizer("0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.marker.exists(), "the installed executable was not invoked")
        self.assertEqual(self.wasm.read_bytes(), WASM)

    def test_wrong_checksum_is_refused_before_executing_the_tool(self):
        self.optimizer(f"pathlib.Path({str(self.marker)!r}).write_text('executed')\nprint('wasm-opt version 125')\n")
        result = self.run_optimizer(str(self.tool))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("checksum mismatch", result.stderr)
        self.assertFalse(self.marker.exists(), "unverified code must not execute")
        self.assertEqual(self.wasm.read_bytes(), WASM)

    def test_verified_optimizer_replaces_the_output_atomically(self):
        self.optimizer(
            "if '--version' in sys.argv: print('wasm-opt version 125 (version_125)')\n"
            "else:\n"
            "    assert '-Oz' in sys.argv\n"
            "    pathlib.Path(sys.argv[sys.argv.index('-o')+1]).write_bytes(b'\\0asm\\1\\0\\0\\0\\0\\x0a\\x09optimized')\n"
        )
        self.invoke_verified_fixture()
        self.assertEqual(self.wasm.read_bytes(), WASM + b"\0\x0a\x09optimized")
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["client.wasm", "wasm-opt"])

    def test_other_version_is_refused_before_replacing_the_output(self):
        self.optimizer("print('wasm-opt version 124')\n")
        with self.assertRaisesRegex(ValueError, "requires Binaryen 125"):
            self.invoke_verified_fixture()
        self.assertEqual(self.wasm.read_bytes(), WASM)

    def test_failed_optimizer_keeps_the_compiler_output_and_its_error(self):
        self.optimizer(
            "if '--version' in sys.argv: print('wasm-opt version 125')\n"
            "else: sys.exit('optimizer fixture failed')\n"
        )
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.invoke_verified_fixture()
        self.assertIn("optimizer fixture failed", error.exception.stderr)
        self.assertEqual(self.wasm.read_bytes(), WASM)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["client.wasm", "wasm-opt"])

    def test_invalid_optimizer_output_is_not_published(self):
        self.optimizer(
            "if '--version' in sys.argv: print('wasm-opt version 125')\n"
            "else: pathlib.Path(sys.argv[sys.argv.index('-o')+1]).write_bytes(b'invalid')\n"
        )
        with self.assertRaisesRegex(ValueError, "invalid WebAssembly"):
            self.invoke_verified_fixture()
        self.assertEqual(self.wasm.read_bytes(), WASM)


if __name__ == "__main__":
    unittest.main()
