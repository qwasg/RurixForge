"""Freeze an existing marked V6 preview into an explicit Inno Setup file list.

This is preview packaging, not the final-release acceptance workflow. Original
game files, candidate marker, QA and historical receipts remain byte-identical.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil


HERE = Path(__file__).resolve().parent
EXPECTED_HOST = "24fd6b5bc191802054fef4a74203052bc78f298032f31b33582ddabc750d9a06"
EXPECTED_RULES = "58b79154d869577a3787dc2f45a3479c27e1dfebd453f347dcf2d4c97270eafa"
EXPECTED_MARKER = "17b05029e90aa8284e2e6179196b7c9839cff3554386386e21125c45660c1598"


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def canonical(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def checked_relative(value):
    rel = PurePosixPath(value)
    if not value or rel.is_absolute() or ".." in rel.parts or any(c in value for c in '\x00\r\n"{}:'):
        raise ValueError(f"Unsafe package path: {value!r}")
    if any(p.casefold() in {"private", "logs", "saves", ".forge", ".git", ".codex", "node_modules", "target"} or p.casefold().startswith(".env") for p in rel.parts):
        raise ValueError(f"Runtime/private data cannot enter the installer: {value}")
    return rel


def runtime_path(name):
    return name.startswith(("bin/", "Content/", "Web/", "v6/")) or name in {
        "bridge.mjs", "multiplayer-v6.mjs", "forge.toml", "Start-Game.cmd", "Start-Game-GPU-Particles.cmd"
    }


def write_json(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--candidate", required=True, type=Path)
    ap.add_argument("--work", required=True, type=Path)
    ap.add_argument("--preview", action="store_true", required=True)
    ap.add_argument("--build-date", default="20260914")
    ap.add_argument("--compiler-license", type=Path, required=True)
    args = ap.parse_args()
    root = args.candidate.resolve(strict=True)
    work = args.work.resolve()
    if work.exists():
        raise ValueError("Use a new build work directory; previous attempts are preserved")
    marker_path = root / "v6-candidate.json"
    if digest(marker_path) != EXPECTED_MARKER:
        raise ValueError("Candidate marker changed from the reviewed game snapshot")
    marker = json.loads(marker_path.read_text(encoding="utf-8-sig"))
    if marker.get("candidate") is not True or marker.get("version") != 6:
        raise ValueError("This entry point only packages a marked V6 preview")
    if marker["engineSha256"] != EXPECTED_HOST or marker["rulesFingerprint"] != EXPECTED_RULES:
        raise ValueError("Wrong preview native/rules identity")
    records = marker["files"]
    names = [r["path"] for r in records]
    if len(set(n.casefold() for n in names)) != len(names):
        raise ValueError("Duplicate case-insensitive package paths")
    if marker["included"] != sorted(names + ["v6-candidate.json"]):
        raise ValueError("Candidate included list differs from its file manifest")
    if canonical(records) != marker["manifestSha256"]:
        raise ValueError("Candidate full manifest digest is invalid")
    runtime_records = [r for r in records if runtime_path(r["path"])]
    if canonical(runtime_records) != marker["payloadSha256"]:
        raise ValueError("Candidate runtime payload digest is invalid")
    stage = work / "payload"
    stage.mkdir(parents=True)
    for record in records:
        rel = checked_relative(record["path"])
        lexical = root.joinpath(*rel.parts)
        source = lexical.resolve(strict=True)
        if lexical.is_symlink() or not source.is_relative_to(root):
            raise ValueError(f"Source escapes candidate: {rel}")
        if source.stat().st_size != record["bytes"] or digest(source) != record["sha256"]:
            raise ValueError(f"Candidate file changed: {rel}")
        dest = stage.joinpath(*rel.parts)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, dest)
        if dest.stat().st_size != record["bytes"] or digest(dest) != record["sha256"]:
            raise ValueError(f"Staged copy mismatch: {rel}")
    shutil.copyfile(marker_path, stage / marker_path.name)
    if digest(stage / marker_path.name) != EXPECTED_MARKER:
        raise ValueError("Staged marker differs from reviewed marker")
    game_records = records + [{"path": marker_path.name, "bytes": marker_path.stat().st_size, "sha256": EXPECTED_MARKER}]
    if {f.relative_to(stage).as_posix() for f in stage.rglob("*") if f.is_file()} != set(marker["included"]):
        raise ValueError("Staging contains files outside the frozen game allowlist")
    overlays = [(HERE / "INSTALL-README.txt", "INSTALL-README.txt")]
    overlays.append((args.compiler_license.resolve(strict=True), "Licenses/Inno-Setup-LICENSE.txt"))
    for name in ("Play-Game.cmd", "launcher.mjs"):
        overlays.append((HERE / "overlay" / name, name))
    overlay_records = []
    for source, name in overlays:
        if not source.is_file() or (stage / name).exists():
            raise ValueError(f"Missing or conflicting installer overlay: {name}")
        (stage / name).parent.mkdir(parents=True, exist_ok=True)
        expected_overlay_hash = digest(source)
        shutil.copyfile(source, stage / name)
        if digest(stage / name) != expected_overlay_hash:
            raise ValueError(f"Installer overlay changed during staging: {name}")
        overlay_records.append({"path": name, "bytes": (stage / name).stat().st_size, "sha256": expected_overlay_hash})
    details = {
        "type": "preview-installer-wrapper", "candidate": True, "finalAcceptanceClaimed": False,
        "rulesFingerprint": EXPECTED_RULES, "engineSha256": EXPECTED_HOST,
        "originalGamePayloadSha256": marker["payloadSha256"], "originalMarkerSha256": EXPECTED_MARKER,
        "originalGameFileCount": len(game_records), "installerOverlayFiles": overlay_records,
        "signed": False, "signpathGrantReceived": False,
        "dataHandling": "Install-time copy excludes play-created saves/logs. Uninstall leaves later-created user files.",
    }
    meta = stage / "INSTALLER-PREVIEW.json"
    write_json(meta, details)
    overlay_records.append({"path": meta.name, "bytes": meta.stat().st_size, "sha256": digest(meta)})
    install_records = sorted(game_records + overlay_records, key=lambda r: r["path"])
    # Identical sources are referenced by one canonical path to let Inno share
    # their compressed bytes. Every distinct destination remains in the list.
    unique = {}
    entries = []
    for item in install_records:
        rel = checked_relative(item["path"])
        key = (item["sha256"], item["bytes"])
        source_name = unique.setdefault(key, item["path"])
        source_path = str(stage / source_name).replace('"', '""')
        parent = str(rel.parent).replace("/", "\\")
        dest_dir = "{app}" + ("\\" + parent if parent != "." else "")
        entries.append(f'Source: "{source_path}"; DestDir: "{dest_dir}"; DestName: "{rel.name}"; Flags: ignoreversion')
    files_include = work / "payload-files.iss"
    files_include.write_text("\n".join(entries) + "\n", encoding="utf-8-sig")
    out = work / "output"
    out.mkdir()
    definitions = {
        "PayloadDir": str(stage), "FilesInclude": str(files_include),
        "InstallerOutput": str(out), "BuildDate": args.build_date,
    }
    preamble = "\n".join(f'#define {k} "{v}"' for k, v in definitions.items())
    script = work / "installer.iss"
    script.write_text(preamble + "\n" + (HERE / "CodeSentinelsV6Preview.iss").read_text(encoding="utf-8"), encoding="utf-8-sig")
    write_json(work / "installed-files.json", {"schemaVersion": 1, "files": install_records})
    receipt = {
        "status": "preview-installer-staging-verified-not-compiled", "recordedAtUtc": datetime.now(timezone.utc).isoformat(),
        "candidateSource": str(root), "payloadStage": str(stage),
        "originalGameFiles": len(game_records), "installFiles": len(install_records),
        "originalGameBytes": sum(r["bytes"] for r in game_records),
        "installBytes": sum(r["bytes"] for r in install_records),
        "uniqueSourceBlobs": len(unique), "uniqueSourceBytes": sum(size for _, size in unique),
        "originalGamePayloadSha256": marker["payloadSha256"], "engineSha256": EXPECTED_HOST,
        "rulesFingerprint": EXPECTED_RULES, "everyOriginalFileHashVerified": True,
        "candidateMarkerPreserved": True, "existingCandidateMutated": False,
        "runtimeUserDataIncluded": False, "privateSigningApplicationIncluded": False,
        "installerOverlayFiles": overlay_records, "installedFilesManifestSha256": digest(work / "installed-files.json"),
        "innoScriptSha256": digest(script), "filesIncludeSha256": digest(files_include),
        "compilerExecuted": False, "installerExecuted": False, "signed": False,
        "finalGameAcceptanceClaimed": False,
    }
    write_json(work / "staging-receipt.json", receipt)
    print(json.dumps({k: receipt[k] for k in ("status", "installFiles", "installBytes", "uniqueSourceBlobs", "uniqueSourceBytes", "engineSha256")}))
    print(str(script))


if __name__ == "__main__":
    main()
