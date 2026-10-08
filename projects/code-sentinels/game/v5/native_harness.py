"""Exercise the actual V5 DLL through public commands and native-frame ABI.

No gameplay state injection, private exports, running engine, or GPU is used.
Each invocation redirects save data to a fresh private temporary directory.
"""
from __future__ import annotations

import argparse
import ctypes as C
import hashlib
import json
import math
import os
from pathlib import Path
import tempfile
import time
import traceback

ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
W, H = 32, 20


class Binding(C.Structure):
    _fields_ = [("entity_id", C.c_uint64), ("kind", C.c_uint32), ("data", C.c_int32 * 6)]


class Update(C.Structure):
    _fields_ = [("entity_id", C.c_uint64), ("translation", C.c_float * 3),
                ("scale", C.c_float * 3), ("frame", C.c_int32), ("animator_bool", C.c_int32)]


def xy(cell):
    return (cell % W - 15.5, 9.5 - cell // W)


def footprint(kind, cell):
    size = 3 if kind == 6 else 2 if kind in (1, 2, 4, 5, 8) else 1
    if cell % W + size > W or cell // W + size > H:
        return []
    return [cell + dy * W + dx for dy in range(size) for dx in range(size)]


class Harness:
    def __init__(self):
        self.save = tempfile.TemporaryDirectory(prefix="sentinels-v5-cpu-")
        os.environ["FORGE_GAME_SAVE_DIR"] = self.save.name
        self.build = json.loads((OUT / "native-build.json").read_text())
        manifest=json.loads(Path(self.build["manifest"]).read_text())
        assert hashlib.sha256((ROOT/self.build["module"]).read_bytes()).hexdigest()==manifest["sourceSha256"],"compile current native source before testing"
        self.api = C.CDLL(self.build["dll"])
        for name in ("cs5_input", "cs5_get"):
            fn = getattr(self.api, name)
            fn.argtypes, fn.restype = [C.c_float], C.c_float
        self.api.cs5_reset.argtypes, self.api.cs5_reset.restype = [], C.c_float
        self.api.cs5_frame.argtypes = [C.c_uint32, C.c_float, C.POINTER(Binding), C.c_uint32,
                                       C.POINTER(Update), C.c_uint32]
        self.api.cs5_frame.restype = C.c_uint32
        self.bindings = (Binding * 1)(Binding(77, 0, (C.c_int32 * 6)(0, 1, 2, 3, 4, 5)))
        self.updates = (Update * 1)()
        self.records = []
        self.reset()

    def reset(self):
        assert self.api.cs5_reset() == 1

    def get(self, key):
        return float(self.api.cs5_get(key))

    def fields(self, base, count):
        return [self.get(base + i) for i in range(count)]

    def building(self, slot):
        return self.fields(1000 + slot * 20, 20)

    def unit(self, slot):
        return self.fields(3000 + slot * 24, 24)

    def anim(self, slot, unit=False):
        return self.fields((14000 if unit else 13000) + slot * 8, 8)

    def events_since(self, sequence):
        latest=round(self.get(12001))
        return [self.fields(16000+((seq-1)%256)*12,12)
                for seq in range(max(sequence+1,latest-255),latest+1)]

    def command(self, code, required=True):
        result = float(self.api.cs5_input(float(code)))
        if required and result != 1:
            raise AssertionError(f"command {code} failed: feedback={self.get(14)} credits={self.get(0)} compute={self.get(1)}")
        return result

    def step(self, dt):
        count = self.api.cs5_frame(1, dt, self.bindings, 1, self.updates, 1)
        assert count == 1 and self.updates[0].entity_id == 77

    def advance(self, seconds, quantum=0.25):
        remaining = seconds
        while remaining > 1e-7:
            dt = min(quantum, remaining)
            self.step(dt)
            remaining -= dt

    def visual(self, visual_id, kind=2):
        bindings = (Binding * 1)(Binding(987, kind, (C.c_int32 * 6)(visual_id, 0, 0, 0, 0, 0)))
        updates = (Update * 1)()
        assert self.api.cs5_frame(1, 0, bindings, 1, updates, 1) == 1
        u = updates[0]
        return {"position": list(u.translation), "scale": list(u.scale), "frame": u.frame, "attacking": u.animator_bool}

    def record(self, name, **data):
        row = {"name": name, "time": self.get(12000), **data}
        self.records.append(row)
        print(json.dumps(row), flush=True)
        return row

    def tiles(self):
        result = []
        for row in range(H):
            for part in range(4):
                bits = round(self.get(6000 + row * 6 + part))
                result.extend((bits >> (i * 3)) & 7 for i in range(8))
        return result

    def occupied(self):
        cells = set()
        for slot in range(48):
            b = self.building(slot)
            if b[9] > .5:
                cells.update(footprint(round(b[0]), round(b[1])))
        for slot in range(32):
            u = self.unit(slot)
            if u[0] > 0:
                cells.add(round(u[1]))
        return cells

    def candidates(self, kind, near=323, predicate=lambda c: True):
        tiles, occupied = self.tiles(), self.occupied()
        def valid(c):
            cells = footprint(kind, c)
            if not cells or any(tiles[x] in (1, 2) or x in occupied for x in cells):
                return False
            if kind == 9 and tiles[c] not in (5, 6):
                return False
            if kind in (4, 5, 6):
                terrain, radius = {4: (2, 2.1), 5: (6, 3.1), 6: (2, 4.1)}[kind]
                if not any(t == terrain and math.dist(xy(c), xy(i)) <= radius for i, t in enumerate(tiles)):
                    return False
            return predicate(c)
        return sorted((c for c in range(640) if valid(c)), key=lambda c: math.dist(xy(c), xy(near)))

    def buy(self, kind, cell=None, near=323):
        if cell is None:
            cell = self.candidates(kind, near)[0]
        self.command(1_000_000 + kind * 1000 + cell)
        return next(i for i in range(48) if self.building(i)[1] == cell)

    def buy_unit(self, kind, cell):
        self.command(4_000_000 + kind * 1000 + cell)
        return next(i for i in range(32) if self.unit(i)[1] == cell)

    def connect(self, kind, a, b):
        # Route through genuine passable cells; each edge is an ordinary command.
        # Prefer the native L route to keep the real 64-link budget available.
        tiles = self.tiles()
        def line(x, y):
            path = [x]
            while x % W != y % W:
                x += 1 if x % W < y % W else -1
                path.append(x)
            while x // W != y // W:
                x += W if x // W < y // W else -W
                path.append(x)
            return path
        paths = [[(a,b)], [(b,a)]]
        for segments in paths:
            if all(all(tiles[c] not in (1,2) for c in line(x,y)) for x,y in segments):
                before = [self.get(4000+i*8+4) for i in range(64)]
                self.command((5_000_000 if kind==1 else 6_000_000)+segments[0][0]*1000+segments[0][1])
                return next(i for i in range(64) if not before[i] and self.get(4000+i*8+4))
        raise AssertionError(f"no direct legal cable route {a}->{b}")

    def fund(self, amount):
        if self.get(10) <= 0:
            self.buy(9)
        if self.get(0) < amount:
            self.advance((amount-self.get(0))/self.get(10)+.1)


def lifecycle(h):
    h.reset()
    first = h.anim(0)
    h.advance(.5)
    quarter = h.anim(0)
    assert first[0:2] == [1,0] and quarter[0] == 1 and 7 <= quarter[1] <= 8
    h.advance(1.65)
    assert h.anim(0)[0] == 2
    h.record("core_land_work", initial=first, at_half_second=quarter, after_land=h.anim(0))
    h.command(10_000_002)
    frozen = h.fields(12000,6)+h.anim(0)+h.fields(0,3)
    h.advance(2)
    assert frozen == h.fields(12000,6)+h.anim(0)+h.fields(0,3)
    h.command(10_000_002)
    h.record("pause_freezes_state_and_clock", frozen=frozen)
    extractor = h.buy(9)
    h.advance(2.1)
    assert h.anim(extractor)[0] == 2 and h.get(10) > 0
    h.fund(5000)
    kind, cell = 3, h.candidates(3)[0]
    slot = h.buy(kind, cell)
    h.advance(2.1)
    assert h.anim(slot)[0] == 2
    h.command(2_100_000+slot)
    death = [h.anim(slot)]
    assert death[-1][0:2] == [4,80] and h.building(slot)[9] == 0
    h.advance(1.95, .05); death.append(h.anim(slot))
    assert death[-1][0] == 4 and death[-1][1] >= 125
    h.advance(.15, .05); death.append(h.anim(slot))
    assert death[-1][0:2] == [5,127]
    h.advance(2.25, .05); death.append(h.anim(slot))
    assert death[-1][0:2] == [5,127]
    h.advance(.2, .05); death.append(h.anim(slot))
    assert death[-1][0] == 0
    h.record("complete_destroy_and_wreck", samples=death)
    slot = h.buy(3, cell)
    h.command(2_100_000+slot)
    new_slot = h.buy(3, cell)
    assert slot == new_slot and h.get(12002) == 1 and h.anim(slot)[0] == 1
    assert h.visual(1200)["frame"] == (3-1)*128+80
    h.record("slot_reuse_retains_old_visual", current=h.anim(slot), ghost=h.fields(15000,8))
    h.advance(4.6)
    assert h.get(12002) == 0
    # Rapid legal sell/rebuild creates more deaths than the fixed ghost budget.
    h.fund(7000)
    for _ in range(27):
        h.command(2_100_000+slot)
        h.buy(3, cell)
    assert h.get(12002) == 24
    alpha=[h.visual(1400+i) for i in range(64)]
    additive=[h.visual(1300+i) for i in range(64)]
    assert sum(v["position"][0]>-90 for v in alpha)==12
    assert not any(v["position"][0]>-90 for v in additive)
    saturated = h.anim(slot)
    assert saturated[0] == 4
    h.advance(4.35, .05)
    assert h.get(12002) == 24 and h.anim(slot)[0] == 5
    h.advance(5, .05)
    assert h.get(12002) == 0 and h.anim(slot)[0] == 2
    h.record("ghost_budget_queue_preserves_all_tails", saturated=saturated, recovered=h.anim(slot),activeAlphaAtSaturation=12)
    before=h.fields(12000,6)+h.fields(0,6)
    for _ in range(3):
        h.visual(1000)
        h.step(0)
    assert before == h.fields(12000,6)+h.fields(0,6)
    assert h.visual(1000)["scale"][0] == C.c_float(2/.640625).value
    h.record("zero_dt_reconnect_and_native_scale", visual=h.visual(1000))


def abi(h):
    h.reset()
    h.advance(2.1)
    before=h.fields(0,6)+h.fields(12000,6)
    assert C.sizeof(Binding)==40 and C.sizeof(Update)==40
    assert h.api.cs5_frame(2,.25,h.bindings,1,h.updates,1)==0
    assert h.api.cs5_frame(1,.25,h.bindings,2,h.updates,1)==0
    assert h.api.cs5_frame(1,.25,None,1,h.updates,1)==0
    assert before==h.fields(0,6)+h.fields(12000,6)
    bad=(Binding*1)(Binding(77,99,(C.c_int32*6)(0,0,0,0,0,0)))
    assert h.api.cs5_frame(1,.25,bad,1,h.updates,1)==0
    after=h.fields(0,6)+h.fields(12000,6)
    h.record("invalid_binding_does_not_advance_simulation",before=before,after=after)
    assert before==after,"invalid native binding advanced game state before rejection"


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--suite",default="lifecycle",choices=["lifecycle","economy","campaign","battle","walls","abi"])
    args=parser.parse_args()
    h=Harness(); started=time.perf_counter()
    report={"suite":args.suite,"dll":h.build["dll"],"processId":os.getpid(),
            "publicAbiOnly":True,"dllLoaded":True,"gameplayStateInjected":False,"gpuUsed":False}
    try:
        if args.suite == "lifecycle": lifecycle(h)
        elif args.suite == "abi": abi(h)
        else:
            from native_campaign import campaign, economy, battle, walls
            {"campaign":campaign,"economy":economy,"battle":battle,"walls":walls}[args.suite](h)
        report["passed"]=True
    except Exception as error:
        report.update(passed=False,error=str(error),traceback=traceback.format_exc())
    finally:
        report.update(wallSeconds=time.perf_counter()-started,records=h.records)
        (OUT/f"cpu-{args.suite}-results.json").write_text(json.dumps(report,indent=2),encoding="utf8")
        print(json.dumps({k:v for k,v in report.items() if k!="records"}),flush=True)
    raise SystemExit(0 if report["passed"] else 1)


if __name__ == "__main__":
    main()
