# gen_graphs.py — 按 docs/spec/graphs.md v2 生成 PvZ 核心行为图(.rxgraph JSON)
# 铁律(已验证):call_function 是动作节点,必须先入执行链,result 才可读;
#   纯节点(var.get/has_tag/find_by_tag/get_transform)只被引用、不进链。
# 图解释器没有字符串比较/向量构造/循环:
#   - 数值分支一律走 cmpT 累加 + is_zero/nonpositive;
#   - 位置只能「查表」:find_by_tag(格子) → get_transform → set_transform(目标);
#   - 动作名分支用 var.set(name=<action pin>) 把输入值存进同名变量再读(maze 先例)。
# 指针输入契约(engine-host logic.inject_pointer):一次点击派发 click_x / click_y / click_z
#   (世界坐标)与 click(=1) 四条输入,同帧按序到达。
# 用法: python gen_graphs.py
import json, pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "Content" / "Graphs"
RULES = "Content/Scripts/pvz_rules.rx"
DATA = "Content/Scripts/pvz_data.rx"


def off(x: float, y: float) -> dict:
    return {"rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "translation": [x, y, 0.0]}


# 各类对象池的停车位互不重叠(此前全部回收到 (0,-60) 会让回收僵尸与回收豌豆持续互触发)。
OFF_ZOMBIE = off(0.0, -60.0)
OFF_PEA = off(0.0, -70.0)
OFF_SUN = off(0.0, -80.0)
OFF_PLANT = off(-40.0, -60.0)
OFF_UI = off(0.0, -90.0)
BANNER_POS = {"rotation": [0.0, 0.0, 0.0, 1.0], "scale": [1.0, 1.0, 1.0], "translation": [0.0, 0.8, 0.5]}
SKY_SUN_Y = 6.6


class G:
    def __init__(self, gid):
        self.doc = {"version": 1, "id": gid, "name": gid, "exposedProps": [], "nodes": [], "edges": []}
        self._n = 0
    def prop(self, name, default, kind="F32"):
        self.doc["exposedProps"].append({"name": name, "kind": kind, "default": default})
    def node(self, ntype, nid=None, **inputs):
        self._n += 1
        nid = nid or f"n{self._n}"
        nd = {"id": nid, "type": ntype, "pos": [0.0, 0.0]}
        if inputs:
            nd["inputs"] = dict(inputs)
        self.doc["nodes"].append(nd)
        return nid
    @staticmethod
    def c(v): return {"const": v}
    @staticmethod
    def r(name): return {"ref": name}
    @staticmethod
    def p(node, pin): return {"node": node, "pin": pin}
    def edge(self, a, apin, b, bpin="exec"):
        self.doc["edges"].append({"from": [a, apin], "to": [b, bpin]})
    def emit(self, filename):
        p = OUT / filename
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(self.doc, ensure_ascii=False, indent=1), encoding="utf-8")
        return p


class Seq:
    """有序动作链:每个动作节点依次 exec 相连;head=首节点(挂分支用),tail=末节点。"""
    def __init__(self, g):
        self.g = g
        self.head = None
        self.tail = None
    def add(self, nid):
        if self.head is None:
            self.head = nid
        if self.tail is not None:
            self.g.edge(self.tail, "exec", nid)
        self.tail = nid
        return nid
    def add_all(self, nids):
        for n in nids:
            self.add(n)
        return self.tail


# 便捷动作构造(全部返回节点 id;调用方负责经 Seq.add 入链)
def n_call(g, module, fn, args): return g.node("call.call_function", None, module=G.c(module), fn=G.c(fn), args=args)
def n_vget(g, name): return g.node("var.get", None, name=G.c(name))
def n_vset(g, name, value): return g.node("var.set", None, name=G.c(name), value=value)
def n_vadd(g, name, value): return g.node("var.add", None, name=G.c(name), value=value)
def n_tag(g, ent, tag): return g.node("entity.has_tag", None, entity=ent, tag=G.c(tag))
def n_find(g, tag): return g.node("entity.find_by_tag", None, tag=G.c(tag))
def n_gett(g, ent): return g.node("entity.get_transform", None, entity=ent)
def n_sett(g, ent, tr): return g.node("entity.set_transform", None, entity=ent, transform=tr)
def n_addtag(g, ent, t): return g.node("entity.add_tag", None, entity=ent, tag=G.c(t))
def n_tween(g, ent, off_, dur): return g.node("transform.move_tween", None, target=ent, offset=G.c(off_), duration=G.c(dur))
def n_msg(g, name, payload): return g.node("call.send_message", None, name=G.c(name), payload=payload)
def n_log(g, m): return g.node("debug.log", None, message=G.c(m))
def _ent(v):
    return G.c(v) if isinstance(v, str) else v
def n_anim_b(g, ent, param, v): return g.node("animator.set_bool", None, entity=_ent(ent), param=G.c(param), value=G.c(v))
def n_anim_t(g, ent, param): return g.node("animator.set_trigger", None, entity=_ent(ent), param=G.c(param))
def n_frame(g, ent, idx): return g.node("sprite.set_frame", None, entity=ent, index=idx)
def n_delay(g, d): return g.node("flow.delay", None, duration=G.c(d))
def n_br(g, cond_node, pin="result"): return g.node("flow.branch", None, condition=G.p(cond_node, pin))


def cmp_eq_const(g, xval_src, k):
    """x == k → (前导链节点列表, 分支节点)。xval_src: var 名(str)或 (node,pin)。"""
    t = g.node("var.set", None, name=G.c("cmpT"), value=G.c(0.0))
    if isinstance(xval_src, str):
        add = g.node("var.add", None, name=G.c("cmpT"), value=G.p(n_vget(g, xval_src), "out"))
    else:
        add = g.node("var.add", None, name=G.c("cmpT"), value=G.p(xval_src[0], xval_src[1]))
    sub = g.node("var.add", None, name=G.c("cmpT"), value=G.c(-float(k)))
    z = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out"))
    br = n_br(g, z)
    return [t, add, sub, z], br


def chain(g, nodes):
    """把节点列表依 exec 串起来,返回 (head, tail)。"""
    for a, b in zip(nodes, nodes[1:]):
        g.edge(a, "exec", b)
    return nodes[0], nodes[-1]


def cmp_pred(g, var, k, pred):
    """pred(var - k) → (前导链节点, 分支节点)。pred ∈ is_zero/nonpositive/nonnegative/gt_zero。"""
    t = n_vset(g, "cmpT", G.c(0.0))
    add = n_vadd(g, "cmpT", G.p(n_vget(g, var), "out"))
    sub = n_vadd(g, "cmpT", G.c(-float(k)))
    z = n_call(g, RULES, pred, G.p(n_vget(g, "cmpT"), "out"))
    br = n_br(g, z)
    chain(g, [t, add, sub, z, br])
    return t, br


