#!/usr/bin/env python3
"""Run legacy tests and a native Metal smoke against a managed vanilla server.

All server state, downloads, captures and logs stay in the system temporary
folder. Requires an owned 1.8.9 client jar and a Java 8 installation.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request

REPO = Path(__file__).resolve().parents[1]
ROOT = Path(tempfile.gettempdir()) / "mc-1.8.9-ref"
TARGET = Path(tempfile.gettempdir()) / "horizon-189-native-target"
SERVER_SHA1 = "b58b2ceb36e01bcd8dbf49c8fb66c55a9f0676cd"
SERVER_URL = f"https://launcher.mojang.com/v1/objects/{SERVER_SHA1}/server.jar"
PACKAGES = [
    "horizon-189-client", "horizon-protocol-47", "horizon-auth",
    "horizon-world-189", "horizon-assets-189", "horizon-physics-189",
    "horizon-client-platform", "horizon-threads",
]


def java8() -> Path:
    candidates = []
    if value := os.environ.get("HORIZON_189_REFERENCE_JAVA"):
        candidates.append(Path(value))
    if value := os.environ.get("JAVA_HOME"):
        candidates.append(Path(value) / "bin/java")
    candidates.extend((Path.home() / "Library/Java/JavaVirtualMachines").glob("*/Contents/Home/bin/java"))
    if value := shutil.which("java"):
        candidates.append(Path(value))
    for candidate in candidates:
        result = subprocess.run([str(candidate), "-version"], capture_output=True, text=True, timeout=5)
        if result.returncode == 0 and 'version "1.8.' in result.stderr:
            return candidate
    raise RuntimeError("Java 8 is required for the isolated reference server")


def prepare() -> None:
    ROOT.mkdir(parents=True, exist_ok=True)
    jar = ROOT / "server.jar"
    if not jar.exists():
        staged = ROOT / "server.jar.download"
        try:
            with urllib.request.urlopen(SERVER_URL,timeout=30) as response, staged.open("wb") as output:
                shutil.copyfileobj(response,output)
            if hashlib.sha1(staged.read_bytes()).hexdigest() != SERVER_SHA1:
                raise RuntimeError("Reference server SHA-1 mismatch")
            staged.replace(jar)
        finally:
            staged.unlink(missing_ok=True)
    if hashlib.sha1(jar.read_bytes()).hexdigest() != SERVER_SHA1:
        raise RuntimeError("Existing reference server SHA-1 mismatch")
    (ROOT / "eula.txt").write_text("eula=true\n")
    (ROOT / "server.properties").write_text("\n".join([
        "server-ip=127.0.0.1", "server-port=25565", "online-mode=false",
        "level-name=validation-world", "level-type=FLAT", "level-seed=0",
        "view-distance=2", "max-players=4", "spawn-protection=0",
        "difficulty=1", "gamemode=0", "enable-rcon=false", "enable-query=false",
        "spawn-animals=false", "spawn-monsters=false", "allow-flight=false", "",
    ]))


def main() -> None:
    # Refuse to interfere with a server this invocation did not start.
    with socket.socket() as probe:
        probe.settimeout(1)
        if probe.connect_ex(("127.0.0.1", 25565)) == 0:
            raise RuntimeError("Reference port 25565 is already occupied")
    java = java8()
    prepare()
    env = dict(os.environ)
    env["PATH"] = str(Path.home() / ".cargo/bin") + os.pathsep + env["PATH"]
    env["CARGO_INCREMENTAL"] = "0"
    env["CARGO_BUILD_JOBS"] = "4"
    scratch = ROOT / "gate-home"
    scratch.mkdir(exist_ok=True)
    env.update(
        HORIZON_HOME=str(scratch / "runtime"),
        HORIZON_LAUNCHER_HOME=str(scratch / "launcher"),
        XDG_DATA_HOME=str(scratch / "data"),
        XDG_CONFIG_HOME=str(scratch / "config"),
        HORIZON_LAUNCHER_NO_SELF_UPDATE="1",
        HORIZON_NO_AUDIO="1",
    )
    owned = Path(env.get("HORIZON_189_TEST_JAR", str(Path.home() / "Library/Application Support/minecraft/versions/1.8.9/1.8.9.jar")))
    if not owned.is_file():
        raise RuntimeError("Set HORIZON_189_TEST_JAR to the owned client jar")
    ready = ROOT / "fixture-ready"
    ready.unlink(missing_ok=True)
    env.update(HORIZON_189_TEST_JAR=str(owned), HORIZON_189_REFERENCE_GATE="1", HORIZON_189_FIXTURE_READY=str(ready))
    server_log = ROOT / "reference-gate-server.log"
    test_log = ROOT / "reference-gate-tests.log"
    child = None
    with server_log.open("w") as log:
        server = subprocess.Popen([str(java), "-Xms128M", "-Xmx768M", "-jar", "server.jar", "nogui"], cwd=ROOT, stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT, text=True)
        def command(text: str) -> None:
            assert server.stdin is not None
            server.stdin.write(text + "\n")
            server.stdin.flush()
        try:
            deadline = time.monotonic() + 60
            while "Done (" not in server_log.read_text(errors="replace"):
                if server.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(f"Reference startup failed; inspect {server_log}")
                time.sleep(0.2)
            command("time set 18000")
            command("gamerule doMobSpawning false")
            args = ["cargo", "test", "--offline"]
            for package in PACKAGES:
                args.extend(["-p", package])
            args.extend(["--target", "aarch64-apple-darwin", "--target-dir", str(TARGET), "--", "--include-ignored", "--nocapture"])
            configured = placed = mined = spectator = death = split = collected = False
            with test_log.open("w") as output:
                child = subprocess.Popen(args, cwd=REPO, env=env, stdout=output, stderr=subprocess.STDOUT)
                deadline = time.monotonic() + 300
                while child.poll() is None:
                    text = server_log.read_text(errors="replace")
                    if "NativeCombat joined the game" in text and not configured:
                        configured = True
                        command("tp NativeCombat 0.5 4 0.5 0 0")
                        time.sleep(2)
                        for setup in [
                            "kill @e[type=!Player]", "fill -3 3 -3 4 3 6 minecraft:stone",
                            "fill -3 4 -3 4 8 6 minecraft:air", "tp NativeCombat 0.5 4 0.5 0 0",
                            "gamemode 0 NativeCombat",
                            "replaceitem entity NativeCombat slot.hotbar.0 minecraft:diamond_sword",
                            "replaceitem entity NativeCombat slot.hotbar.1 minecraft:cobblestone 16",
                            'setblock 2 4 0 minecraft:chest 4 replace {Items:[{Slot:0b,id:"minecraft:diamond",Count:3b,Damage:0s}]}',
                            "summon Zombie 0.5 4 2.5 {NoAI:1,PersistenceRequired:1}",
                        ]:
                            command(setup)
                        time.sleep(0.3)
                        ready.touch()
                    progress = test_log.read_text(errors="replace")
                    if "REFERENCE_BLOCK_PLACEMENT_SENT" in progress and not placed:
                        placed = True
                        command("testforblock 0 4 1 minecraft:cobblestone")
                    if "REFERENCE_DIG_FINISHED" in progress and not mined:
                        mined = True
                        command("testforblock 0 4 1 minecraft:air")
                    if "REFERENCE_DRAG_SPLIT_SENT" in progress and not split:
                        split = True
                        command('testforblock 2 4 0 minecraft:chest 4 {Items:[{Slot:1b,id:"minecraft:diamond",Count:1b},{Slot:2b,id:"minecraft:diamond",Count:1b}]}')
                    if "REFERENCE_COLLECT_SENT" in progress and not collected:
                        collected = True
                        command('testforblock 2 4 0 minecraft:chest 4 {Items:[]}')
                    if "REFERENCE_SPECTATOR_REQUESTED" in progress and not spectator:
                        spectator = True
                        command("fill 0 4 2 1 7 2 minecraft:stone")
                        command("gamemode 3 NativeCombat")
                    if "REFERENCE_DEATH_REQUESTED" in progress and not death:
                        death = True
                        command("gamemode 0 NativeCombat")
                        command("kill NativeCombat")
                    if time.monotonic() > deadline:
                        raise TimeoutError(f"Reference tests timed out; inspect {test_log}")
                    time.sleep(0.1)
            if child.returncode:
                raise RuntimeError(f"Reference tests failed; inspect {test_log}")
            if server_log.read_text(errors="replace").count("Successfully found the block at 0,4,1") < 2:
                raise AssertionError("Server did not confirm both placement and mining")
            if server_log.read_text(errors="replace").count("Successfully found the block at 2,4,0") < 2:
                raise AssertionError("Server did not confirm drag-split and collect contents")
            # The jar-backed meshing regression is a required gate, not an optional skip.
            required = [
                "terrain::tests::greedy_meshing_covers_exactly_the_cells_and_texels_of_per_block_faces",
                "scene_tests::packed_terrain_fields_decode_on_metal",
                "scene_tests::bindless_terrain_pages_match_array_sampling_on_metal",
            ]
            results = test_log.read_text(errors="replace")
            for name in required:
                if f"test {name} ... ok" not in results:
                    raise RuntimeError(f"Required meshing regression did not pass: {name}")

            build = ["cargo", "build", "--offline", "-p", "horizon-189-client", "--target", "aarch64-apple-darwin", "--target-dir", str(TARGET)]
            subprocess.run(build, cwd=REPO, env=env, check=True, timeout=300, stdout=subprocess.DEVNULL)
            binary = TARGET / "aarch64-apple-darwin/debug/horizon-189-client"
            with (ROOT / "reference-gate-metal.log").open("w") as output:
                child = subprocess.Popen([str(binary), "--offline", "--username", "NativeMetal", "--server", "127.0.0.1", "--smoke-frames", "120", "--smoke-chat", "native chat check"], cwd=ROOT, env=env, stdout=output, stderr=subprocess.STDOUT)
                child.wait(timeout=60)
            if child.returncode:
                raise RuntimeError("Native Metal smoke failed")
            if "MESH_VIEW_READY source=live-camera" not in (ROOT / "reference-gate-metal.log").read_text(errors="replace"):
                raise AssertionError("Native camera did not reach the mesh scheduler")
            if "<NativeMetal> native chat check" not in server_log.read_text(errors="replace"):
                raise AssertionError("Server did not receive the chat sent through the chat screen")
            flags = [line for line in server_log.read_text(errors="replace").splitlines() if "moved wrongly" in line or "moved too quickly" in line]
            if flags:
                raise AssertionError("Reference movement flags detected")
            print("VANILLA_189_REFERENCE_GATE_PASS")
            print(f"Logs retained in {ROOT}")
        finally:
            if child is not None and child.poll() is None:
                child.terminate()
                child.wait(timeout=10)
            if server.poll() is None:
                try:
                    command("stop")
                    server.wait(timeout=30)
                except (BrokenPipeError, subprocess.TimeoutExpired):
                    server.terminate()
                    server.wait(timeout=5)
            ready.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
