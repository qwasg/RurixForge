"""Run isolated public-ABI scenarios and targeted CPU-only engine regressions."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys

OUT=Path(__file__).resolve().parent
ROOT=OUT.parents[1]
REPO=ROOT.parents[1]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--skip-engine",action="store_true")
    args=parser.parse_args()
    runs=[]
    suites=["abi","lifecycle","economy","battle","walls","campaign"]
    for suite in suites:
        command=[sys.executable,str(OUT/"native_harness.py"),"--suite",suite]
        result=subprocess.run(command,cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding="utf8")
        (OUT/f"cpu-{suite}.log").write_text(result.stdout,encoding="utf8")
        data=json.loads((OUT/f"cpu-{suite}-results.json").read_text())
        row={"name":suite,"passed":result.returncode==0 and data["passed"],"records":len(data["records"]),"dll":data["dll"],"log":f"cpu-{suite}.log"}
        runs.append(row);print(json.dumps(row),flush=True)
    if not args.skip_engine:
        commands=[
            ("native-abi-runtime",["cargo","test","-p","forge-logic","callruntime::tests::native_frame_abi_updates_in_bulk_and_rejects_bad_output_atomically","--","--exact","--nocapture"]),
            ("sprite-schema",["cargo","test","-p","forge-scene","sprite_","--","--nocapture"]),
            ("sprite-variants-cpu",["cargo","test","-p","engine-host","--bin","engine-host","sprite_variants_cpu_frames_pivots_legacy_and_stable_inventory","--","--nocapture"]),
        ]
        for name,command in commands:
            result=subprocess.run(command,cwd=REPO,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding="utf8")
            (OUT/f"{name}.log").write_text(result.stdout,encoding="utf8")
            row={"name":name,"passed":result.returncode==0 and "running 0 tests" not in result.stdout,"command":command,"log":f"{name}.log"}
            runs.append(row);print(json.dumps(row),flush=True)
    report={"finishedUtc":datetime.now(timezone.utc).isoformat(),"passed":all(r["passed"] for r in runs),
            "scope":"public V5 native DLL commands and CPU-only engine contracts","gpuVisualAcceptance":False,
            "isolatedSaveDirectories":True,"gameplayStateInjected":False,"runs":runs}
    (OUT/"cpu-acceptance.json").write_text(json.dumps(report,indent=2),encoding="utf8")
    raise SystemExit(0 if report["passed"] else 1)


if __name__=="__main__":main()