def switch_const(g, var, cases, entry_seq=None):
    """按 var 的常量值分派:cases = [(k, leaf_head)],首分支挂 entry_seq 尾;返回最后一个分支的 else 悬空节点。"""
    prev = None
    for k, leaf in cases:
        pre, br = cmp_eq_const(g, var, float(k))
        chain(g, pre + [br])
        if prev is None:
            if entry_seq is not None:
                entry_seq.add(pre[0])
        else:
            g.edge(prev, "else", pre[0])
        if leaf is not None:
            g.edge(br, "then", leaf)
        prev = br
    return prev


# ============ 1. zombie_<id>.rxgraph ============
def gen_zombie(zombie_id=1, fname="zombie_walker.rxgraph"):
    g = G(pathlib.Path(fname).stem)
    g.prop("zombieId", float(zombie_id)); g.prop("poolId", 1.0)
    s = Seq(g)
    st = g.node("event.on_start"); s.add(st)
    s.add(n_call(g, DATA, "zombie_hp", G.r("zombieId")))
    s.add(n_vset(g, "hp", G.p(g.doc["nodes"][-1]["id"], "result")))
    for v in ("walking", "eating", "dead"):
        s.add(n_vset(g, v, G.c(0.0)))
    # on_update
    upd = g.node("event.on_update")
    su = Seq(g); su.add(upd)
    su.add(n_vset(g, "cmpT", G.c(0.0)))
    su.add(n_vadd(g, "cmpT", G.p(n_vget(g, "dead"), "out")))
    zdead = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); su.add(zdead)
    brdead = n_br(g, zdead); su.add(brdead)
    act = n_tag(g, G.c("$self"), "zombie_active")
    bract = n_br(g, act, "out"); g.edge(brdead, "then", bract)
    sa = Seq(g)
    sa.add(n_vset(g, "cmpT", G.c(0.0)))
    sa.add(n_vadd(g, "cmpT", G.p(n_vget(g, "walking"), "out")))
    sa.add(n_vadd(g, "cmpT", G.p(n_vget(g, "eating"), "out")))
    z0 = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); sa.add(z0)
    brinit = n_br(g, z0); sa.add(brinit)
    si = Seq(g); si.add(n_vset(g, "walking", G.c(1.0))); si.add(n_anim_b(g, "$self", "walking", True))
    g.edge(brinit, "then", si.head)
    se = Seq(g)
    se.add(n_vset(g, "cmpT", G.c(0.0)))
    se.add(n_vadd(g, "cmpT", G.p(n_vget(g, "eating"), "out")))
    ze = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); se.add(ze)
    breat = n_br(g, ze); se.add(breat)
    g.edge(brinit, "else", se.head)
    sw = Seq(g)
    spd = n_call(g, DATA, "zombie_speed_code", G.r("zombieId")); sw.add(spd)
    sw.add(n_vset(g, "cmpT", G.c(0.0)))
    sw.add(n_vadd(g, "cmpT", G.p(spd, "result")))
    sw.add(n_vadd(g, "cmpT", G.c(-3.0)))
    ge = n_call(g, RULES, "nonnegative", G.p(n_vget(g, "cmpT"), "out")); sw.add(ge)
    brspd = n_br(g, ge); sw.add(brspd)
    g.edge(brspd, "then", n_tween(g, G.c("$self"), [-0.068, 0.0, 0.0], 0.1))
    g.edge(brspd, "else", n_tween(g, G.c("$self"), [-0.034, 0.0, 0.0], 0.1))
    g.edge(breat, "then", sw.head)
    g.edge(bract, "then", sa.head)
    # on_trigger_enter(死亡序列进行中忽略一切接触:否则 delay 期间再挨豌豆会重复走死亡链,
    # 多发 zombie_died 把 zombiesAlive 打负 → 提前判胜)
    te = g.node("event.on_trigger_enter")
    st_ = Seq(g); st_.add(te)
    alive_head, br_alive = cmp_pred(g, "dead", 0.0, "is_zero")
    st_.add(alive_head)
    tpea = n_tag(g, G.p(te, "otherEntity"), "pea_active")
    brpea = n_br(g, tpea, "out"); g.edge(br_alive, "then", brpea)
    sh = Seq(g)
    sh.add(n_vadd(g, "hp", G.c(-20.0)))
    sh.add(n_sett(g, G.p(te, "otherEntity"), G.c(OFF_PEA)))
    sh.add(n_addtag(g, G.p(te, "otherEntity"), "pea_free"))
    hd = n_call(g, RULES, "hp_dead", G.p(n_vget(g, "hp"), "out")); sh.add(hd)
    brdie = n_br(g, hd); sh.add(brdie)
    g.edge(brpea, "then", sh.head)
    # 死亡序列(共享收敛点)
    sd = Seq(g)
    sd.add(n_vset(g, "dead", G.c(1.0))); sd.add(n_vset(g, "walking", G.c(0.0)))
    sd.add(n_anim_t(g, "$self", "die")); sd.add(n_delay(g, 1.0))
    sd.add(n_sett(g, G.c("$self"), G.c(OFF_ZOMBIE))); sd.add(n_addtag(g, G.c("$self"), f"zombie_free_{zombie_id}"))
    sd.add(n_msg(g, "zombie_died", G.c(-1000.0)))
    qreset = n_call(g, DATA, "zombie_hp", G.r("zombieId")); sd.add(qreset)
    sd.add(n_vset(g, "hp", G.p(qreset, "result")))
    sd.add(n_vset(g, "dead", G.c(0.0)))
    sd.add(n_vset(g, "walking", G.c(0.0)))
    sd.add(n_vset(g, "eating", G.c(0.0)))
    g.edge(brdie, "then", sd.head)
    tpl = n_tag(g, G.p(te, "otherEntity"), "plant_active")
    brpl = n_br(g, tpl, "out"); g.edge(brpea, "else", brpl)
    se2 = Seq(g); se2.add(n_vset(g, "eating", G.c(1.0))); se2.add(n_vset(g, "walking", G.c(0.0)))
    se2.add(n_anim_b(g, "$self", "eating", True))
    g.edge(brpl, "then", se2.head)
    tmo = n_tag(g, G.p(te, "otherEntity"), "mower_active")
    brmo = n_br(g, tmo, "out"); g.edge(brpl, "else", brmo)
    g.edge(brmo, "then", sd.head)
    ox = g.node("event.on_trigger_exit")
    sm = Seq(g); sm.add(ox)
    sm.add(n_vset(g, "cmpT", G.c(0.0)))
    sm.add(n_vadd(g, "cmpT", G.p(n_vget(g, "eating"), "out")))
    zm = n_call(g, RULES, "gt_zero", G.p(n_vget(g, "cmpT"), "out")); sm.add(zm)
    brm = n_br(g, zm); sm.add(brm)
    sr = Seq(g); sr.add(n_vset(g, "eating", G.c(0.0))); sr.add(n_vset(g, "walking", G.c(1.0)))
    sr.add(n_anim_b(g, "$self", "eating", False))
    g.edge(brm, "then", sr.head)
    return g.emit(fname)


