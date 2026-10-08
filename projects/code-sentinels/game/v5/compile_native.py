"""Compile V5 and write its portable native cache manifest without loading it.

This command does not start the game or execute tests. The lifecycle fragment is
kept in sync for review; the DLL is built from the complete Rust module.
"""
import hashlib
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
MODULE = "Content/Scripts/sentinels_v5.rs"
source = (ROOT / MODULE).read_bytes()
version = subprocess.run(
    ["rustc", "--version", "--verbose"], check=True, stdout=subprocess.PIPE
).stdout
cache = ROOT / ".forge/cache/rxdll"
cache.mkdir(parents=True, exist_ok=True)
key = hashlib.sha256(
    source + b"|rust-cdylib-v1|edition2021|opt2|panic-abort|" + version
).hexdigest()[:16]
dll = cache / f"sentinels_v5-{key}.dll"
subprocess.run(
    [
        "rustc", str(ROOT / MODULE), "--crate-type", "cdylib", "--edition", "2021",
        "-C", "opt-level=2", "-C", "panic=abort", "-o", str(dll),
    ],
    check=True,
)
manifest_path = dll.with_suffix(".native.json")
manifest = {
    "backend": "rust-cdylib-v1",
    "module": MODULE,
    "sourceSha256": hashlib.sha256(source).hexdigest(),
    "dllSha256": hashlib.sha256(dll.read_bytes()).hexdigest(),
    "dll": dll.name,
}
manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf8")
build = {
    "operation": "native compilation only",
    "module": MODULE,
    "dll": str(dll),
    "manifest": str(manifest_path),
    "bytes": dll.stat().st_size,
    "testsRun": False,
    "gameStarted": False,
    "dllLoaded": False,
}
(ROOT / "game/v5/native-build.json").write_text(
    json.dumps(build, ensure_ascii=False, indent=2), encoding="utf8"
)
print(json.dumps(build, ensure_ascii=False))
