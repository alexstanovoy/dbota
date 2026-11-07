"""Exercise the real test orchestrator with small executable Cargo/libtest stand-ins."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]

CARGO = r'''#!/usr/bin/env python3
import json,os,sys
from pathlib import Path
root=Path(os.environ['FAKE_ROOT'])
args=sys.argv[1:]
active=root/'cargo.active'
try:
    active.touch(exist_ok=False)
except FileExistsError:
    sys.exit('concurrent Cargo commands')
try:
    if args[0]=='metadata':
        print((root/'metadata.json').read_text())
    elif args[0]=='build':
        assert Path.cwd()==root/'producer', 'producer config boundary'
        target=Path(args[args.index('--target-dir')+1])/'release/bota_server'
        target.parent.mkdir(parents=True,exist_ok=True)
        target.write_text('#!/bin/sh\nexit 0\n');target.chmod(0o755)
    elif '--no-run' in args:
        names=['dbota'] if '-p' in args and args[args.index('-p')+1]=='dbota' else ['dbota','importer']
        for name in names:
            print(json.dumps({'reason':'compiler-artifact','profile':{'test':True},
              'package_id':name,'target':{'name':name},'executable':str(root/'bin'/name)}))
    elif '--doc' in args:
        assert 'dbota' not in args, 'bin-only packages have no doctests'
        assert (root/'menu.done').exists() and (root/'rest.done').exists(), 'doctests ran before lanes drained'
        if os.environ.get('FAKE_FULL_MATCH'):
            assert (root/'full.done').exists(), 'doctests ran before the exclusive full match'
        (root/'docs.done').touch()
        print('test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')
    else:
        assert os.environ['RUST_TEST_THREADS']=='1'
        assert '--test-threads=1' in args
        (root/'serial.json').write_text(json.dumps(args))
        print('serial Cargo test preserved')
finally:
    active.unlink()
'''

BINARY = r'''#!/usr/bin/env python3
import os,sys,time
from pathlib import Path
root=Path(os.environ['FAKE_ROOT']);args=sys.argv[1:]
menu='main_menu::tests::network'
full='main_menu::tests::two_players_register_find_accept_play_and_see_their_result'
names=[menu,'e2e::two_clients','tip_content_ignored'] if Path(sys.argv[0]).name=='dbota' else ['importer::pure']
if Path(sys.argv[0]).name=='dbota' and os.environ.get('FAKE_FULL_MATCH'):
    names.append(full)
if full in args and '--skip' not in args:
    names=[name for name in names if name==full]
if args[:1]==[menu.split('network')[0]]:
    names=[name for name in names if name.startswith('main_menu::tests::')]
if '--skip' in args:
    names=[name for name in names if args[args.index('--skip')+1] not in name]
if '--list' in args:
    if os.environ.get('FAKE_BAD_PARTITION') and '--skip' in args:
        names=[]
    for name in names:print(name+': test')
    sys.exit(0)
assert os.environ['RUST_TEST_THREADS']=='1'
assert '--test-threads=1' in args
assert Path(os.environ['BOTA_SERVER_BIN']).is_file(), 'server must be prepared before concurrent tests'
library_path = os.environ['LD_LIBRARY_PATH']
assert all(library_path.split(os.pathsep)), 'an empty library path searches the working directory'
(root/'library-path.txt').write_text(library_path)
with (root/'selected.jsonl').open('a') as selected:
    for name in names:
        selected.write(__import__('json').dumps(name)+'\n')
if names==[full]:
    assert not (root/'menu.active').exists() and not (root/'rest.active').exists(), 'full match overlapped another lane'
    assert (root/'menu.done').exists() and (root/'rest.done').exists(), 'full match ran before both lanes drained'
    assert (root/'importer.done').exists(), 'full match ran before other binaries drained'
    (root/'full.started').touch()
    (root/'full.done').touch()
    failed=bool(os.environ.get('FAKE_FAIL_FULL'))
    print(f'test result: {"FAILED" if failed else "ok"}. {int(not failed)} passed; {int(failed)} failed; 0 ignored; 0 measured; 0 filtered out')
    sys.exit(7 if failed else 0)
if names==['importer::pure']:
    assert (root/'rest.done').exists(), 'other binaries overlapped the asset-client lane'
    (root/'importer.done').touch()
    print('test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out')
    sys.exit(0)
group='menu' if names and all(name.startswith('main_menu::tests::') for name in names) else 'rest'
(root/(group+'.active')).touch()
(root/(group+'.started')).touch()
(root/(group+'.pid')).write_text(str(os.getpid()))
other=root/('rest.started' if group=='menu' else 'menu.started')
deadline=time.monotonic()+5
while not other.exists() and time.monotonic()<deadline:time.sleep(0.01)
assert other.exists(), 'the independent lanes did not overlap'
if os.environ.get('FAKE_HANG_LANES'):time.sleep(30)
if group=='menu' and os.environ.get('FAKE_HANG_MENU'):time.sleep(30)
if group=='menu':time.sleep(0.2)
if full in names:
    assert not (root/'menu.active').exists() and not (root/'rest.active').exists(), 'full match overlapped another lane'
(root/(group+'.done')).touch()
(root/(group+'.active')).unlink()
failed=group=='rest' and os.environ.get('FAKE_FAIL_REST')
ignored=sum(name.endswith('_ignored') for name in names)
print(f'test result: {"FAILED" if failed else "ok"}. {len(names)-ignored-int(bool(failed))} passed; {int(bool(failed))} failed; {ignored} ignored; 0 measured; 0 filtered out')
sys.exit(7 if failed else 0)
'''


class TestRunnerTests(unittest.TestCase):
    def setUp(self):
        (ROOT / "temp").mkdir(exist_ok=True)
        self.scratch = tempfile.TemporaryDirectory(prefix="runner-test-", dir=ROOT / "temp")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        (self.root / "tools").mkdir()
        (self.root / "bin").mkdir()
        (self.root / "crates/importer").mkdir(parents=True)
        (self.root / "producer").mkdir()
        shutil.copyfile(ROOT / "tools/test_runner.py", self.root / "tools/test_runner.py")
        for name, source in [("cargo", CARGO), ("dbota", BINARY), ("importer", BINARY)]:
            path = self.root / "bin" / name
            path.write_text(source)
            path.chmod(0o755)
        packages = [
            {"id": "dbota", "name": "dbota", "manifest_path": str(self.root / "Cargo.toml"), "targets": [{"kind": ["bin"], "doctest": False}]},
            {"id": "importer", "name": "importer", "manifest_path": str(self.root / "crates/importer/Cargo.toml"), "targets": [{"kind": ["lib"], "doctest": True}]},
            {"id": "bota", "name": "bota_client_core", "manifest_path": str(self.root / "producer/crates/bota_client_core/Cargo.toml"), "targets": []},
        ]
        (self.root / "metadata.json").write_text(json.dumps({"packages": packages, "workspace_members": ["dbota", "importer"], "workspace_default_members": ["dbota", "importer"]}))
        self.environment = dict(os.environ, CARGO=str(self.root / "bin/cargo"), FAKE_ROOT=str(self.root))
        self.environment.pop("BOTA_SERVER_BIN", None)

    def run_runner(self, arguments=(), **environment):
        return subprocess.run([sys.executable, str(self.root / "tools/test_runner.py"), *arguments],
                              env=dict(self.environment, **environment), text=True, capture_output=True, timeout=20)

    def test_partition_runs_independent_lanes_and_drains_before_other_binaries_and_docs(self):
        result = self.run_runner()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("3 = 1 menu-core + 2 remaining", result.stdout)
        self.assertTrue((self.root / "importer.done").exists())
        self.assertTrue((self.root / "docs.done").exists())
        self.assertIn("1 ignored", result.stdout)

    def test_failure_cancels_sibling_and_later_binaries_and_keeps_failure_output(self):
        result = self.run_runner(FAKE_FAIL_REST="1", FAKE_HANG_MENU="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "rest.done").exists())
        self.assertIn("FAILED", result.stdout)
        self.assertFalse((self.root / "importer.done").exists())
        self.assertFalse((self.root / "docs.done").exists())
        with self.assertRaises(ProcessLookupError):
            os.kill(int((self.root / "menu.pid").read_text()), 0)
        self.assertFalse((self.root / "cargo.active").exists())

    def test_full_match_runs_exclusively_after_both_lanes_with_exact_coverage(self):
        result = self.run_runner(FAKE_FULL_MATCH="1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        selected = [json.loads(line) for line in (self.root / "selected.jsonl").read_text().splitlines()]
        self.assertEqual(len(selected), len(set(selected)), "a test was selected twice")
        self.assertEqual(set(selected), {"main_menu::tests::network", "main_menu::tests::two_players_register_find_accept_play_and_see_their_result", "e2e::two_clients", "tip_content_ignored", "importer::pure"})
        self.assertTrue((self.root / "full.done").exists())
        self.assertTrue((self.root / "docs.done").exists())

    def test_default_failure_skips_full_match_and_docs(self):
        result = self.run_runner(FAKE_FULL_MATCH="1", FAKE_FAIL_REST="1", FAKE_HANG_MENU="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "full.started").exists())
        self.assertFalse((self.root / "docs.done").exists())

    def test_no_fail_fast_runs_full_match_and_docs_after_a_lane_failure(self):
        result = self.run_runner(["--no-fail-fast"], FAKE_FULL_MATCH="1", FAKE_FAIL_REST="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "full.done").exists())
        self.assertTrue((self.root / "docs.done").exists())

    def test_full_match_failure_stops_before_docs_by_default(self):
        result = self.run_runner(FAKE_FULL_MATCH="1", FAKE_FAIL_FULL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "full.done").exists())
        self.assertFalse((self.root / "docs.done").exists())

    def test_no_fail_fast_keeps_docs_after_a_full_match_failure(self):
        result = self.run_runner(["--no-fail-fast"], FAKE_FULL_MATCH="1", FAKE_FAIL_FULL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "full.done").exists())
        self.assertTrue((self.root / "docs.done").exists())

    def test_no_fail_fast_waits_for_other_lane_and_runs_remaining_binaries_and_docs(self):
        result = self.run_runner(["--no-fail-fast"], FAKE_FAIL_REST="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.root / "menu.done").exists())
        self.assertTrue((self.root / "importer.done").exists())
        self.assertTrue((self.root / "docs.done").exists())

    def assert_signal_drains(self, number):
        process = subprocess.Popen([sys.executable, str(self.root / "tools/test_runner.py")],
                                   env=dict(self.environment, FAKE_HANG_LANES="1"),
                                   text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        owned = []
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                paths = [self.root / f"{lane}.pid" for lane in ("menu", "rest")]
                if all(path.exists() for path in paths):
                    owned = [int(path.read_text()) for path in paths]
                    break
                time.sleep(0.01)
            self.assertEqual(len(owned), 2, "both lanes must start before interruption")
            process.send_signal(number)
            output, _ = process.communicate(timeout=15)
            self.assertNotEqual(process.returncode, 0, output)
            for pid in owned:
                with self.assertRaises(ProcessLookupError):
                    os.kill(pid, 0)
            self.assertFalse((self.root / "importer.done").exists())
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
            for pid in owned:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    def test_interrupt_terminates_and_reaps_both_lanes_without_starting_more_tests(self):
        self.assert_signal_drains(signal.SIGINT)

    def test_sigterm_terminates_and_reaps_both_lanes_without_starting_more_tests(self):
        self.assert_signal_drains(signal.SIGTERM)

    def test_incomplete_partition_fails_before_any_test_can_be_lost(self):
        result = self.run_runner(FAKE_BAD_PARTITION="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("do not partition", result.stderr)
        self.assertFalse((self.root / "menu.started").exists())

    def test_bin_only_package_selection_does_not_invoke_library_doctests(self):
        result = self.run_runner(["-p", "dbota"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / "docs.done").exists())

    def test_filtered_and_ignored_runs_keep_serial_cargo_selection(self):
        for selection in ("e2e::two_clients", "--ignored"):
            with self.subTest(selection=selection):
                result = self.run_runner(["-p", "dbota", "--", selection])
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(selection, json.loads((self.root / "serial.json").read_text()))
                self.assertFalse((self.root / "menu.started").exists())

    def test_explicit_target_selection_keeps_cargos_own_execution_semantics(self):
        result = self.run_runner(["--tests"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("--tests", json.loads((self.root / "serial.json").read_text()))
        self.assertFalse((self.root / "menu.started").exists())

    def test_filtered_importer_selection_does_not_build_an_unrelated_game_server(self):
        result = self.run_runner(["-p", "importer", "--", "decoder"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((self.root / "target/bota/release/bota_server").exists())
        self.assertIn("decoder", json.loads((self.root / "serial.json").read_text()))

    def test_dynamic_library_paths_never_include_the_working_directory(self):
        result = self.run_runner(LD_LIBRARY_PATH="/existing/lib:")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        paths = (self.root / "library-path.txt").read_text().split(os.pathsep)
        self.assertIn("/existing/lib", paths)
        self.assertNotIn("", paths)


if __name__ == "__main__":
    unittest.main()