# ============ 2. pea_fly.rxgraph ============
def gen_pea():
    g = G("pea_fly")
    g.prop("poolId", 1.0)
    upd = g.node("event.on_update")
    s = Seq(g); s.add(upd)
    act = n_tag(g, G.c("$self"), "pea_active")
    br = n_br(g, act, "out"); s.add(br)
    st = Seq(g)
    st.add(n_tween(g, G.c("$self"), [0.8, 0.0, 0.0], 0.1))
    st.add(n_vadd(g, "flyT", G.p(upd, "dt")))
    ex = n_call(g, RULES, "pea_expired", G.p(n_vget(g, "flyT"), "out")); st.add(ex)
    br2 = n_br(g, ex); st.add(br2)
    sr = Seq(g)
    sr.add(n_sett(g, G.c("$self"), G.c(OFF_PEA))); sr.add(n_addtag(g, G.c("$self"), "pea_free"))
    sr.add(n_vset(g, "flyT", G.c(0.0)))
    g.edge(br2, "then", sr.head)
    g.edge(br, "then", st.head)
    se = Seq(g); se.add(n_vset(g, "flyT", G.c(0.0)))
    g.edge(br, "else", se.head)
    return g.emit("pea_fly.rxgraph")


# ============ 3. sun_fall.rxgraph ============
# 两种激活态:sun_active = 天降(从 y=6.6 按池序号落到某一行中心,再停住);
#             sun_placed = 植物产出(原地不落)。两者都可被 CellProbe 收集(见 gen_probe)。
def gen_sun():
    g = G("sun_fall")
    g.prop("poolId", 1.0)
    upd = g.node("event.on_update")
    s = Seq(g); s.add(upd)
    act = n_tag(g, G.c("$self"), "sun_active")
    br = n_br(g, act, "out"); s.add(br)
    sa = Seq(g)
    sa.add(n_vadd(g, "liveT", G.p(upd, "dt")))
    # 剩余下落秒数 = sun_fall_secs(poolId) - fallT;>0 继续下落(1.6 单位/秒)。
    fs = n_call(g, RULES, "sun_fall_secs", G.r("poolId")); sa.add(fs)
    sa.add(n_vset(g, "cmpT", G.p(fs, "result")))
    ngf = n_call(g, RULES, "neg", G.p(n_vget(g, "fallT"), "out")); sa.add(ngf)
    sa.add(n_vadd(g, "cmpT", G.p(ngf, "result")))
    landed = n_call(g, RULES, "sun_landed", G.p(n_vget(g, "cmpT"), "out")); sa.add(landed)
    brf = n_br(g, landed); sa.add(brf)
    sf = Seq(g); sf.add(n_vadd(g, "fallT", G.p(upd, "dt"))); sf.add(n_tween(g, G.c("$self"), [0.0, -0.16, 0.0], 0.1))
    g.edge(brf, "else", sf.head)
    ex = n_call(g, RULES, "sun_expired", G.p(n_vget(g, "liveT"), "out"))
    sa2 = Seq(g); sa2.add(ex)
    bre = n_br(g, ex); sa2.add(bre)
    src = Seq(g)
    src.add(n_sett(g, G.c("$self"), G.c(OFF_SUN))); src.add(n_addtag(g, G.c("$self"), "sun_free"))
    src.add(n_vset(g, "liveT", G.c(0.0))); src.add(n_vset(g, "fallT", G.c(0.0)))
    g.edge(bre, "then", src.head)
    g.edge(brf, "then", sa2.head); g.edge(sf.tail, "exec", sa2.head)
    g.edge(br, "then", sa.head)
    # 非天降:sun_placed 只计寿命;都不是 → 复位计时。
    placed = n_tag(g, G.c("$self"), "sun_placed")
    brp = n_br(g, placed, "out"); g.edge(br, "else", brp)
    sp = Seq(g); sp.add(n_vadd(g, "liveT", G.p(upd, "dt")))
    ex2 = n_call(g, RULES, "sun_expired", G.p(n_vget(g, "liveT"), "out")); sp.add(ex2)
    bre2 = n_br(g, ex2); sp.add(bre2)
    g.edge(bre2, "then", src.head)
    g.edge(brp, "then", sp.head)
    sn = Seq(g); sn.add(n_vset(g, "liveT", G.c(0.0))); sn.add(n_vset(g, "fallT", G.c(0.0)))
    g.edge(brp, "else", sn.head)
    # 兼容旧 CLI 协议:collect_sun(action=3)一键收全部。
    oi = g.node("event.on_input")
    sm = Seq(g); sm.add(oi)
    ua = n_call(g, RULES, "unpack_action", G.p(oi, "value")); sm.add(ua)
    sm.add(n_vset(g, "cmpT", G.c(0.0)))
    sm.add(n_vadd(g, "cmpT", G.p(ua, "result")))
    sm.add(n_vadd(g, "cmpT", G.c(-3.0)))
    eq = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); sm.add(eq)
    bra = n_br(g, eq); sm.add(bra)
    act2 = n_tag(g, G.c("$self"), "sun_active")
    brm = n_br(g, act2, "out"); g.edge(bra, "then", brm)
    sc = Seq(g)
    sc.add(n_sett(g, G.c("$self"), G.c(OFF_SUN))); sc.add(n_addtag(g, G.c("$self"), "sun_free"))
    sc.add(n_vset(g, "liveT", G.c(0.0))); sc.add(n_vset(g, "fallT", G.c(0.0)))
    sc.add(n_msg(g, "sun_gained", G.c(25.0)))
    g.edge(brm, "then", sc.head)
    act3 = n_tag(g, G.c("$self"), "sun_placed")
    brm2 = n_br(g, act3, "out"); g.edge(brm, "else", brm2)
    g.edge(brm2, "then", sc.head)
    return g.emit("sun_fall.rxgraph")


# ============ 3b. cell_probe.rxgraph ============
# 格子探针:控制器把它挪到被点击的格子;探针 Trigger 盒覆盖整格,进入盒内的阳光即被收集(+25)。
# 同时它的 transform 就是「当前点击格心」,植物落位直接读它(免去每植物×每格的查表树)。
def gen_probe():
    g = G("cell_probe")
    te = g.node("event.on_trigger_enter")
    s = Seq(g); s.add(te)
    other = G.p(te, "otherEntity")
    is_sky = n_tag(g, other, "sun_active")
    br = n_br(g, is_sky, "out"); s.add(br)
    collect = Seq(g)
    collect.add(n_sett(g, other, G.c(OFF_SUN)))
    collect.add(n_addtag(g, other, "sun_free"))
    collect.add(n_msg(g, "sun_gained", G.c(25.0)))
    g.edge(br, "then", collect.head)
    is_placed = n_tag(g, other, "sun_placed")
    br2 = n_br(g, is_placed, "out"); g.edge(br, "else", br2)
    g.edge(br2, "then", collect.head)
    return g.emit("cell_probe.rxgraph")


