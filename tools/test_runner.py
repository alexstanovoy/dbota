"""Run the workspace tests with one compiler and at most one asset-client test at a time."""
import concurrent.futures
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parent.parent
MENU = "main_menu::tests::"
FULL_MATCH = MENU + "two_players_register_find_accept_play_and_see_their_result"
CARGO = os.environ.get("CARGO", "cargo")
PROFILE = ["--locked", "--profile", "test-release"]
LIVE = set()
MUTEX = threading.Lock()
CANCELLED = threading.Event()
MAIN_SPAWN = False
PENDING_SIGNAL = None


class Cancelled(Exception):
    pass


class Terminated(BaseException):
    def __init__(self, number):
        self.number = number


def terminate(number, _frame):
    global PENDING_SIGNAL
    signal.signal(number, signal.SIG_IGN)
    PENDING_SIGNAL = number
    if not MAIN_SPAWN:
        raise Terminated(number)


def start(command, **kwargs):
    global MAIN_SPAWN
    main_thread = threading.current_thread() is threading.main_thread()
    with MUTEX:
        if CANCELLED.is_set() or PENDING_SIGNAL is not None:
            raise Cancelled()
        # A main-thread signal must not interrupt Popen before its child is registered.
        if main_thread:
            MAIN_SPAWN = True
        try:
            process = subprocess.Popen(command, start_new_session=True, **kwargs)
            LIVE.add(process)
        finally:
            if main_thread:
                MAIN_SPAWN = False
    if main_thread and PENDING_SIGNAL is not None:
        raise Terminated(PENDING_SIGNAL)
    return process


def finish(process):
    try:
        return process.wait(timeout=3600)
    except subprocess.TimeoutExpired:
        stop([process])
        return 124
    finally:
        if process.poll() is not None:
            with MUTEX:
                LIVE.discard(process)


def stop(processes):
    for process in processes:
        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    deadline = time.monotonic() + 10
    for process in processes:
        try:
            process.wait(timeout=max(0, deadline - time.monotonic()))
        except subprocess.TimeoutExpired:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def cancel():
    with MUTEX:
        CANCELLED.set()
        processes = list(LIVE)
    stop(processes)


def capture(command, **kwargs):
    process = start(command, stdout=subprocess.PIPE, text=True, **kwargs)
    try:
        output, _ = process.communicate(timeout=3600)
    except subprocess.TimeoutExpired:
        stop([process])
        raise RuntimeError("test preparation timed out") from None
    finally:
        if process.poll() is not None:
            with MUTEX:
                LIVE.discard(process)
    if process.returncode:
        raise RuntimeError(f"test preparation exited {process.returncode}: {command}")
    return output


