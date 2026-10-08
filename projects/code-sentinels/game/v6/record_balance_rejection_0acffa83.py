"""Record the reviewed rejection of one frozen ruleset; never approve a release."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path

base = Path(__file__).resolve().parent
out = base / "final-balance-0acffa83-20260913" / "root-balance-rejection.json"
if out.exists():
    raise SystemExit("Existing decision preserved.")

def read(rel):
    return json.loads((base / rel).read_text(encoding="utf-8"))

def ref(rel):
    p = base / rel
    return {"path": "game/v6/" + rel, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}

prefix = "final-balance-0acffa83-20260913/"
review = read(prefix + "branch/branch-review-data.json")
analysis = read(prefix + "branch/analysis.json")
integrity = read(prefix + "branch/strict-integrity-check.json")
assert review["rulesFingerprint"] == "0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a"
assert integrity["passed"] and analysis["games"] == 1000
branches = {}
for name in ("speed", "security", "algorithm", "science", "lightweight"):
    group = review["branchGroups"][name]["excludingSelf"]
    branches[name] = {k: group[k] for k in (
        "playerObservations", "wins", "losses", "unresolved", "winFractionAllObservations")}
    assert branches[name]["playerObservations"] == 320

decision = {
    "kind": "reviewed-balance-rejection",
    "recordedAtUtc": datetime.now(timezone.utc).isoformat(),
    "rulesFingerprint": review["rulesFingerprint"],
    "status": "rejected",
    "finalEligible": False,
    "finalBalanceAcceptance": False,
    "statisticalIntegrityPassed": True,
    "scope": "Review of the completed frozen 1000-case matrix, not an assertion that all planned gameplay or release gates failed.",
    "games": analysis["games"],
    "completed": analysis["completed"],
    "unresolved": analysis["unresolved"],
    "excludingSameBranchControls": branches,
    "durationSeconds": analysis["durationSeconds"],
    "completedIn30To45Minutes": analysis["completedIn30To45Minutes"],
    "blockingIssues": [
        "Science wins 252/320 cross-branch observations while lightweight wins 84/320; the observed disparity does not meet the intended branch tradeoffs.",
        "Two canonical matches remained unfinished at the retained 55-minute observation boundary. Their outcomes must not be invented or omitted."
    ],
    "followUpConcerns": [
        "The 30-45-minute design target is not a hard artificial victory delay. Preserve actual shorter outcomes and evaluate resource/objective pacing.",
        "The physical direct-projectile diagnostic confirms premature expiry for a receding target inside range. Its contribution to whole-match imbalance remains to be measured in paired reruns.",
        "Frontline energy-defense orders and final inventory do not independently prove shield damage absorption or a causal economic advantage."
    ],
    "nextAction": "Correct the proved projectile lifecycle defect, run fixed paired pilot cases, and review before authorizing a new full matrix. LAN remains on hold pending accepted rules and matching runtime.",
    "evidence": [ref(prefix + rel) for rel in (
        "branch/analysis.json", "branch/branch-review-data.json", "branch/strict-integrity-check.json",
        "strategy/analysis.json", "strategy/strict-integrity-check.json")]
        + [ref("projectile-lifetime-fa31-20260913-b/probe-receipt.json")],
}
out.write_text(json.dumps(decision, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps({"path": str(out), "sha256": hashlib.sha256(out.read_bytes()).hexdigest(), "status": decision["status"]}))