# ============ 4. plant_<id>.rxgraph(按适配器共享主体) ============
# 通用:hp/被啃/死亡回池、放到已占格被弹回并退款;shooter 发豌豆,producer 产阳光,
# explosive 接触僵尸即炸(自身随之回池),barrier/special 纯挡路。
def gen_plant(plant_id: int, adapter: str, fname: str, cost: float):
    g = G(pathlib.Path(fname).stem)
    g.prop("plantId", float(plant_id))
    free_tag = f"plant_free_{plant_id}"
    # ---- on_start ----
    st = g.node("event.on_start")
    s = Seq(g); s.add(st)
    hp0 = n_call(g, DATA, "plant_hp", G.r("plantId")); s.add(hp0)
    s.add(n_vset(g, "hp", G.p(hp0, "result")))
    if adapter == "shooter":
        iq = n_call(g, DATA, "plant_attack_interval", G.r("plantId")); s.add(iq)
        s.add(n_vset(g, "interval", G.p(iq, "result"))); s.add(n_vset(g, "remain", G.p(iq, "result")))
    elif adapter == "producer":
        iq = n_call(g, DATA, "plant_production_interval", G.r("plantId")); s.add(iq)
        s.add(n_vset(g, "interval", G.p(iq, "result"))); s.add(n_vset(g, "remain", G.p(iq, "result")))
    s.add(n_vset(g, "eaten", G.c(0.0))); s.add(n_vset(g, "activeT", G.c(0.0)))

    # ---- 回池(共享收敛点):OFF + free 标签 + 黑板复位 ----
    recycle = Seq(g)
    recycle.add(n_sett(g, G.c("$self"), G.c(OFF_PLANT)))
    recycle.add(n_addtag(g, G.c("$self"), free_tag))
    hpr = n_call(g, DATA, "plant_hp", G.r("plantId")); recycle.add(hpr)
    recycle.add(n_vset(g, "hp", G.p(hpr, "result")))
    recycle.add(n_vset(g, "eaten", G.c(0.0))); recycle.add(n_vset(g, "activeT", G.c(0.0)))
    if adapter in ("shooter", "producer"):
        recycle.add(n_vset(g, "remain", G.p(n_vget(g, "interval"), "out")))

    # ---- on_update ----
    upd = g.node("event.on_update")
    su = Seq(g); su.add(upd)
    act = n_tag(g, G.c("$self"), "plant_active")
    br = n_br(g, act, "out"); su.add(br)
    sa = Seq(g)
    sa.add(n_vadd(g, "activeT", G.p(upd, "dt")))
    # 被啃:hp += bite_damage(dt);hp_dead → 回池
    eat_head, br_eat = cmp_pred(g, "eaten", 0.0, "gt_zero")
    sa.add(eat_head)
    bite = Seq(g)
    bd = n_call(g, RULES, "bite_damage", G.p(upd, "dt")); bite.add(bd)
    bite.add(n_vadd(g, "hp", G.p(bd, "result")))
    hd = n_call(g, RULES, "hp_dead", G.p(n_vget(g, "hp"), "out")); bite.add(hd)
    br_dead = n_br(g, hd); bite.add(br_dead)
    g.edge(br_eat, "then", bite.head)
    g.edge(br_dead, "then", recycle.head)
    # 行为
    behave = Seq(g)
    if adapter in ("shooter", "producer"):
        ng = n_call(g, RULES, "neg", G.p(upd, "dt")); behave.add(ng)
        behave.add(n_vadd(g, "remain", G.p(ng, "result")))
        np_ = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "remain"), "out")); behave.add(np_)
        br2 = n_br(g, np_); behave.add(br2)
        fire = Seq(g)
        fire.add(n_vset(g, "remain", G.p(n_vget(g, "interval"), "out")))
        gt = n_gett(g, G.c("$self"))
        if adapter == "shooter":
            pea = n_find(g, "pea_free")
            fire.add(n_sett(g, G.p(pea, "entity"), G.p(gt, "transform")))
            fire.add(n_addtag(g, G.p(pea, "entity"), "pea_active"))
            fire.add(n_anim_t(g, "$self", "attack"))
        else:
            sun = n_find(g, "sun_free")
            fire.add(n_sett(g, G.p(sun, "entity"), G.p(gt, "transform")))
            fire.add(n_addtag(g, G.p(sun, "entity"), "sun_placed"))
            fire.add(n_anim_t(g, "$self", "produce"))
        g.edge(br2, "then", fire.head)
    else:
        behave.add(n_vset(g, "cmpT", G.c(0.0)))  # 占位:纯挡路植物无每帧行为
    g.edge(br_eat, "else", behave.head)
    g.edge(br_dead, "else", behave.head)
    g.edge(br, "then", sa.head)
    # 未激活:黑板复位(池中待命)
    idle = Seq(g)
    idle.add(n_vset(g, "eaten", G.c(0.0))); idle.add(n_vset(g, "activeT", G.c(0.0)))
    if adapter in ("shooter", "producer"):
        idle.add(n_vset(g, "remain", G.p(n_vget(g, "interval"), "out")))
    g.edge(br, "else", idle.head)

    # ---- on_trigger_enter ----
    te = g.node("event.on_trigger_enter")
    st_ = Seq(g); st_.add(te)
    other = G.p(te, "otherEntity")
    selfact = n_tag(g, G.c("$self"), "plant_active")
    br_self = n_br(g, selfact, "out"); st_.add(br_self)
    is_z = n_tag(g, other, "zombie_active")
    br_z = n_br(g, is_z, "out"); g.edge(br_self, "then", br_z)
    if adapter == "explosive":
        boom = Seq(g)
        boom.add(n_sett(g, other, G.c(OFF_ZOMBIE)))
        boom.add(n_addtag(g, other, "zombie_spent"))
        boom.add(n_msg(g, "zombie_died", G.c(-1000.0)))
        boom.add(n_anim_t(g, "$self", "special"))
        g.edge(br_z, "then", boom.head)
        g.edge(boom.tail, "exec", recycle.head)
    else:
        g.edge(br_z, "then", n_vset(g, "eaten", G.c(1.0)))
    # 落到已占格:对方是活植物且自己刚落位(activeT < 0.3)→ 弹回 + 退款
    is_p = n_tag(g, other, "plant_active")
    br_p = n_br(g, is_p, "out"); g.edge(br_z, "else", br_p)
    fresh_head, br_fresh = cmp_pred(g, "activeT", 0.3, "nonpositive")
    g.edge(br_p, "then", fresh_head)
    bounce = Seq(g)
    bounce.add(n_msg(g, "plant_refund", G.c(500000.0 + cost)))
    g.edge(br_fresh, "then", bounce.head)
    g.edge(bounce.tail, "exec", recycle.head)
    # ---- on_trigger_exit:啃食者离开/死亡 ----
    ox = g.node("event.on_trigger_exit")
    sx = Seq(g); sx.add(ox); sx.add(n_vset(g, "eaten", G.c(0.0)))
    return g.emit(fname)


# ============ 6. loseline.rxgraph ============
def gen_loseline():
    g = G("loseline")
    g.prop("row", 3.0)
    te = g.node("event.on_trigger_enter")
    s = Seq(g); s.add(te)
    z = n_tag(g, G.p(te, "otherEntity"), "zombie_active")
    br = n_br(g, z, "out"); s.add(br)
    enc = n_call(g, RULES, "msg_loseline", G.r("row")); sm = Seq(g); sm.add(enc)
    sm.add(n_msg(g, "loseline_hit_row", G.p(enc, "result")))
    g.edge(br, "then", sm.head)
    return g.emit("loseline.rxgraph")