def options(arguments, metadata):
    """Unknown/filtered Cargo invocations keep Cargo's own selection and doctest semantics."""
    before, separator, after = "\0".join(arguments).partition("\0--\0")
    if arguments[:1] == ["--"]:
        before, separator, after = "", "--", "\0".join(arguments[1:])
    cargo = before.split("\0") if before else []
    tests = after.split("\0") if after else []
    if cargo[-1:] == ["--"]:
        cargo.pop()
    if any(arg.startswith("--test-threads") for arg in tests):
        raise RuntimeError("tools/test.sh manages --test-threads for the client memory limit")
    names = {p["name"]: p["id"] for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    selected, excluded, features = [], [], []
    workspace = False
    parallel = not tests
    index = 0
    while index < len(cargo):
        arg = cargo[index]
        key, equal, value = arg.partition("=")
        if key in ("-p", "--package", "--exclude", "--features", "-F"):
            if not equal:
                index += 1
                if index == len(cargo):
                    return cargo, tests, None, [], True
                value = cargo[index]
            if key in ("-p", "--package"):
                selected.append(value)
            elif key == "--exclude":
                excluded.append(value)
            else:
                features.extend([key, value])
                # Let Cargo retain package-qualified feature/doctest selection semantics.
                parallel = False
        elif arg == "--workspace":
            workspace = True
        elif arg in ("--all-features", "--no-default-features", "--offline", "--frozen"):
            features.append(arg)
        elif arg in ("--all-targets", "--lib", "--bins", "--tests", "--examples", "--benches"):
            parallel = False
        elif arg not in ("--locked", "--quiet", "-q", "--verbose", "-v", "--no-fail-fast"):
            parallel = False
        index += 1
    known_selection = all(name in names for name in selected + excluded)
    if not known_selection:
        parallel = False
    members = set(metadata["workspace_members"] if workspace else metadata["workspace_default_members"])
    if selected:
        members = {names[name] for name in selected if name in names}
    members.difference_update(names[name] for name in excluded if name in names)
    libraries = [p["name"] for p in metadata["packages"] if p["id"] in members and any(
        "lib" in target["kind"] and target.get("doctest", True) for target in p["targets"])]
    needs_server = names.get("dbota") in members if known_selection else True
    return cargo, tests, libraries if parallel else None, features, needs_server


def build(arguments, metadata):
    process = start([CARGO, "test", *PROFILE, *arguments, "--no-run", "--message-format=json"],
                    cwd=ROOT, stdout=subprocess.PIPE, text=True)
    artifacts = {}
    for line in process.stdout:
        message = json.loads(line)
        if message.get("reason") == "compiler-message":
            print(message["message"].get("rendered", ""), end="", flush=True)
        if message.get("reason") == "compiler-artifact" and message["profile"]["test"] and message.get("executable"):
            package = next(p for p in metadata["packages"] if p["id"] == message["package_id"])
            artifacts[message["executable"]] = (message["target"]["name"], Path(package["manifest_path"]).parent)
    if finish(process):
        raise RuntimeError("test binaries did not build")
    return artifacts


def server_environment(metadata, required):
    environment = dict(os.environ, RUST_TEST_THREADS="1")
    if required and "BOTA_SERVER_BIN" not in environment:
        manifest = next(p["manifest_path"] for p in metadata["packages"] if p["name"] == "bota_client_core")
        producer = Path(manifest).parents[2]
        target = ROOT / "target/bota"
        process = start([CARGO, "build", "--locked", "--release", "-p", "bota_server", "--features", "websocket",
                         "--target-dir", str(target)], cwd=producer)
        if finish(process):
            raise RuntimeError("the test game server did not build")
        environment["BOTA_SERVER_BIN"] = str(target / "release/bota_server")
    return environment


def listed(executable, arguments, directory, environment):
    output = capture([executable, *arguments, "--list", "--format=terse"], cwd=directory, env=environment)
    return {line.removesuffix(": test") for line in output.splitlines() if line.endswith(": test")}


def lanes(artifacts, environment):
    menus, rest, full_matches = [], [], []
    for executable, (name, directory) in artifacts.items():
        if name == "dbota" and directory == ROOT:
            whole = listed(executable, [], directory, environment)
            menu = listed(executable, [MENU, "--skip", FULL_MATCH], directory, environment)
            other = listed(executable, ["--skip", MENU], directory, environment)
            full = listed(executable, [FULL_MATCH, "--exact"], directory, environment)
            if menu & other or menu & full or other & full or menu | other | full != whole:
                raise RuntimeError("menu and remaining test lists do not partition the binary")
            print(f"test partition: {len(whole)} = {len(menu)} menu-core + {len(other)} remaining + {len(full)} exclusive full-match (including ignored)")
            if menu:
                menus.append((executable, [MENU, "--skip", FULL_MATCH], directory))
            if other:
                rest.append((executable, ["--skip", MENU], directory))
            if full:
                full_matches.append((executable, [FULL_MATCH, "--exact"], directory))
        else:
            rest.append((executable, [], directory))
    return menus, rest, full_matches


def run_lane(jobs, environment, output, fail_fast):
    result = 0
    with output.open("w") as log:
        for executable, arguments, directory in jobs:
            if CANCELLED.is_set():
                return 130
            try:
                process = start([executable, *arguments, "--test-threads=1"], cwd=directory, env=environment,
                                stdout=log, stderr=subprocess.STDOUT)
            except Cancelled:
                return 130
            status = finish(process)
            result = status or result
            if status and fail_fast:
                print(f"test runner: {Path(executable).name} exited {status}; cancelling other binaries",
                      file=sys.stderr, flush=True)
                cancel()
                return status
    return result


def main():
    arguments = sys.argv[1:]
    metadata = json.loads(capture([CARGO, "metadata", "--locked", "--format-version=1"], cwd=ROOT))
    cargo, tests, libraries, features, needs_server = options(arguments, metadata)
    fail_fast = "--no-fail-fast" not in cargo
    if libraries is None:
        environment = server_environment(metadata, needs_server and "--doc" not in cargo and "--no-run" not in cargo)
        process = start([CARGO, "test", *PROFILE, *cargo, "--", *tests, "--test-threads=1"], cwd=ROOT, env=environment)
        return finish(process)
    artifacts = build(cargo, metadata)
    environment = server_environment(metadata, any(name == "dbota" and directory == ROOT for name, directory in artifacts.values()))
    directories = {str(Path(executable).parent) for executable in artifacts}
    inherited = [path for path in environment.get("LD_LIBRARY_PATH", "").split(os.pathsep) if path]
    environment["LD_LIBRARY_PATH"] = os.pathsep.join([*sorted(directories), *inherited])
    ROOT.joinpath("target").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="test-runner-", dir=ROOT / "target") as scratch:
        logs = [Path(scratch) / name for name in ("menu.log", "remaining.log", "full-match.log")]
        try:
            menu, remaining, full_matches = lanes(artifacts, environment)
            # MenuCore tests start network cores/servers, never asset-loaded Bevy Clients.
            # Every asset-client test stays in the single remaining lane, including two-client tests.
            with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                jobs = [pool.submit(run_lane, group, environment, log, fail_fast) for group, log in zip((menu, remaining), logs[:2])]
                try:
                    result = any(job.result() != 0 for job in jobs)
                except BaseException:
                    cancel()
                    raise
            # This lockstep game has a wall-clock service lifetime. Let it finish without
            # competing with asset-loaded clients, after every parallel test process exits.
            if not (result and fail_fast):
                result = bool(run_lane(full_matches, environment, logs[2], fail_fast)) or result
        finally:
            for log in logs:
                if log.exists():
                    with log.open() as source:
                        shutil.copyfileobj(source, sys.stdout)
    if result and fail_fast:
        return 1
    if libraries:
        packages = [item for name in libraries for item in ("-p", name)]
        extra = [] if fail_fast else ["--no-fail-fast"]
        process = start([CARGO, "test", *PROFILE, *features, *packages, *extra, "--doc", "--", "--test-threads=1"], cwd=ROOT, env=environment)
        result = bool(finish(process)) or result
    return int(result)


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, terminate)
    signal.signal(signal.SIGINT, terminate)
    try:
        sys.exit(main())
    except Terminated as signal_exit:
        sys.exit(128 + signal_exit.number)
    except (RuntimeError, OSError, ValueError) as error:
        print(f"test runner: {error}", file=sys.stderr)
        sys.exit(1)
    finally:
        cancel()
