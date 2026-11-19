#!/usr/bin/env python3
"""Record a deterministic ten-second voice through the native client's real audio stream."""
import argparse
import array
from contextlib import ExitStack
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import tempfile
import time

RATE = 48_000
FRAMES = 10 * RATE
ONSET_TOLERANCE = 0.050

PIPEWIRE = """
context.properties = {
    core.daemon = true
    core.name = pipewire-0
    default.clock.rate = 48000
    default.clock.allowed-rates = [ 48000 ]
    default.clock.quantum = 256
    default.clock.min-quantum = 256
    default.clock.max-quantum = 256
}
context.spa-libs = {
    support.* = support/libspa-support
    audio.convert.* = audioconvert/libspa-audioconvert
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-metadata }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-access }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
]
context.objects = [
    { factory = spa-node-factory args = {
        factory.name = support.node.driver
        node.name = private-driver
        priority.driver = 200000
    } }
    { factory = adapter args = {
        factory.name = support.null-audio-sink
        node.name = probe-sink
        media.class = Audio/Sink
        audio.position = [ FL FR ]
        adapter.auto-port-config = { mode = dsp monitor = true position = preserve }
    } }
]
"""
ALSA = """
pcm.!default {
    type pipewire
    server "pipewire-0"
    playback_node "probe-sink"
    capture_node "probe-sink"
    rate 48000
    channels 2
    hint { show on description "Private probe output" }
}
ctl.!default { type pipewire server "pipewire-0" }
"""


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def process(stack, args, env, log, **options):
    output = stack.enter_context(log.open("wb"))
    child = subprocess.Popen(args, env=env, stderr=output, **options)
    stack.callback(stop, child)
    return child


def wait_for(predicate, what, seconds=10):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.02)
    raise AssertionError(f"timed out waiting for {what}")


def expect_signs(samples):
    assert len(samples) == FRAMES, f"captured {len(samples)} active frames, expected {FRAMES}"
    word = 0xC35A29EF
    for index, sample in enumerate(samples):
        word ^= (word << 13) & 0xffffffff
        word ^= word >> 17
        word ^= (word << 5) & 0xffffffff
        assert abs(sample) > 4096 and (sample > 0) == bool(word & 1), (
            f"silent, out-of-order, or repeated audio at sample {index}: {sample}"
        )


def capture(recorder, client, output):
    pcm = bytearray()
    client_text = bytearray()
    blocks = []
    with selectors.DefaultSelector() as ready:
        ready.register(recorder.stdout, selectors.EVENT_READ, "pcm")
        ready.register(client.stdout, selectors.EVENT_READ, "client")
        deadline = time.monotonic() + 20
        ended_at = None
        while time.monotonic() < deadline:
            for key, _ in ready.select(timeout=0.1):
                data = os.read(key.fd, 1024 if key.data == "pcm" else 8192)
                if not data:
                    ready.unregister(key.fileobj)
                    continue
                if key.data == "pcm":
                    blocks.append((len(pcm), len(data), time.time_ns()))
                    pcm.extend(data)
                else:
                    client_text.extend(data)
            if client.poll() is not None:
                assert client.returncode == 0, "native playback failed (see client.log)"
                ended_at = time.monotonic() if ended_at is None else ended_at
                if time.monotonic() - ended_at > 0.2:
                    break
        else:
            raise AssertionError("the native voice did not finish in 20 seconds")
    (output / "monitor.s16le").write_bytes(pcm)
    trace = client_text.decode()
    (output / "presentation.txt").write_text(trace)
    marks = [line.split() for line in trace.splitlines() if line.startswith("audio-probe-presented ")]
    assert len(marks) == 1, f"one presentation report expected, got {marks}"
    _, host, media = marks[0]
    samples = array.array("h")
    samples.frombytes(pcm)
    mono = samples[::2]
    active = [index for index, sample in enumerate(mono) if abs(sample) > 4096]
    assert active, "no signal reached the private sink"
    first, last = active[0], active[-1]
    expect_signs(mono[first:last + 1])
    byte = first * 4
    begin, size, read_at = next(block for block in blocks if block[0] <= byte < block[0] + block[1])
    monitor_onset = read_at / 1e9 - (begin + size - byte) / (4 * RATE)
    reported_onset = float(host) - float(media)
    error = abs(monitor_onset - reported_onset)
    assert error <= ONSET_TOLERANCE, f"recorded vs reported onset: {error * 1000:.2f} ms"
    print(f"native audio: {FRAMES} consecutive samples, zero gaps or repeats; onset error "
          f"{error * 1000:.2f} ms (limit {ONSET_TOLERANCE * 1000:g} ms)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    for command in ("pipewire", "wireplumber", "pw-record", "pw-link"):
        if not shutil.which(command):
            parser.error(f"this Linux backend check needs {command}")
    output = args.evidence.resolve()
    output.mkdir(parents=True, exist_ok=True)
    # Keep socket paths short. Each process is confined to the named socket and ALSA config.
    with tempfile.TemporaryDirectory(prefix="dbota-audio-") as directory, ExitStack() as stack:
        private = Path(directory)
        (private / "pipewire.conf").write_text(PIPEWIRE)
        (private / "alsa.conf").write_text(ALSA)
        env = dict(os.environ, PIPEWIRE_RUNTIME_DIR=directory, XDG_RUNTIME_DIR=directory,
                   PIPEWIRE_REMOTE="pipewire-0", ALSA_CONFIG_PATH=str(private / "alsa.conf"),
                   XDG_STATE_HOME=str(private / "state"), XDG_CONFIG_HOME=str(private / "config"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:path={directory}/no-bus",
                   DBUS_SYSTEM_BUS_ADDRESS=f"unix:path={directory}/no-bus")
        for key in ("DBOTA_NULL_AUDIO", "PIPEWIRE_CONFIG_DIR", "PIPEWIRE_CONFIG_NAME", "PIPEWIRE_PROPS"):
            env.pop(key, None)
        server = process(stack, ["pipewire"], dict(env, PIPEWIRE_CONFIG_DIR=directory),
                         output / "server.log", stdout=subprocess.DEVNULL)
        wait_for(lambda: (private / "pipewire-0").is_socket(), "the private socket")
        assert server.poll() is None, "the private audio server exited"
        process(stack, ["wireplumber", "--profile", "policy"], env, output / "policy.log",
                stdout=subprocess.DEVNULL)
        recorder = process(stack, ["pw-record", "--target", "probe-sink", "--rate", str(RATE),
                                   "--channels", "2", "--format", "s16", "--latency", "256",
                                   "--raw", "-P", "{ node.name=probe-recorder stream.capture.sink=true }", "-"],
                           env, output / "record.log", stdout=subprocess.PIPE)
        client = process(stack, [str(args.client.resolve()), "audio-output", "--probe"], env,
                         output / "client.log", stdout=subprocess.PIPE, stdin=subprocess.PIPE)
        with selectors.DefaultSelector() as ready:
            ready.register(client.stdout, selectors.EVENT_READ)
            assert ready.select(timeout=10), "the native stream did not open"
            assert client.stdout.readline() == b"audio-probe-ready\n", "the native stream failed to open"

        def linked():
            links = subprocess.run(["pw-link", "-l"], env=env, capture_output=True,
                                   text=True, timeout=2, check=True).stdout
            return "probe-recorder" in links and "alsa_playback" in links

        wait_for(linked, "the monitor and the native stream to link to the null sink")
        client.stdin.write(b"1")
        client.stdin.flush()
        capture(recorder, client, output)


if __name__ == "__main__":
    main()