# ============ 6b. mower.rxgraph ============
def gen_mower():
    g = G("mower")
    g.prop("row", 3.0)
    st = g.node("event.on_start")
    s0 = Seq(g); s0.add(st)
    s0.add(n_vset(g, "used", G.c(0.0)))
    te = g.node("event.on_trigger_enter")
    s = Seq(g); s.add(te)
    z = n_tag(g, G.p(te, "otherEntity"), "zombie_active")
    br = n_br(g, z, "out"); s.add(br)
    sa = Seq(g)
    sa.add(n_vset(g, "cmpT", G.c(0.0)))
    sa.add(n_vadd(g, "cmpT", G.p(n_vget(g, "used"), "out")))
    zu = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); sa.add(zu)
    brused = n_br(g, zu); sa.add(brused)
    g.edge(br, "then", sa.head)
    sact = Seq(g)
    sact.add(n_vset(g, "used", G.c(1.0)))
    sact.add(n_addtag(g, G.c("$self"), "mower_active"))
    sact.add(n_tween(g, G.c("$self"), [22.0, 0.0, 0.0], 2.2))
    g.edge(brused, "then", sact.head)
    return g.emit("mower.rxgraph")


# ============ 7. level_<id>.rxgraph(波次表生成期烘焙 + 指针交互) ============
def gen_controller(rows=(3,), cols=range(1, 10), plant_ids=(1, 2), level_code=101,
                   fname="level_controller.rxgraph", sky_sun=True, six_rows=False, all_rows=None):
    """rows = 可出怪/可种植的行;all_rows = 点击可命中的全部行(收阳光对所有行开放)。"""
    all_rows = list(all_rows) if all_rows else list(rows)
    levels = json.loads((ROOT / "docs" / "data" / "levels.json").read_text(encoding="utf-8"))
    zids = {z["id"]: i + 1 for i, z in enumerate(json.loads((ROOT / "docs" / "data" / "zombies.json").read_text(encoding="utf-8")))}
    lid = f"{level_code // 100}-{level_code % 100}"
    lvl = next(l for l in levels if l["id"] == lid)
    total_waves = int(lvl["waves"]); pool = [zids[z] for z in lvl["zombie_ids"]]
    schedule = []
    for w in range(1, total_waves + 1):
        size = min(3, 1 + w // 4)
        schedule.append([pool[(w + m * 3) % len(pool)] for m in range(size)])
    g = G(pathlib.Path(fname).stem)
    g.prop("levelCode", float(level_code))
    cols = list(cols)
    row_fn = "click_row6" if six_rows else "click_row5"

    # ---------- on_start ----------
    st = g.node("event.on_start")
    s0 = Seq(g); s0.add(st)
    q = n_call(g, DATA, "level_starting_sun", G.r("levelCode")); s0.add(q)
    s0.add(n_vset(g, "sun", G.p(q, "result")))
    s0.add(n_vset(g, "totalWaves", G.c(float(total_waves))))
    qs = n_call(g, RULES, "level_stage", G.r("levelCode")); s0.add(qs)
    s0.add(n_vset(g, "stage", G.p(qs, "result")))
    for name, val in [("phase", 1.0), ("waveIndex", 0.0), ("zombiesAlive", 0.0),
                      ("skyRemain", 3.0 if sky_sun else 1.0e9),
                      ("waveRemain", 18.0), ("memberRemain", 0.0), ("waveLeft", 0.0), ("skyIdx", 0.0),
                      ("selectedPlant", 0.0), ("click", 0.0), ("click_x", 0.0), ("click_y", 0.0)]:
        s0.add(n_vset(g, name, G.c(val)))
    hs0 = n_call(g, RULES, "msg_hud_sun", G.p(n_vget(g, "sun"), "out")); s0.add(hs0)
    s0.add(n_msg(g, "hud_set_sun", G.p(hs0, "result")))

    # ---------- on_update ----------
    upd = g.node("event.on_update")
    su = Seq(g); su.add(upd)
    su.add(n_vset(g, "cmpT", G.c(0.0)))
    su.add(n_vadd(g, "cmpT", G.p(n_vget(g, "phase"), "out")))
    su.add(n_vadd(g, "cmpT", G.c(-1.0)))
    zp = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); su.add(zp)
    br_phase = n_br(g, zp); su.add(br_phase)
    seq_ab = g.node("flow.sequence")
    seq_cd = g.node("flow.sequence")
    g.edge(br_phase, "then", seq_ab)
    # —— 天降阳光 ——
    sk = Seq(g)
    if sky_sun:
        nk = n_call(g, RULES, "neg", G.p(upd, "dt")); sk.add(nk)
        sk.add(n_vadd(g, "skyRemain", G.p(nk, "result")))
        np1 = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "skyRemain"), "out")); sk.add(np1)
        br_sky = n_br(g, np1); sk.add(br_sky)
        ss = Seq(g)
        qiv = n_call(g, DATA, "stage_sky_sun_interval", G.p(n_vget(g, "stage"), "out")); ss.add(qiv)
        ss.add(n_vset(g, "skyRemain", G.p(qiv, "result")))
        ss.add(n_vadd(g, "skyIdx", G.c(1.0)))
        md = n_call(g, RULES, "mod8", G.p(n_vget(g, "skyIdx"), "out")); ss.add(md)
        ss.add(n_vset(g, "skySlot", G.p(md, "result")))
        cases = []
        for i in range(8):
            f = n_find(g, "sun_free")
            pos = n_sett(g, G.p(f, "entity"), G.c(off(-6.4 + i * 1.6, SKY_SUN_Y)))
            tg = n_addtag(g, G.p(f, "entity"), "sun_active")
            g.edge(pos, "exec", tg)
            cases.append((i, pos))
        switch_const(g, "skySlot", cases, ss)
        g.edge(br_sky, "then", ss.head)
    else:
        sk.add(n_vset(g, "cmpT", G.c(0.0)))
    # —— 波次 ——
    wv = Seq(g)
    nw = n_call(g, RULES, "neg", G.p(upd, "dt")); wv.add(nw)
    wv.add(n_vadd(g, "waveRemain", G.p(nw, "result")))
    np2 = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "waveRemain"), "out")); wv.add(np2)
    br_wv = n_br(g, np2); wv.add(br_wv)
    wo = Seq(g)
    wo.add(n_vset(g, "cmpT", G.c(0.0)))
    wo.add(n_vadd(g, "cmpT", G.p(n_vget(g, "waveIndex"), "out")))
    ngt = n_call(g, RULES, "neg", G.p(n_vget(g, "totalWaves"), "out")); wo.add(ngt)
    wo.add(n_vadd(g, "cmpT", G.p(ngt, "result")))
    wo.add(n_vadd(g, "cmpT", G.c(1.0)))
    zmore = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "cmpT"), "out")); wo.add(zmore)
    br_more = n_br(g, zmore); wo.add(br_more)
    wopen = Seq(g)
    wopen.add(n_vadd(g, "waveIndex", G.c(1.0)))
    wave_size_tail = []
    cases = []
    for w in range(1, total_waves + 1):
        swl = n_vset(g, "waveLeft", G.c(float(len(schedule[w - 1]))))
        wave_size_tail.append(swl)
        cases.append((w, swl))
    last_br = switch_const(g, "waveIndex", cases, wopen)
    qiv2 = n_call(g, RULES, "wave_interval", G.p(n_vget(g, "waveIndex"), "out"))
    rst2 = n_vset(g, "waveRemain", G.p(qiv2, "result"))
    for t in wave_size_tail:
        g.edge(t, "exec", qiv2)
    g.edge(last_br, "else", qiv2)
    g.edge(qiv2, "exec", rst2)
    g.edge(br_more, "then", wopen.head)
    g.edge(br_wv, "then", wo.head)
    # —— 成员 ——
    mb = Seq(g)
    nm = n_call(g, RULES, "neg", G.p(upd, "dt")); mb.add(nm)
    mb.add(n_vadd(g, "memberRemain", G.p(nm, "result")))
    np3 = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "memberRemain"), "out")); mb.add(np3)
    br_m = n_br(g, np3); mb.add(br_m)
    mw = Seq(g)
    gwl = n_call(g, RULES, "gt_zero", G.p(n_vget(g, "waveLeft"), "out")); mw.add(gwl)
    br_wl = n_br(g, gwl); mw.add(br_wl)
    prevw2 = None
    for w in range(1, total_waves + 1):
        members = schedule[w - 1]
        pre, brw = cmp_eq_const(g, "waveIndex", float(w))
        chain(g, pre + [brw])
        if prevw2 is None:
            g.edge(br_wl, "then", pre[0])
        else:
            g.edge(prevw2, "else", pre[0])
        sq = Seq(g)
        sq.add(n_vset(g, "seqT", G.c(float(len(members)))))
        ngl = n_call(g, RULES, "neg", G.p(n_vget(g, "waveLeft"), "out")); sq.add(ngl)
        sq.add(n_vadd(g, "seqT", G.p(ngl, "result")))
        g.edge(brw, "then", sq.head)
        prevq = None
        for qi, zid in enumerate(members):
            pre2, brq = cmp_eq_const(g, "seqT", float(qi))
            chain(g, pre2 + [brq])
            if prevq is None:
                g.edge(sq.tail, "exec", pre2[0])
            else:
                g.edge(prevq, "else", pre2[0])
            spawn_row = rows[(w + qi - 1) % len(rows)]
            spawn_y = (6.0 - spawn_row + 0.5) * 1.6 - 4.8 if six_rows else (5.0 - spawn_row + 0.5) * 1.6 - 4.0
            fz = n_find(g, f"zombie_free_{zid}")
            available = n_tag(g, G.p(fz, "entity"), f"zombie_free_{zid}")
            br_available = n_br(g, available, "out")
            ent = n_sett(g, G.p(fz, "entity"), G.c(off(9.5, spawn_y)))
            tg = n_addtag(g, G.p(fz, "entity"), "zombie_active")
            sp = Seq(g)
            sp.add(ent); sp.add(tg); sp.add(n_vadd(g, "zombiesAlive", G.c(1.0)))
            sp.add(n_vadd(g, "waveLeft", G.c(-1.0))); sp.add(n_vset(g, "memberRemain", G.c(0.6)))
            retry = n_vset(g, "memberRemain", G.c(0.75))
            g.edge(brq, "then", br_available)
            g.edge(br_available, "then", sp.head)
            g.edge(br_available, "else", retry)
            prevq = brq
        prevw2 = brw
    g.edge(br_m, "then", mw.head)
    g.edge(seq_ab, "seq0", sk.head)
    g.edge(seq_ab, "seq1", seq_cd)
    g.edge(seq_cd, "seq0", wv.head)
    g.edge(seq_cd, "seq1", mb.head)

    # ---------- on_message ----------
    om = g.node("event.on_message")
    sm = Seq(g); sm.add(om)
    sm.add(n_vset(g, "msgPayload", G.p(om, "payload")))
    sm.add(n_vset(g, "msgT", G.p(om, "payload")))
    sm.add(n_vadd(g, "msgT", G.c(1000.0)))
    mnp = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "msgT"), "out")); sm.add(mnp)
    br_died = n_br(g, mnp); sm.add(br_died)
    sd = Seq(g)
    sd.add(n_vadd(g, "zombiesAlive", G.c(-1.0)))
    az = n_call(g, RULES, "nonpositive", G.p(n_vget(g, "zombiesAlive"), "out")); sd.add(az)
    br_az = n_br(g, az); sd.add(br_az)
    sw2 = Seq(g)
    sw2.add(n_vset(g, "cmpT", G.c(0.0)))
    sw2.add(n_vadd(g, "cmpT", G.p(n_vget(g, "waveIndex"), "out")))
    ngt2 = n_call(g, RULES, "neg", G.p(n_vget(g, "totalWaves"), "out")); sw2.add(ngt2)
    sw2.add(n_vadd(g, "cmpT", G.p(ngt2, "result")))
    zw = n_call(g, RULES, "nonnegative", G.p(n_vget(g, "cmpT"), "out")); sw2.add(zw)
    br_wv2 = n_br(g, zw); sw2.add(br_wv2)
    swin = Seq(g)
    swin.add(n_vset(g, "phase", G.c(2.0)))
    rw = n_call(g, DATA, "level_reward_code", G.r("levelCode")); swin.add(rw)
    win_enc = n_call(g, RULES, "msg_level_win", G.p(rw, "result")); swin.add(win_enc)
    swin.add(n_msg(g, "level_win", G.p(win_enc, "result")))
    swin.add(n_log(g, "LEVEL_WIN"))
    g.edge(br_wv2, "then", swin.head)
    g.edge(br_az, "then", sw2.head)
    g.edge(br_died, "then", sd.head)
    # sun_gained(payload 25)
    sunseq = Seq(g)
    sunseq.add(n_vset(g, "msgSunT", G.p(om, "payload")))
    sunseq.add(n_vadd(g, "msgSunT", G.c(-25.0)))
    sun_eq = n_call(g, RULES, "is_zero", G.p(n_vget(g, "msgSunT"), "out")); sunseq.add(sun_eq)
    br_sun = n_br(g, sun_eq); sunseq.add(br_sun)
    g.edge(br_died, "else", sunseq.head)
    gain = Seq(g)
    gain.add(n_vadd(g, "sun", G.c(25.0)))
    gain_enc = n_call(g, RULES, "msg_hud_sun", G.p(n_vget(g, "sun"), "out")); gain.add(gain_enc)
    gain.add(n_msg(g, "hud_set_sun", G.p(gain_enc, "result")))
    g.edge(br_sun, "then", gain.head)
    # kind 分派:2 判负线,5 退款
    lk = Seq(g)
    kind = n_call(g, RULES, "msg_kind", G.p(om, "payload")); lk.add(kind)
    lk.add(n_vset(g, "msgKind", G.p(kind, "result")))
    kind_pre, kind_br = cmp_eq_const(g, "msgKind", 2.0)
    lk.add_all(kind_pre); lk.add(kind_br)
    g.edge(br_sun, "else", lk.head)
    row_decode = Seq(g)
    row_val = n_call(g, RULES, "msg_value", G.p(om, "payload")); row_decode.add(row_val)
    row_decode.add(n_vset(g, "msgRow", G.p(row_val, "result")))
    g.edge(kind_br, "then", row_decode.head)
    prev_row = None
    for r in rows:
        rpre, rbr = cmp_eq_const(g, "msgRow", float(r))
        chain(g, rpre + [rbr])
        if prev_row is None:
            g.edge(row_decode.tail, "exec", rpre[0])
        else:
            g.edge(prev_row, "else", rpre[0])
        phase_guard = Seq(g)
        phase_guard.add(n_vset(g, "cmpT", G.c(0.0)))
        phase_guard.add(n_vadd(g, "cmpT", G.p(n_vget(g, "phase"), "out")))
        phase_guard.add(n_vadd(g, "cmpT", G.c(-1.0)))
        zph = n_call(g, RULES, "is_zero", G.p(n_vget(g, "cmpT"), "out")); phase_guard.add(zph)
        brph = n_br(g, zph); phase_guard.add(brph)
        over = Seq(g)
        over.add(n_vset(g, "phase", G.c(9.0)))
        go_enc = n_call(g, RULES, "msg_game_over", G.c([float(r)])); over.add(go_enc)
        over.add(n_msg(g, "game_over", G.p(go_enc, "result")))
        over.add(n_log(g, "LEVEL_LOSE"))
        g.edge(rbr, "then", phase_guard.head)
        g.edge(brph, "then", over.head)
        prev_row = rbr
    # kind 5:退款(种到已占格被弹回)
    ref_pre, ref_br = cmp_eq_const(g, "msgKind", 5.0)
    chain(g, ref_pre + [ref_br])
    g.edge(kind_br, "else", ref_pre[0])
    refund = Seq(g)
    ref_val = n_call(g, RULES, "msg_value", G.p(om, "payload")); refund.add(ref_val)
    refund.add(n_vadd(g, "sun", G.p(ref_val, "result")))
    ref_enc = n_call(g, RULES, "msg_hud_sun", G.p(n_vget(g, "sun"), "out")); refund.add(ref_enc)
    refund.add(n_msg(g, "hud_set_sun", G.p(ref_enc, "result")))
    g.edge(ref_br, "then", refund.head)

    # ---------- on_input ----------
    oi = g.node("event.on_input")
    si = Seq(g); si.add(oi)
    # 通用捕获:click 标记先清零,再把输入值存进「动作同名变量」(click_x/click_y/… 自动落位;
    # click 事件本身把 click 置 1)。
    si.add(n_vset(g, "click", G.c(0.0)))
    si.add(g.node("var.set", None, name=G.p(oi, "action"), value=G.p(oi, "value")))
    si.add(n_vset(g, "inV", G.p(oi, "value")))
    ua = n_call(g, RULES, "unpack_action", G.p(n_vget(g, "inV"), "out")); si.add(ua)
    si.add(n_vset(g, "inAction", G.p(ua, "result")))
    up = n_call(g, RULES, "unpack_payload", G.p(n_vget(g, "inV"), "out")); si.add(up)
    si.add(n_vset(g, "inPay", G.p(up, "result")))
    # 旧协议 action==1 选卡(payload=植物 id)
    pre, br_a1 = cmp_eq_const(g, "inAction", 1.0)
    si.add_all(pre); si.add(br_a1)
    sc1 = Seq(g); sc1.add(n_vset(g, "selectedPlant", G.p(n_vget(g, "inPay"), "out"))); sc1.add(n_log(g, "CARD_SELECTED"))
    g.edge(br_a1, "then", sc1.head)
    # 旧协议 action==2 plant_at(payload=植物*10000+行*100+列)→ 汇入共享落位链
    pre2, br_a2 = cmp_eq_const(g, "inAction", 2.0)
    chain(g, pre2 + [br_a2]); g.edge(br_a1, "else", pre2[0])
    legacy = Seq(g)
    upn = n_call(g, RULES, "unpack_plant", G.p(n_vget(g, "inPay"), "out")); legacy.add(upn)
    legacy.add(n_vset(g, "selectedPlant", G.p(upn, "result")))
    ucn = n_call(g, RULES, "unpack_cellcode", G.p(n_vget(g, "inPay"), "out")); legacy.add(ucn)
    legacy.add(n_vset(g, "curCell", G.p(ucn, "result")))
    urn = n_call(g, RULES, "unpack_row", G.p(n_vget(g, "curCell"), "out")); legacy.add(urn)
    legacy.add(n_vset(g, "curRow", G.p(urn, "result")))
    ucl = n_call(g, RULES, "unpack_col", G.p(n_vget(g, "curCell"), "out")); legacy.add(ucl)
    legacy.add(n_vset(g, "curCol", G.p(ucl, "result")))
    g.edge(br_a2, "then", legacy.head)
    # 旧协议 action==3 收阳光:由 sun_fall 实例自行处理
    pre3, br_a3 = cmp_eq_const(g, "inAction", 3.0)
    chain(g, pre3 + [br_a3]); g.edge(br_a2, "else", pre3[0])
    g.edge(br_a3, "then", n_log(g, "SUN_COLLECT_INPUT"))
    # 指针点击:click 变量为 1(其余动作名的输入落到别的变量,click 保持 0)
    clk = n_call(g, RULES, "gt_zero", G.p(n_vget(g, "click"), "out"))
    br_clk = n_br(g, clk)
    chain(g, [clk, br_clk]); g.edge(br_a3, "else", clk)
    # —— 卡栏?——
    bar = n_call(g, RULES, "in_card_bar", G.p(n_vget(g, "click_y"), "out"))
    br_bar = n_br(g, bar)
    chain(g, [bar, br_bar]); g.edge(br_clk, "then", bar)
    slot = n_call(g, RULES, "card_slot", G.p(n_vget(g, "click_x"), "out"))
    sslot = n_vset(g, "cardSlot", G.p(slot, "result"))
    chain(g, [slot, sslot]); g.edge(br_bar, "then", slot)
    cursor = n_find(g, "card_cursor")
    slot_cases = []
    for i, pid in enumerate(plant_ids):
        pick = Seq(g)
        pick.add(n_vset(g, "selectedPlant", G.c(float(pid))))
        card = n_find(g, f"card_slot_{i}")
        pick.add(n_sett(g, G.p(cursor, "entity"), G.p(n_gett(g, G.p(card, "entity")), "transform")))
        pick.add(n_log(g, "CARD_SELECTED"))
        slot_cases.append((i, pick.head))
    slot_seq = Seq(g); slot_seq.tail = sslot; slot_seq.head = slot
    switch_const(g, "cardSlot", slot_cases, slot_seq)
    # —— 草坪格?——
    rowc = n_call(g, RULES, row_fn, G.p(n_vget(g, "click_y"), "out"))
    srow = n_vset(g, "curRow", G.p(rowc, "result"))
    colc = n_call(g, RULES, "click_col", G.p(n_vget(g, "click_x"), "out"))
    scol = n_vset(g, "curCol", G.p(colc, "result"))
    chain(g, [rowc, srow, colc, scol]); g.edge(br_bar, "else", rowc)

    # ---------- 共享落位链:curRow/curCol → 探针挪到该格 → (selectedPlant>0 且付得起) 放植物 ----------
    # 入口:旧协议 legacy.tail 与指针 scol 都汇到 row 分派树。
    probe = n_find(g, "cell_probe")
    row_cases = []
    place_heads = []  # 可种植行的格叶子尾部 → 汇入放植物链;不可种植行只挪探针(收阳光)
    for r in all_rows:
        col_cases = []
        for c in cols:
            cell = n_find(g, f"cell_r{r}c{c}")
            mv = n_sett(g, G.p(probe, "entity"), G.p(n_gett(g, G.p(cell, "entity")), "transform"))
            if r in rows:
                place_heads.append(mv)
            col_cases.append((c, mv))
        # 该行的列分派树:首节点由 row 分支 then 进入
        first_pre = None
        prev = None
        for k, leaf in col_cases:
            pre_c, br_c = cmp_eq_const(g, "curCol", float(k))
            chain(g, pre_c + [br_c])
            if prev is None:
                first_pre = pre_c[0]
            else:
                g.edge(prev, "else", pre_c[0])
            g.edge(br_c, "then", leaf)
            prev = br_c
        row_cases.append((r, first_pre))
    entry = Seq(g); entry.head = scol; entry.tail = scol
    switch_const(g, "curRow", row_cases, entry)
    # 旧协议入口接到同一棵树的首节点(= scol 的 exec 出边目标)。
    tree_head = next(e["to"][0] for e in g.doc["edges"] if e["from"] == [scol, "exec"])
    g.edge(legacy.tail, "exec", tree_head)
    # 放植物:selectedPlant>0 → 付费校验 → 按植物 id 取池实体挪到探针处
    sp_pre, br_sel = cmp_pred(g, "selectedPlant", 0.0, "gt_zero")
    for mv in place_heads:
        g.edge(mv, "exec", sp_pre)
    pay = Seq(g)
    pay.add(n_vset(g, "afP", G.c(0.0)))
    pay.add(n_vadd(g, "afP", G.p(n_vget(g, "sun"), "out")))
    qc = n_call(g, DATA, "plant_cost", G.p(n_vget(g, "selectedPlant"), "out")); pay.add(qc)
    nc = n_call(g, RULES, "neg", G.p(qc, "result")); pay.add(nc)
    pay.add(n_vadd(g, "afP", G.p(nc, "result")))
    af = n_call(g, RULES, "afford_packed", G.p(n_vget(g, "afP"), "out")); pay.add(af)
    br_af = n_br(g, af); pay.add(br_af)
    g.edge(br_sel, "then", pay.head)
    place_cases = []
    for pid in plant_ids:
        fp = n_find(g, f"plant_free_{pid}")
        avail = n_tag(g, G.p(fp, "entity"), f"plant_free_{pid}")
        br_av = n_br(g, avail, "out")
        put = Seq(g)
        put.add(n_sett(g, G.p(fp, "entity"), G.p(n_gett(g, G.p(probe, "entity")), "transform")))
        put.add(n_addtag(g, G.p(fp, "entity"), "plant_active"))
        put.add(n_vadd(g, "sun", G.p(nc, "result")))
        hsc = n_call(g, RULES, "msg_hud_sun", G.p(n_vget(g, "sun"), "out")); put.add(hsc)
        put.add(n_msg(g, "hud_set_sun", G.p(hsc, "result")))
        put.add(n_vset(g, "selectedPlant", G.c(0.0)))
        put.add(n_sett(g, G.p(cursor, "entity"), G.c(OFF_UI)))
        put.add(n_log(g, "PLANT_PLACED"))
        g.edge(br_av, "then", put.head)
        place_cases.append((pid, br_av))
    # br_af.then 进入植物分派树(switch_const 的 entry 语义是接 exec 出边,这里手接 then)。
    prev = None
    for k, leaf in place_cases:
        pre_p, br_p = cmp_eq_const(g, "selectedPlant", float(k))
        chain(g, pre_p + [br_p])
        if prev is None:
            g.edge(br_af, "then", pre_p[0])
        else:
            g.edge(prev, "else", pre_p[0])
        g.edge(br_p, "then", leaf)
        prev = br_p
    return g.emit(fname)


# ============ 8. hud.rxgraph ============
def gen_hud():
    g = G("hud")
    om = g.node("event.on_message")
    s = Seq(g); s.add(om)
    kind = n_call(g, RULES, "msg_kind", G.p(om, "payload")); s.add(kind)
    s.add(n_vset(g, "hudKind", G.p(kind, "result")))
    # kind 1:阳光数 → 四位数字帧
    pre_kind, br_kind = cmp_eq_const(g, "hudKind", 1.0)
    s.add_all(pre_kind); s.add(br_kind)
    body = Seq(g)
    value = n_call(g, RULES, "msg_value", G.p(om, "payload")); body.add(value)
    body.add(n_vset(g, "sunV", G.p(value, "result")))
    for tag_name, fn in (("hud_d1000", "digit_thousands"), ("hud_d100", "digit_hundreds"),
                         ("hud_d10", "digit_tens"), ("hud_d1", "digit_ones")):
        dcall = n_call(g, RULES, fn, G.p(n_vget(g, "sunV"), "out")); body.add(dcall)
        body.add(n_frame(g, G.p(n_find(g, tag_name), "entity"), G.p(dcall, "result")))
    g.edge(br_kind, "then", body.head)
    # kind 3:胜利横幅;kind 4:失败横幅
    pre3, br3 = cmp_eq_const(g, "hudKind", 3.0)
    chain(g, pre3 + [br3]); g.edge(br_kind, "else", pre3[0])
    g.edge(br3, "then", n_sett(g, G.p(n_find(g, "banner_win"), "entity"), G.c(BANNER_POS)))
    pre4, br4 = cmp_eq_const(g, "hudKind", 4.0)
    chain(g, pre4 + [br4]); g.edge(br3, "else", pre4[0])
    g.edge(br4, "then", n_sett(g, G.p(n_find(g, "banner_lose"), "entity"), G.c(BANNER_POS)))
    return g.emit("hud.rxgraph")


if __name__ == "__main__":
    # 独立运行 = 预览/自检,产物落 .forge/tmp/graph_preview,不污染 Content/Graphs(正式产物由 build_all_levels.py 生成)。
    OUT = ROOT / ".forge" / "tmp" / "graph_preview"
    paths = [gen_zombie(), gen_pea(), gen_sun(), gen_probe(), gen_loseline(), gen_mower(),
             gen_plant(1, "shooter", "Plants/plant_1.rxgraph", 100.0), gen_controller(), gen_hud()]
    for p in paths:
        d = json.loads(p.read_text(encoding="utf-8"))
        print(f"{p.name}: nodes={len(d['nodes'])} edges={len(d['edges'])}")
