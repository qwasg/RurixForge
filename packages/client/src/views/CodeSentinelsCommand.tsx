import { useEffect, useMemo, useRef, useState, type DragEvent, type PointerEvent } from 'react';
import { Activity, ArrowLeft, ArrowRight, ArrowUp, BatteryCharging, Boxes, Cable, Check, ChevronDown, ChevronUp, CircleHelp, Coins, Cpu, Crosshair, Factory, Flag, Grid2X2, Home, Layers, LockKeyhole, MousePointer2, Move, Network, Pause, Play, Plus, RadioTower, RotateCcw, Shield, Target, Trash2, Unplug, Users, Wrench, X, Zap } from 'lucide-react';
import { GPUS, OPERATORS, BUGS } from '@/lib/sentinelsV2';
import { BUILDINGS, COMMAND, GRID_WIDTH, GRID_HEIGHT, GPU_TECH, UNIT_TECH, TERRAIN_NAMES, GPUS as GPU_GAMEPLAY } from '@/lib/sentinelsV4';
import { useSentinelsCommandRuntime } from '@/lib/useSentinelsCommandRuntime';
import { COMMAND_HOTKEYS, useCommandHotkeys } from '@/lib/useCommandHotkeys';
import TacticalCard, { TACTICAL_CARD_MIME } from '@/components/game/TacticalCard';
import CommandLobby from '@/components/game/CommandLobby';
import CommandWallAnimation from '@/components/game/CommandWallAnimation';
import CommandPlacementPreview from '@/components/game/CommandPlacementPreview';
import CommandHub from '@/components/game/CommandHub';
import CommandResourcePanel from '@/components/game/CommandResourcePanel';
import { loadCommandSettings, playCommandTone, saveCommandSettings } from '@/lib/commandSettings';
import './code-sentinels-command.css';

const ASSETS = '/games/code-sentinels/', ART = ASSETS + 'ui-v4/art/', OLD_ART = ASSETS + 'ui-v3/art/';
const WORLD_WIDTH = GRID_HEIGHT * 16 / 9, MAP_MARGIN = (WORLD_WIDTH - GRID_WIDTH) / 2;
const OP_ART = ['operator-vscode', 'operator-pycharm', 'operator-deepseek', 'operator-gpt'];
const TURRET_ART = ['vscode-turret', 'pycharm-turret'];
const BUILD_ART: Record<number, string> = { 1: 'command-core', 2: 'data-center', 3: 'wind-power', 4: 'hydro-power', 5: 'coal-power', 6: 'nuclear-power', 7: 'mobile-relay', 8: 'research-lab', 9: 'resource-extractor', 10: 'cudad-wall' };
const TERRAIN_COLORS = ['#365849', '#2a3531', '#29576b', '#817b59', '#576b57', '#8b9d7b', '#674849', '#718a8b', '#716051', '#66888a'];
const number = (value: number) => Math.max(0, Math.floor(value)).toLocaleString('en-US');
const rate = (value: number) => Math.max(0, value).toLocaleString('en-US', { maximumFractionDigits: 1 });
const clamp = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));
const center = (cell: number) => ({ x: cell % GRID_WIDTH + .5 + MAP_MARGIN, y: Math.floor(cell / GRID_WIDTH) + .5 });
const nativePoint = (entity: { x: number; y: number }) => ({ x: entity.x + WORLD_WIDTH / 2, y: GRID_HEIGHT / 2 - entity.y });
const cellName = (cell: number) => `${String.fromCharCode(65 + cell % GRID_WIDTH % 26)}${cell % GRID_WIDTH >= 26 ? '2' : ''}${Math.floor(cell / GRID_WIDTH) + 1}`;
const buildingArt = (kind: number) => ART + BUILD_ART[kind] + '.png';
const buildingSize = (kind: number) => kind === 6 ? 3 : [1, 2, 4, 5, 8].includes(kind) ? 2 : 1;
const buildingSpriteSize = (kind: number) => kind === 1 ? 1.6 : kind === 6 ? 1.9 : [2, 4, 5, 8].includes(kind) ? 1.45 : kind === 7 ? 1.1 : 1;
function buildingContainsCell(building: { kind: number; cell: number }, cell: number): boolean {
  const size = buildingSize(building.kind), dx = cell % GRID_WIDTH - building.cell % GRID_WIDTH;
  const dy = Math.floor(cell / GRID_WIDTH) - Math.floor(building.cell / GRID_WIDTH);
  return dx >= 0 && dy >= 0 && dx < size && dy < size;
}
function buildingFootprint(kind: number, anchor: number): number[] {
  const size = buildingSize(kind), x = anchor % GRID_WIDTH, y = Math.floor(anchor / GRID_WIDTH);
  if (x + size > GRID_WIDTH || y + size > GRID_HEIGHT) return [];
  return Array.from({ length: size * size }, (_, index) => (y + Math.floor(index / size)) * GRID_WIDTH + x + index % size);
}
const gpuArt = (id: number) => OLD_ART + (id >= 6 ? 'hardware-datacenter' : id === 5 ? 'hardware-pro' : 'hardware-consumer') + '.png';
type Point = { x: number; y: number };
type Selection = { kind: 'unit' | 'building' | 'wall' | 'link'; slot: number };
type Deck = 'buildings' | 'units' | 'gpus';
type Tool = 'select' | 'build' | 'deploy' | 'power' | 'compute' | 'wall' | 'shield' | 'move' | 'skill';
type DragCard = { kind: 'building' | 'operator' | 'hardware'; id: number };
type PendingPlacement = { kind: number; cell: number; sentAt: number };
type DeferredLinkEndpoint = { placement: PendingPlacement; tool: 'power' | 'compute'; from: number | null; cell: number; shift: boolean };
const PLACEMENT_SYNC_TIMEOUT = 5000;
type Gesture = { kind: 'select' | 'pan' | 'wall' | 'click'; start: Point; end: Point; client: Point; pan: Point; shift: boolean; pointerId: number };
const selectionKey = (item: Selection) => `${item.kind}:${item.slot}`;

function wallPath(a: number, b: number): number[] {
  let x = a % GRID_WIDTH, y = Math.floor(a / GRID_WIDTH);
  const tx = b % GRID_WIDTH, ty = Math.floor(b / GRID_WIDTH), cells = [a];
  // Orthogonal strokes preserve a closed, cell-based topology; the engine validates the same path.
  while (x !== tx) { x += Math.sign(tx - x); cells.push(y * GRID_WIDTH + x); }
  while (y !== ty) { y += Math.sign(ty - y); cells.push(y * GRID_WIDTH + x); }
  return cells;
}

function segmentDistance(point: Point, a: Point, b: Point): number {
  const dx = b.x - a.x, dy = b.y - a.y, length = dx * dx + dy * dy;
  const t = length ? clamp(((point.x - a.x) * dx + (point.y - a.y) * dy) / length, 0, 1) : 0;
  return Math.hypot(point.x - a.x - dx * t, point.y - a.y - dy * t);
}

export default function CodeSentinelsCommand({ version = 4 }: { version?: 4 | 5 }) {
  const r = useSentinelsCommandRuntime(version), s = r.state;
  const operatorArt = (kind: number) => version === 5 && kind < 3
    ? ASSETS + 'ui-v5/building-references/' + TURRET_ART[kind - 1] + '.png'
    : OLD_ART + OP_ART[kind - 1] + '.png';
  const [wallAnimationReady, setWallAnimationReady] = useState(false);
  const [settings, setSettings] = useState(loadCommandSettings);
  const [pendingSector, setPendingSector] = useState<{ level: number; fresh: boolean } | null>(null);
  const [hubTransition, setHubTransition] = useState(false);
  const lobbyFromHub = useRef(false);
  const [deck, setDeck] = useState<Deck>('buildings'), [deckOpen, setDeckOpen] = useState(true);
  const [tool, setTool] = useState<Tool>('select'), [card, setCard] = useState(3), [gpuModel, setGpuModel] = useState(1);
  const [selection, setSelection] = useState<Selection[]>([]), [gpuSlot, setGpuSlot] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null), [linkStart, setLinkStart] = useState<number | null>(null);
  const [dragCard, setDragCard] = useState<DragCard | null>(null), [gesture, setGesture] = useState<Gesture | null>(null);
  const [overlay, setOverlay] = useState(1), [zoom, setZoom] = useState(1), [pan, setPan] = useState<Point>({ x: 0, y: 0 });
  const [drawer, setDrawer] = useState<'help' | 'tech' | 'lobby' | 'hub' | 'resources' | null>(null), [tutorial, setTutorial] = useState(true);
  const [noticeOpen, setNoticeOpen] = useState(false), [inspectResult, setInspectResult] = useState(false);
  const shellRef = useRef<HTMLDivElement>(null), boardRef = useRef<HTMLDivElement>(null), windowRef = useRef<HTMLDivElement>(null), modalRef = useRef<HTMLElement>(null);
  const gestureRef = useRef<Gesture | null>(null), lastIntent = useRef<{ key: string; time: number } | null>(null);
  const pendingPlacements = useRef(new Map<string, PendingPlacement>()), deferredLinkEndpoint = useRef<DeferredLinkEndpoint | null>(null);
  const deferredLinkTimer = useRef<number | null>(null);
  const activeBuildings = s.buildings.filter(b => b.active), activeUnits = s.units.filter(u => u.active);
  const activeLinks = s.links.filter(link => link.active), activeEnemies = s.enemies.filter(enemy => enemy.active);
  const primary = selection[0], unit = primary?.kind === 'unit' ? activeUnits.find(u => u.slot === primary.slot) : undefined;
  const building = primary?.kind === 'building' ? activeBuildings.find(b => b.slot === primary.slot) : undefined;
  const selectedGpu = gpuSlot === null ? undefined : s.gpus.find(g => g.slot === gpuSlot && g.model > 0);
  const referenceGpu = GPUS[(selectedGpu?.model ?? gpuModel) - 1];
  const selectedLink = primary?.kind === 'link' ? activeLinks.find(link => link.slot === primary.slot) : undefined;
  const selectedWall = primary?.kind === 'wall' ? primary.slot : null;
  const commandCore = activeBuildings.find(b => b.kind === 1 && b.owner === 0);
  const usable = r.ready && r.nativeReady && r.connected && !r.busy && !r.paused && s.phase < 2;
  const gameOver = r.ready && s.phase >= 2;
  const campaignComplete = s.phase === 2 && s.level >= 3;
  const finishingDestruction = Boolean(r.animationState && (
    r.animationState.buildings.some(item => item.phase === 4)
    || r.animationState.units.some(item => item.phase === 4)
    || r.animationState.events.some(event => event.active && [5, 16, 17].includes(event.type) && event.age < 2)
  ));
  const standalone = new URLSearchParams(location.search).get('standalone') === '1';
  const buildCards = BUILDINGS.filter(b => b.id >= 2 && b.id <= 9);
  const selectedMeta = building ? BUILDINGS.find(meta => meta.id === building.kind) : undefined;
  const chosenBuilding = BUILDINGS.find(meta => meta.id === card);
  const rack = building?.kind === 2 ? s.gpus.filter(g => g.centerSlot === building.slot) : [];
  const researchLab = activeBuildings.find(b => b.kind === 8 && b.owner === 0);
  const wallStart = gesture?.kind === 'wall' ? pointCell(gesture.start) : null, wallEnd = gesture?.kind === 'wall' ? pointCell(gesture.end) : null;
  const wallDraft = wallStart !== null && wallEnd !== null ? wallPath(wallStart, wallEnd) : [];
  const closedCells = useMemo(() => s.closedCells.flatMap((value, cell) => value ? [cell] : []), [s.closedCells]);
  const wallCells = useMemo(() => s.wallCells.flatMap((value, cell) => value ? [cell] : []), [s.wallCells]);
  const closed = useMemo(() => new Set(closedCells), [closedCells]);
  const walls = useMemo(() => new Set(wallCells), [wallCells]);
  const anyTool = tool !== 'select' || Boolean(dragCard);

  useEffect(() => { document.title = '编译防线 · 指挥网络'; }, []);
  useEffect(() => { saveCommandSettings(settings); }, [settings]);
  useEffect(() => { setOverlay(settings.overlay); }, [settings.overlay]);
  useEffect(() => { setTutorial(settings.showTutorial); }, [settings.showTutorial]);
  useEffect(() => { if (r.error && !r.busy) setPendingSector(null); }, [r.error, r.busy]);
  useEffect(() => {
    setNoticeOpen(Boolean(r.notice)); const timer = window.setTimeout(() => setNoticeOpen(false), 4600);
    return () => clearTimeout(timer);
  }, [r.notice, r.noticeRevision]);
  useEffect(() => { setLinkStart(null); setGesture(null); gestureRef.current = null; lastIntent.current = null;
    pendingPlacements.current.clear(); clearDeferredLink(); }, [r.connectionEpoch]);
  useEffect(() => {
    if (!r.ready || !r.connected) { pendingPlacements.current.clear(); clearDeferredLink(); }
  }, [r.ready, r.connected]);
  useEffect(() => () => clearDeferredLink(), []);
  useEffect(() => {
    const pending = deferredLinkEndpoint.current;
    if (pending && (tool !== pending.tool || linkStart !== pending.from)) clearDeferredLink();
    else if (pending && performance.now() - pending.placement.sentAt >= PLACEMENT_SYNC_TIMEOUT) {
      // Background tabs may delay the timer; a late publication cannot revive
      // an expired intention merely because its timer callback has not run yet.
      clearDeferredLink(); pendingPlacements.current.delete(`${pending.placement.kind}:${pending.placement.cell}`);
      r.setNotice('尚未确认该建筑，连线未发送；请确认建造结果后重试');
    }
    else if (pending && usable && s.buildings.some(building => building.active && building.owner === 0
      && building.kind === pending.placement.kind && building.cell === pending.placement.cell)) {
      clearDeferredLink();
      // Retry only the endpoint interpretation, using the confirmed publication.
      // The existing footprint hit test supplies its authoritative anchor.
      clickMap(center(pending.cell), pending.shift);
    }
    for (const [id, placement] of pendingPlacements.current) {
      if (performance.now() - placement.sentAt >= PLACEMENT_SYNC_TIMEOUT || s.buildings.some(building => building.active
        && building.kind === placement.kind && building.cell === placement.cell)) pendingPlacements.current.delete(id);
    }
  }, [s.buildings, tool, linkStart, usable]);
  useEffect(() => {
    setSelection(previous => previous.filter(item => item.kind === 'unit' ? s.units.some(u => u.slot === item.slot && u.kind > 0)
      : item.kind === 'building' ? s.buildings.some(b => b.slot === item.slot && b.kind > 0)
      : item.kind === 'link' ? s.links.some(l => l.slot === item.slot && l.active) : s.wallCells[item.slot]));
    if (gpuSlot !== null && !s.gpus.some(g => g.slot === gpuSlot && g.model > 0)) setGpuSlot(null);
  }, [s.units, s.buildings, s.links, s.wallCells, s.gpus, gpuSlot]);
  useEffect(() => {
    if (tool === 'skill' && !unit) setTool('select');
  }, [tool, unit]);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    if (shellRef.current) shellRef.current.inert = Boolean(drawer) || !r.ready;
    if (drawer === 'help' || drawer === 'tech') modalRef.current?.focus();
    return () => { if (shellRef.current) shellRef.current.inert = false; if (drawer) previous?.focus(); };
  }, [drawer, r.ready]);
  useEffect(() => {
    if (!pendingSector || !r.nativeReady || !r.connected || r.paused || r.busy) return;
    if (!(pendingSector.fresh && s.level === pendingSector.level) && !r.send(COMMAND.selectLevel(pendingSector.level))) return;
    pendingPlacements.current.clear(); clearDeferredLink();
    setPendingSector(null); setDrawer(null); setSelection([]); setGpuSlot(null); setTool('select'); setLinkStart(null);
    setDeck('buildings'); setDeckOpen(true); setZoom(1); setPan({ x: 0, y: 0 }); setInspectResult(false); setTutorial(settings.showTutorial);
  }, [pendingSector, r.nativeReady, r.connected, r.paused, r.busy, r.send, s.level, settings.showTutorial]);

  function pointCell(point: Point): number | null {
    const col = Math.floor(point.x - MAP_MARGIN), row = Math.floor(point.y);
    return col >= 0 && col < GRID_WIDTH && row >= 0 && row < GRID_HEIGHT ? row * GRID_WIDTH + col : null;
  }
  function eventPoint(event: { clientX: number; clientY: number }): Point {
    const rect = boardRef.current?.getBoundingClientRect();
    return rect ? { x: (event.clientX - rect.left) / rect.width * WORLD_WIDTH, y: (event.clientY - rect.top) / rect.height * GRID_HEIGHT } : { x: 0, y: 0 };
  }
  function focusMap() { shellRef.current?.focus({ preventScroll: true }); }
  function clearDeferredLink() {
    deferredLinkEndpoint.current = null;
    if (deferredLinkTimer.current !== null) clearTimeout(deferredLinkTimer.current);
    deferredLinkTimer.current = null;
  }
  function buildingAt(cell: number) { return activeBuildings.find(building => buildingContainsCell(building, cell)); }
  function invalidPlacement(kind: number, anchor: number): boolean {
    const cells = kind ? buildingFootprint(kind, anchor) : [anchor];
    if (!cells.length || cells.some(cell => s.terrain[cell] === undefined || [1, 2].includes(s.terrain[cell]) || walls.has(cell)
      || activeBuildings.some(building => building.kind !== 7 && buildingContainsCell(building, cell))
      || activeUnits.some(unit => unit.cell === cell))) return true;
    const nearby = (terrain: number, distance: number) => s.terrain.some((tile, cell) => tile === terrain
      && Math.hypot(cell % GRID_WIDTH - anchor % GRID_WIDTH, Math.floor(cell / GRID_WIDTH) - Math.floor(anchor / GRID_WIDTH)) <= distance);
    if (kind === 4) return !nearby(2, 2.1);
    if (kind === 5) return !nearby(6, 3.1);
    if (kind === 6) return !nearby(2, 4.1);
    if (kind === 9) return ![5, 6].includes(s.terrain[anchor]);
    return false;
  }
  function issue(command: number, key = String(command)): boolean {
    if (!usable) return false;
    if (lastIntent.current?.key === key && performance.now() - lastIntent.current.time < 350) return false;
    if (!r.send(command)) return false;
    // This records an intention only. Buildings and money remain snapshot-owned.
    if (command >= 1_002_000 && command < 1_010_000) {
      const kind = Math.floor((command - 1_000_000) / 1000), cell = command % 1000;
      if (cell < GRID_WIDTH * GRID_HEIGHT) pendingPlacements.current.set(`${kind}:${cell}`, { kind, cell, sentAt: performance.now() });
    }
    lastIntent.current = { key, time: performance.now() }; return true;
  }
  function cancel() {
    if (deferredLinkEndpoint.current) { clearDeferredLink(); r.setNotice('已取消等待中的连线'); }
    if (drawer === 'hub') { if (!hubTransition && !pendingSector) continueOperation(); return; }
    if (drawer) { setDrawer(null); return; }
    if (tool !== 'select' || linkStart !== null) { setTool('select'); setLinkStart(null); setGesture(null); gestureRef.current = null; return; }
    if (selection.length || gpuSlot !== null) { setSelection([]); setGpuSlot(null); return; }
    setDeckOpen(false);
  }
  function chooseDeck(next: Deck) {
    setDeck(next); setDeckOpen(true); setTool('select'); setLinkStart(null); focusMap();
    if (next === 'gpus' && building?.kind !== 2) r.setNotice('点击地图上的数据中心，再将显卡装入它的机架');
  }
  function chooseCard(kind: DragCard['kind'], id: number) {
    focusMap(); setLinkStart(null); setGpuSlot(null);
    if (kind === 'hardware') {
      setGpuModel(id); setDeck('gpus'); setDeckOpen(true); setTool('select');
      r.setNotice(building?.kind === 2 ? '选择空槽，或 Enter 安装到首个空槽' : '将显卡拖到数据中心，或先点选地图上的数据中心');
      return;
    }
    const required = kind === 'building' ? BUILDINGS.find(meta => meta.id === id)?.tech ?? 0 : UNIT_TECH[id];
    if (required > s.tech) { r.setNotice(`需要科技 ${required} 级 · 为研究院接入电力与算力，再升级科技`); return; }
    setCard(id); setDeck(kind === 'building' ? 'buildings' : 'units'); setTool(kind === 'building' ? 'build' : 'deploy');
    setDeckOpen(false); setSelection([]);
    r.setNotice(kind === 'building' ? '点击合适地形建造 · Shift 可连续建造 · Esc 取消' : id < 3 ? '选择固定炮台位置，再从数据中心拉算力线接入' : '在无线覆盖中部署 AI，可携带随身算力向前推进');
  }
  function select(items: Selection[], additive = false) {
    setSelection(previous => additive ? [...previous.filter(old => !items.some(item => selectionKey(item) === selectionKey(old))), ...items] : items);
    setGpuSlot(null); setTool('select'); setLinkStart(null); setDeckOpen(false); focusMap();
  }
  function useTool(next: Exclude<Tool, 'select' | 'build' | 'deploy'>) {
    if (!r.ready) return;
    if (next === 'skill' && !unit) { r.setNotice('先选择一名已部署角色，再按 Q 选择技能落点'); return; }
    if (next === 'move' && !selection.length) { r.setNotice('先选择 AI 或移动基站，可用拖框选中多个单位'); return; }
    setTool(value => value === next ? 'select' : next); setLinkStart(null); setDeckOpen(false); focusMap();
    const hint: Record<string, string> = {
      power: '电力线：点击发电站或已供电建筑，再点击用电建筑；同类网络可串接',
      compute: '算力线：从数据中心接到固定炮台、驻守 AI 或研究院；移动基站自动无线回传',
      wall: '按住左键拖动修建 cudad 护城河 · 直角路径预览 · 松开后提交',
      shield: '围墙内需要供电的数据中心 · 点击闭环内部切换护盾自动充能',
      move: '点击目标位置下达移动命令；AI 离开无线覆盖后会消耗随身算力',
      skill: '点击技能落点 · 覆盖外从随身算力扣除，覆盖内由网络供给',
    };
    r.setNotice(hint[next]);
  }
  function installGpu(centerSlot = building?.kind === 2 ? building.slot : undefined, bay?: number, model = gpuModel) {
    if (centerSlot === undefined) { r.setNotice('先点击地图上的数据中心'); return; }
    const available = s.gpus.filter(g => g.centerSlot === centerSlot);
    const targetBay = bay ?? available.find(g => g.model === 0)?.bay ?? [0, 1, 2, 3].find(index => !available.some(g => g.bay === index && g.model > 0));
    if (targetBay === undefined) { r.setNotice('此数据中心四个机架已满，可建造另一座数据中心'); return; }
    if (GPU_TECH[model] > s.tech) { r.setNotice(`该显卡需要科技 ${GPU_TECH[model]} 级`); return; }
    if (issue(COMMAND.installGpu(centerSlot, targetBay, model))) r.setNotice('显卡安装指令已发送，实际产能取决于数据中心供电');
  }
  function moveSelected(cell: number) {
    if (!usable) return;
    const movers = selection.filter(item => item.kind === 'unit' ? activeUnits.some(u => u.slot === item.slot && u.kind >= 3 && !u.wired && u.owner === 0)
      : item.kind === 'building' && activeBuildings.some(b => b.slot === item.slot && b.kind === 7 && b.owner === 0));
    if (!movers.length) { r.setNotice('编译器是固定炮台；有线 AI 需先拆除算力线，移动基站可直接移动'); return; }
    const formation = Math.ceil(Math.sqrt(movers.length));
    const commands = movers.map((item, index) => {
      const col = clamp(cell % GRID_WIDTH + index % formation - Math.floor(formation / 2), 0, GRID_WIDTH - 1);
      const row = clamp(Math.floor(cell / GRID_WIDTH) + Math.floor(index / formation), 0, GRID_HEIGHT - 1);
      return item.kind === 'unit' ? COMMAND.moveUnit(item.slot, row * GRID_WIDTH + col) : COMMAND.moveRelay(item.slot, row * GRID_WIDTH + col);
    });
    const sent = r.sendMany(commands);
    if (sent) r.setNotice(`${sent} 个单位收到移动指令${sent < movers.length ? `，另 ${movers.length - sent} 个未发送` : ''} · 电池耗尽会停止开火`);
    setTool('select');
  }
  function upgrade() {
    if (selectedGpu) { issue(COMMAND.upgradeGpu(selectedGpu.slot)); return; }
    if (building) { issue(building.kind === 8 ? COMMAND.research() : COMMAND.upgradeBuilding(building.slot)); return; }
    if (unit) issue(COMMAND.upgradeUnit(unit.slot));
  }
  function recycle() {
    if (selectedGpu) { issue(COMMAND.sellGpu(selectedGpu.slot)); return; }
    if (building && building.kind !== 1) { issue(COMMAND.sellBuilding(building.slot)); return; }
    if (unit) { issue(COMMAND.sellUnit(unit.slot)); return; }
    if (selectedLink) { issue(COMMAND.deleteLink(selectedLink.slot)); return; }
    if (selectedWall !== null) issue(COMMAND.removeWall(selectedWall));
  }
  function clickMap(point: Point, shift: boolean) {
    const cell = pointCell(point); if (cell === null) return;
    const hitBuilding = buildingAt(cell);
    const hitUnit = activeUnits.find(u => u.cell === cell);
    if (tool === 'move') { moveSelected(cell); return; }
    if (tool === 'skill') { if (unit) issue(COMMAND.skill(unit.slot, cell)); setTool('select'); return; }
    if (tool === 'power' || tool === 'compute') {
      clearDeferredLink();
      const pending = !hitBuilding ? [...pendingPlacements.current.values()].find(placement => performance.now() - placement.sentAt < PLACEMENT_SYNC_TIMEOUT
        && buildingContainsCell(placement, cell)) : undefined;
      if (pending) {
        const deferred: DeferredLinkEndpoint = { placement: pending, tool, from: linkStart, cell, shift };
        deferredLinkEndpoint.current = deferred;
        r.setNotice('建筑状态同步中，确认后继续连接；Esc 可取消');
        deferredLinkTimer.current = window.setTimeout(() => {
          if (deferredLinkEndpoint.current !== deferred) return;
          clearDeferredLink(); pendingPlacements.current.delete(`${pending.kind}:${pending.cell}`);
          r.setNotice('尚未确认该建筑，连线未发送；请确认建造结果后重试');
        }, Math.max(0, PLACEMENT_SYNC_TIMEOUT - (performance.now() - pending.sentAt)));
        return;
      }
      const endpoint = hitBuilding?.cell ?? hitUnit?.cell ?? cell;
      if (linkStart === null) {
        if (!hitBuilding && !hitUnit) { r.setNotice('点击建筑或单位作为连线端点'); return; }
        setLinkStart(endpoint); r.setNotice('起点已选 · 点击另一个建筑或单位完成连线');
      } else if (linkStart === endpoint) setLinkStart(null);
      else {
        if (issue(tool === 'power' ? COMMAND.powerLine(linkStart, endpoint) : COMMAND.computeLine(linkStart, endpoint))) {
          setLinkStart(null); if (!shift) setTool('select');
        }
      }
      return;
    }
    if (tool === 'shield') {
      if (!closed.has(cell)) { r.setNotice('这里尚未形成拓扑封闭区域，先用 cudad 护城河围成闭环'); return; }
      issue(COMMAND.toggleShield()); setTool('select'); return;
    }
    if (tool === 'build' || tool === 'deploy') {
      if (issue(tool === 'build' ? COMMAND.build(card, cell) : COMMAND.deploy(card, cell))) {
        if (!shift) setTool('select');
      }
      return;
    }
    if (hitUnit) { select([{ kind: 'unit', slot: hitUnit.slot }], shift); return; }
    if (hitBuilding) {
      select([{ kind: 'building', slot: hitBuilding.slot }], shift);
      if (hitBuilding.kind === 2) { setDeck('gpus'); setDeckOpen(true); }
      return;
    }
    if (walls.has(cell)) { select([{ kind: 'wall', slot: cell }]); return; }
    const link = activeLinks.find(link => wallPath(link.fromCell, link.toCell).some((cell, index, cells) => index > 0 && segmentDistance(point, center(cells[index - 1]), center(cell)) < .2));
    if (link) { select([{ kind: 'link', slot: link.slot }]); return; }
    if (!shift) { setSelection([]); setGpuSlot(null); }
  }
  function pointerDown(event: PointerEvent<HTMLDivElement>) {
    if (!r.nativeReady || (event.button !== 0 && event.button !== 1)) return;
    event.preventDefault(); focusMap();
    const point = eventPoint(event);
    const value: Gesture = { kind: event.button === 1 || event.altKey ? 'pan' : tool === 'wall' ? 'wall' : tool === 'select' ? 'select' : 'click', start: point, end: point, client: { x: event.clientX, y: event.clientY }, pan, shift: event.shiftKey, pointerId: event.pointerId };
    gestureRef.current = value; setGesture(value); event.currentTarget.setPointerCapture(event.pointerId);
  }
  function pointerMove(event: PointerEvent<HTMLDivElement>) {
    const point = eventPoint(event); setHover(pointCell(point));
    const current = gestureRef.current; if (!current || current.pointerId !== event.pointerId) return;
    if (current.kind === 'pan') setPan({ x: current.pan.x + event.clientX - current.client.x, y: current.pan.y + event.clientY - current.client.y });
    const value = { ...current, end: point }; gestureRef.current = value; setGesture(value);
  }
  function pointerUp(event: PointerEvent<HTMLDivElement>) {
    const current = gestureRef.current; if (!current || current.pointerId !== event.pointerId) return;
    const point = eventPoint(event); gestureRef.current = null; setGesture(null);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    if (current.kind === 'pan') return;
    if (current.kind === 'wall') {
      const a = pointCell(current.start), b = pointCell(point);
      if (a !== null && b !== null) issue(COMMAND.wall(a, b));
      return;
    }
    if (current.kind === 'select' && Math.hypot(event.clientX - current.client.x, event.clientY - current.client.y) > 7) {
      const minX = Math.min(current.start.x, point.x), maxX = Math.max(current.start.x, point.x), minY = Math.min(current.start.y, point.y), maxY = Math.max(current.start.y, point.y);
      const inside = (cell: number) => { const p = center(cell); return p.x >= minX && p.x <= maxX && p.y >= minY && p.y <= maxY; };
      const items: Selection[] = [...activeUnits.filter(u => u.owner === 0 && inside(u.cell)).map(u => ({ kind: 'unit' as const, slot: u.slot })), ...activeBuildings.filter(b => b.owner === 0 && b.kind === 7 && inside(b.cell)).map(b => ({ kind: 'building' as const, slot: b.slot }))];
      select(items, current.shift); return;
    }
    clickMap(point, current.shift);
  }
  function changeZoom(delta: number, client?: Point) {
    const next = clamp(zoom + delta, 1, 3.5), ratio = next / zoom;
    const area = windowRef.current?.getBoundingClientRect();
    const anchor = client && area ? { x: client.x - area.left - area.width / 2, y: client.y - area.top - area.height / 2 } : { x: 0, y: 0 };
    setPan(value => ({ x: anchor.x - (anchor.x - value.x) * ratio, y: anchor.y - (anchor.y - value.y) * ratio })); setZoom(next);
  }
  function minimapMove(event: PointerEvent<SVGSVGElement>) {
    const rect = event.currentTarget.getBoundingClientRect(), board = boardRef.current;
    if (!board || !r.ready) return;
    const col = clamp((event.clientX - rect.left) / rect.width, 0, 1) * GRID_WIDTH;
    const row = clamp((event.clientY - rect.top) / rect.height, 0, 1) * GRID_HEIGHT;
    setPan({ x: -(col + MAP_MARGIN - WORLD_WIDTH / 2) / WORLD_WIDTH * board.clientWidth * zoom, y: -(row - GRID_HEIGHT / 2) / GRID_HEIGHT * board.clientHeight * zoom });
  }
  function dropCard(event: DragEvent<HTMLDivElement>) {
    event.preventDefault(); setDragCard(null); const cell = pointCell(eventPoint(event)); if (cell === null || !usable) return;
    let payload: DragCard;
    try { payload = JSON.parse(event.dataTransfer.getData(TACTICAL_CARD_MIME)); } catch { return; }
    if (!payload || !Number.isInteger(payload.id)) return;
    if (payload.kind === 'hardware' && payload.id >= 1 && payload.id <= 7) {
      const target = activeBuildings.find(b => b.kind === 2 && buildingContainsCell(b, cell) && b.owner === 0);
      if (target) { select([{ kind: 'building', slot: target.slot }]); installGpu(target.slot, undefined, payload.id); }
      else r.setNotice('显卡需要装入真实地图上的数据中心');
    } else if (payload.kind === 'operator' && payload.id >= 1 && payload.id <= 4) issue(COMMAND.deploy(payload.id, cell));
    else if (payload.kind === 'building' && payload.id >= 2 && payload.id <= 9) issue(COMMAND.build(payload.id, cell));
  }
  async function start(level = s.level) {
    if (r.busy || hubTransition || pendingSector) return;
    setDrawer('hub');
    setPendingSector({ level, fresh: !r.ready });
    if (!r.ready && !await r.start()) { setPendingSector(null); return; }
    if (r.paused) await r.togglePause();
  }
  async function openHub() {
    setDrawer('hub');
    if (r.ready && !r.paused && !gameOver) {
      setHubTransition(true); try { await r.togglePause(); } finally { setHubTransition(false); }
    }
  }
  function continueOperation() {
    if (!r.ready) return;
    setDrawer(null); if (gameOver) setInspectResult(true); if (r.paused) void r.togglePause();
  }
  function locateResource(slot: number) {
    const target = activeBuildings.find(building => building.slot === slot && building.owner === 0); if (!target) return;
    const point = nativePoint(target), offset = (buildingSize(target.kind) - 1) / 2;
    const nextZoom = Math.max(1.4, zoom), board = boardRef.current;
    if (board) setPan({ x: -(point.x + offset - WORLD_WIDTH / 2) / WORLD_WIDTH * board.clientWidth * nextZoom,
      y: -(point.y + offset - GRID_HEIGHT / 2) / GRID_HEIGHT * board.clientHeight * nextZoom });
    setZoom(nextZoom); select([{ kind: 'building', slot }]); setDrawer(null); setTool('select'); setGpuSlot(null);
    if (target.kind === 2) { setDeck('gpus'); setDeckOpen(true); }
    focusMap();
  }
  function changeOverlay() { setSettings(value => ({ ...value, overlay: (overlay + 1) % 3 })); }

  useCommandHotkeys({ enabled: r.ready && settings.shortcuts, blocked: Boolean(drawer) || (gameOver && !inspectResult),
    selectCard: index => { if (deck === 'buildings' && buildCards[index]) chooseCard('building', buildCards[index].id); else if (deck === 'units' && OPERATORS[index]) chooseCard('operator', index + 1); else if (deck === 'gpus' && GPUS[index]) chooseCard('hardware', index + 1); },
    deck: chooseDeck, toggleDeck: () => setDeckOpen(value => !value), tool: useTool, upgrade, recycle,
    target: () => { if (unit) issue(COMMAND.targetMode(unit.slot, (unit.targetMode + 1) % 3)); }, install: () => installGpu(),
    overlay: changeOverlay, nextWave: () => issue(COMMAND.nextWave()), pause: () => void r.togglePause(), cancel,
    help: () => setDrawer('help'), pan: (x, y) => setPan(value => ({ x: value.x + x, y: value.y + y })), zoom: delta => changeZoom(delta), home: () => { setZoom(1); setPan({ x: 0, y: 0 }); },
  });

  const intention = tool === 'build' ? `${chosenBuilding?.name ?? '建筑'} · ${buildingSize(card)}×${buildingSize(card)} 占地（锚点向右下） · ${chosenBuilding?.description ?? ''}`
    : tool === 'deploy' ? `${OPERATORS[card - 1]?.name} · ${card < 3 ? '固定炮台，需要算力连线' : '移动 AI，网络覆盖可充满随身算力'}`
    : tool === 'power' ? `${linkStart === null ? '选择电力起点' : '选择电力终点'} · 发电站 → 数据中心 / 研究院`
    : tool === 'compute' ? `${linkStart === null ? '选择算力起点' : '选择算力终点'} · 数据中心 → 炮台 / 驻守 AI / 研究院`
    : tool === 'wall' ? `拖动修建 cudad 护城河${wallDraft.length ? ` · ${wallDraft.length} 格` : ''} · 围成闭环后按 F 充盾`
    : tool === 'shield' ? `闭环内需包含供能数据中心 · 点击切换护盾自动充能（当前${s.shieldAuto ? '开启' : '关闭'}）`
    : tool === 'move' ? '点击地面移动所选单位 · 无线覆盖外使用随身算力'
    : tool === 'skill' ? `${unit ? OPERATORS[unit.kind - 1].skillName : '技能'} · 点击地面指定释放位置` : '';
  const hoverBuilding = hover === null ? undefined : buildingAt(hover);
  const previewAnchor = hover === null ? null : tool === 'power' || tool === 'compute' || dragCard?.kind === 'hardware' ? hoverBuilding?.cell ?? hover : hover;
  const previewCell = previewAnchor === null ? null : center(previewAnchor);
  const previewBuildingKind = dragCard?.kind === 'building' ? dragCard.id : tool === 'build' ? card : 0;
  const previewSize = previewBuildingKind ? buildingSize(previewBuildingKind)
    : hoverBuilding && (tool === 'power' || tool === 'compute' || dragCard?.kind === 'hardware') ? buildingSize(hoverBuilding.kind) : 1;
  const previewOperatorKind = dragCard?.kind === 'operator' ? dragCard.id : tool === 'deploy' ? card : 0;
  const previewAsset = version === 5 ? previewBuildingKind ? BUILD_ART[previewBuildingKind]
    : previewOperatorKind > 0 && previewOperatorKind < 3 ? TURRET_ART[previewOperatorKind - 1] : null : null;
  const previewSpriteSize = previewBuildingKind ? (version === 5 ? previewSize : buildingSpriteSize(previewBuildingKind)) * 384 / 300 : 1.44;
  const previewSpriteOffset = previewBuildingKind ? (previewSize - 1) / 2 : 0;
  const previewImage = previewBuildingKind ? buildingArt(previewBuildingKind) : previewOperatorKind
    ? version === 5 && previewOperatorKind < 3 ? operatorArt(previewOperatorKind) : ASSETS + OPERATORS[previewOperatorKind - 1].image : null;
  const networkVisible = overlay > 0 || tool === 'power' || tool === 'compute';
  const ownedUnits = activeUnits.filter(u => u.owner === 0);
  const placingStructure = tool === 'build' || tool === 'deploy' || dragCard?.kind === 'building' || dragCard?.kind === 'operator';
  const placementBlocked = placingStructure && previewAnchor !== null && invalidPlacement(previewBuildingKind, previewAnchor);

  return <main className={`command-game ${r.ready ? 'is-live' : 'is-title'} ${settings.reducedMotion ? 'is-reduced-motion' : ''}`} onClickCapture={event => {
    const button = (event.target as Element).closest?.('button');
    if (button && !button.disabled && !button.hasAttribute('data-preview-tone')) playCommandTone(settings.volume);
  }}>
    <div className="command-shell" ref={shellRef} tabIndex={-1}>
      {r.ready ? <div className="command-map-window" ref={windowRef}>
        <div ref={boardRef} className={`command-board ${anyTool ? 'is-tool' : ''} ${gesture?.kind === 'pan' ? 'is-pan' : ''}`}
          style={{ transform: `translate(-50%,-50%) translate(${pan.x}px,${pan.y}px) scale(${zoom})` }}
          onPointerDown={pointerDown} onPointerMove={pointerMove} onPointerUp={pointerUp}
          onPointerCancel={() => { gestureRef.current = null; setGesture(null); }} onPointerLeave={() => { if (!gestureRef.current) setHover(null); }}
          onContextMenu={event => { event.preventDefault(); const cell = pointCell(eventPoint(event)); if (cell !== null) { if (tool !== 'select') cancel(); else moveSelected(cell); } }}
          onDoubleClick={event => { if (tool !== 'select') return; const cell = pointCell(eventPoint(event)), hit = activeUnits.find(u => u.cell === cell); if (hit) select(activeUnits.filter(u => u.kind === hit.kind && u.owner === 0).map(u => ({ kind: 'unit', slot: u.slot }))); }}
          onWheel={event => { event.preventDefault(); changeZoom(event.deltaY < 0 ? .15 : -.15, { x: event.clientX, y: event.clientY }); }}
          onDragOver={event => { if (event.dataTransfer.types.includes(TACTICAL_CARD_MIME)) { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; setHover(pointCell(eventPoint(event))); } }} onDrop={dropCard}>
          <canvas ref={r.canvasRef} width={1280} height={720} aria-label="编译防线原生实时战略地图，使用下方指挥工具或快捷键操作"/>
          {r.animationState && <CommandWallAnimation state={s} animation={r.animationState} paused={r.paused}
            connected={r.connected && r.nativeReady} onReady={setWallAnimationReady} onError={r.setNotice}/>}
          <svg className="command-map-overlay" viewBox={`0 0 ${WORLD_WIDTH} ${GRID_HEIGHT}`} aria-hidden="true">
            <defs><pattern id="command-grid" width="1" height="1" patternUnits="userSpaceOnUse" x={MAP_MARGIN}><path d="M1 0H0V1" fill="none" stroke="#d5ead326" strokeWidth=".025"/></pattern><pattern id="command-shield" width=".4" height=".4" patternUnits="userSpaceOnUse"><path d="M0 .2L.2 0L.4 .2L.2 .4Z" fill="none" stroke="#9becdd44" strokeWidth=".025"/></pattern><pattern id="command-wall-art" x={MAP_MARGIN} y="0" width="1" height="1" viewBox="0 0 1 1" patternUnits="userSpaceOnUse"><image href={buildingArt(10)} width="1" height="1" preserveAspectRatio="xMidYMid meet"/></pattern></defs>
            {overlay === 2 && <rect x={MAP_MARGIN} width={GRID_WIDTH} height={GRID_HEIGHT} fill="url(#command-grid)"/>}
            {networkVisible && closedCells.map(cell => { const p = center(cell); return <rect key={`shield-${cell}`} x={p.x - .5} y={p.y - .5} width="1" height="1" fill="url(#command-shield)" opacity={s.shieldCells[cell] ? .85 : .25}/>; })}
            {wallCells.map(cell => {
              const p = center(cell), x = p.x - .5, y = p.y - .5, col = cell % GRID_WIDTH, row = Math.floor(cell / GRID_WIDTH);
              const edges = [row === 0 || !walls.has(cell - GRID_WIDTH) ? `M${x},${y}h1` : '',
                row === GRID_HEIGHT - 1 || !walls.has(cell + GRID_WIDTH) ? `M${x},${y + 1}h1` : '',
                col === 0 || !walls.has(cell - 1) ? `M${x},${y}v1` : '',
                col === GRID_WIDTH - 1 || !walls.has(cell + 1) ? `M${x + 1},${y}v1` : ''].join(' ');
              return <g key={`wall-art-${cell}`}><rect x={x} y={y} width="1" height="1" fill={wallAnimationReady ? 'none' : 'url(#command-wall-art)'}/>
                <path d={edges} fill="none" stroke="#e2c781" strokeWidth=".055" strokeLinejoin="round" opacity=".75"/></g>;
            })}
            {networkVisible && activeBuildings.filter(b => b.kind === 7).map(b => { const p = nativePoint(b); return <circle key={`coverage-${b.slot}`} cx={p.x} cy={p.y} r={b.range} fill={b.powered && b.connected ? '#67cad016' : '#b7784910'} stroke={b.powered && b.connected ? '#81e6d95c' : '#b99b6666'} strokeWidth=".06" strokeDasharray=".18 .12"/>; })}
            {networkVisible && activeLinks.map(link => { const a = center(link.fromCell), b = center(link.toCell); return <polyline key={`link-${link.slot}`} points={`${a.x},${a.y} ${b.x},${a.y} ${b.x},${b.y}`} fill="none" className={`network-wire ${link.kind === 1 ? 'network-power' : 'network-compute'} ${link.powered ? '' : 'network-offline'}`} opacity={selectedLink?.slot === link.slot ? 1 : .7}/>; })}
            {selection.map(item => { const target = item.kind === 'unit' ? activeUnits.find(u => u.slot === item.slot) : item.kind === 'building' ? activeBuildings.find(b => b.slot === item.slot) : null; const cell = target?.cell ?? (item.kind === 'wall' ? item.slot : null); if (cell === null) return null; const p = target ? nativePoint(target) : center(cell); const size = item.kind === 'building' && target ? buildingSize(target.kind) : 1; return <rect key={selectionKey(item)} x={p.x - .58} y={p.y - .58} width={size + .16} height={size + .16} rx=".13" className="command-ring"/>; })}
            {networkVisible && activeBuildings.filter(b => b.owner === 0 && !b.powered && [2, 7, 8, 9].includes(b.kind)).map(b => { const p = center(b.cell); return <text key={`alert-${b.slot}`} x={p.x + .32} y={p.y - .35} className="command-power-alert">ϟ</text>; })}
            {activeUnits.filter(u => u.kind >= 3 && (networkVisible || selection.some(item => item.kind === 'unit' && item.slot === u.slot))).map(u => { const p = nativePoint(u); return <g key={`battery-${u.slot}`}><line x1={p.x - .32} y1={p.y + .53} x2={p.x + .32} y2={p.y + .53} stroke="#0b1824" className="command-battery"/><line x1={p.x - .32} y1={p.y + .53} x2={p.x - .32 + .64 * clamp(u.battery / Math.max(1, u.batteryMax), 0, 1)} y2={p.y + .53} stroke={u.covered || u.wired ? '#a1ece5' : '#e9bd5b'} className="command-battery"/></g>; })}
            {unit && <circle cx={nativePoint(unit).x} cy={nativePoint(unit).y} r={unit.range} fill="none" stroke="#e6ce7338" strokeWidth=".04" strokeDasharray=".1 .15"/>}
            {wallDraft.map(cell => { const p = center(cell); return <rect key={`draft-${cell}`} x={p.x - .46} y={p.y - .46} width=".92" height=".92" fill="#e8b95a50" stroke="#f3d783" strokeWidth=".04"/>; })}
            {linkStart !== null && previewCell && <polyline points={`${center(linkStart).x},${center(linkStart).y} ${previewCell.x},${center(linkStart).y} ${previewCell.x},${previewCell.y}`} fill="none" stroke={tool === 'power' ? '#f2c05d' : '#91e6e2'} className="network-preview"/>}
            {previewCell && anyTool && <g><rect x={previewCell.x - .48} y={previewCell.y - .48} width={previewSize - .04} height={previewSize - .04} fill={placementBlocked ? '#e4826333' : '#91dfc227'} stroke={placementBlocked ? '#ffae89' : '#cef0d2'} strokeWidth=".05"/>{previewImage && (previewAsset
              ? <CommandPlacementPreview asset={previewAsset} x={previewCell.x + previewSpriteOffset} y={previewCell.y + previewSpriteOffset} footprint={previewSize} fallback={previewImage} fallbackSize={previewSpriteSize}/>
              : <image href={previewImage} x={previewCell.x + previewSpriteOffset - previewSpriteSize / 2} y={previewCell.y + previewSpriteOffset - previewSpriteSize / 2} width={previewSpriteSize} height={previewSpriteSize} className="command-ghost"/>)}{tool === 'skill' && <circle cx={previewCell.x} cy={previewCell.y} r={unit?.kind === 1 ? 1.8 : 3.1} fill="#c499e426" stroke="#dfb4ef" strokeWidth=".055"/>}</g>}
            {gesture?.kind === 'select' && <rect x={Math.min(gesture.start.x, gesture.end.x)} y={Math.min(gesture.start.y, gesture.end.y)} width={Math.abs(gesture.end.x - gesture.start.x)} height={Math.abs(gesture.end.y - gesture.start.y)} fill="#a4e0ca14" stroke="#cef4c8" strokeWidth=".04"/>}
          </svg>
        </div>
      </div> : null}
      <div className="command-vignette"/>
      <header className="command-topline"><button className="command-brand" onClick={() => void openHub()} aria-label="打开行动大厅"><img src={ASSETS + 'ui-v3/operation-seal.svg'} alt=""/><span><strong>编译防线</strong><small>{r.ready ? 'COMMAND NETWORK / LIVE' : 'REALTIME STRATEGY'}</small></span></button>
        {r.ready && <div className="command-resource-strip"><button className="command-resource" onClick={() => setDrawer('resources')} aria-label="查看金币与资源网络"><Coins size={21}/><div><strong>{number(s.credits)}</strong><small>+{rate(s.incomePerSecond)} /s CREDITS</small></div></button><button className="command-resource" onClick={() => setDrawer('resources')} aria-label="查看算力与机架"><Cpu size={21}/><div><strong>{number(s.compute)}</strong><small>+{rate(s.production)} /s · {number(s.capacity)} CAP</small></div></button><button className={`command-resource ${s.powerGenerated < s.powerDemand ? 'is-low' : ''}`} onClick={() => setDrawer('resources')} aria-label="查看电力供需"><Zap size={21}/><div><strong>{number(s.powerGenerated)}<span style={{ fontSize: 12 }}> / {number(s.powerDemand)}</span></strong><small>POWER / LOAD</small></div></button><button className="command-resource" onClick={() => setDrawer('tech')} title="查看科技解锁"><Boxes size={21}/><div><strong>T{s.tech}</strong><small>RESEARCH</small></div></button></div>}
        <nav className="command-top-actions"><button onClick={() => { lobbyFromHub.current = false; setDrawer('lobby'); }} title="玩家对战与基地攻防准备室" aria-label="对战准备室"><Users size={18}/></button><button onClick={() => setDrawer('help')} title="指挥手册 / H" aria-label="指挥手册"><CircleHelp size={18}/></button>{r.ready && <><button className="command-optional" onClick={() => issue(COMMAND.speed())} disabled={!usable} title="切换速度">{s.speed}×</button><button onClick={() => void r.togglePause()} disabled={r.busy || gameOver} aria-label={r.paused ? '继续战斗' : '暂停战斗'}>{r.paused ? <Play size={17}/> : <Pause size={17}/>}</button></>}{!standalone && <a href="/" title="返回编辑器"><ArrowLeft size={17}/></a>}</nav>
      </header>
      {r.ready && <>
        <button className={`command-wave-button ${s.phase === 1 ? 'is-active' : ''}`} disabled={!usable || s.phase !== 0} onClick={() => issue(COMMAND.nextWave())}><span><small>WAVE {String(s.wave).padStart(2, '0')} / SECTOR {s.level}</small><strong>{s.phase === 1 ? '阻止 BUG 入侵' : '发起下一波'}</strong></span><ArrowRight size={18}/><kbd>N</kbd></button>
        <div className="command-wave-status">CORE {number(commandCore?.hp ?? s.baseHP)} · {s.activeEnemies} HOSTILES</div>
        <nav className="command-tool-rail" aria-label="地图指挥工具">
          <button className={tool === 'select' ? 'is-active' : ''} onClick={() => { setTool('select'); setLinkStart(null); }} title="选择 / 拖框多选" aria-label="选择工具"><MousePointer2 size={18}/></button>
          <button className={tool === 'move' ? 'is-active' : ''} onClick={() => useTool('move')} title="移动 / A" aria-label="下达移动命令"><Move size={19}/><small>A</small></button>
          <button className={tool === 'power' ? 'is-active' : ''} onClick={() => useTool('power')} title="连接电力线 / L" aria-label="连接电力线"><Zap size={18}/><small>L</small></button>
          <button className={tool === 'compute' ? 'is-active' : ''} onClick={() => useTool('compute')} title="连接算力线 / C" aria-label="连接算力线"><Cable size={19}/><small>C</small></button>
          <button className={tool === 'wall' ? 'is-active' : ''} onClick={() => useTool('wall')} title="拖建 cudad 护城河 / W" aria-label="修建 cudad 护城河"><Grid2X2 size={18}/><small>W</small></button>
          <button className={tool === 'shield' ? 'is-active' : ''} onClick={() => useTool('shield')} title="拓扑封闭区域充盾 / F" aria-label="为闭环区域充盾"><Shield size={18}/><small>F</small></button>
          <div className="command-tool-divider"/><button onClick={changeOverlay} title={`覆盖显示：${['隐藏', '网络', '网络与地形网格'][overlay]} / G`} aria-label="切换网络覆盖"><Layers size={18}/><small>G</small></button><button onClick={() => setDrawer('tech')} title="科技树" aria-label="科技树"><Boxes size={18}/></button>
        </nav>
        <section className="command-minimap"><header><span><Activity size={11}/> TACTICAL MAP</span><button title="全图 / Home" aria-label="回到全图" onClick={() => { setZoom(1); setPan({ x: 0, y: 0 }); }}><Home size={12}/></button></header><svg viewBox={`0 0 ${GRID_WIDTH} ${GRID_HEIGHT}`} role="img" aria-label="战场小地图，点击定位" onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); minimapMove(event); }} onPointerMove={event => { if (event.buttons === 1) minimapMove(event); }}>
          {s.terrain.map((terrain, cell) => <rect key={cell} x={cell % GRID_WIDTH} y={Math.floor(cell / GRID_WIDTH)} width="1.05" height="1.05" fill={TERRAIN_COLORS[terrain] ?? '#42634e'}/>)}
          {wallCells.map(cell => <rect key={`w${cell}`} x={cell % GRID_WIDTH} y={Math.floor(cell / GRID_WIDTH)} width="1" height="1" fill="#c9b37d"/>)}
          {activeBuildings.map(b => <rect key={`b${b.slot}`} x={b.cell % GRID_WIDTH + .1} y={Math.floor(b.cell / GRID_WIDTH) + .1} width={buildingSize(b.kind) - .2} height={buildingSize(b.kind) - .2} fill={b.owner ? '#ee7958' : b.kind === 1 ? '#f6daa1' : '#92dac9'}/>)}
          {activeUnits.map(u => <circle key={`u${u.slot}`} cx={u.cell % GRID_WIDTH + .5} cy={Math.floor(u.cell / GRID_WIDTH) + .5} r=".3" fill={u.owner ? '#f68972' : '#c5f1f1'}/>)}
          {activeEnemies.map(enemy => <circle key={`e${enemy.slot}`} cx={enemy.cell % GRID_WIDTH + .5} cy={Math.floor(enemy.cell / GRID_WIDTH) + .5} r=".32" fill="#ff7659"/>)}
          <rect x={(WORLD_WIDTH - WORLD_WIDTH / zoom) / 2 - MAP_MARGIN - pan.x / Math.max(1, boardRef.current?.clientWidth ?? 1280) * WORLD_WIDTH / zoom} y={(GRID_HEIGHT - GRID_HEIGHT / zoom) / 2 - pan.y / Math.max(1, boardRef.current?.clientHeight ?? 720) * GRID_HEIGHT / zoom} width={WORLD_WIDTH / zoom} height={GRID_HEIGHT / zoom} fill="none" stroke="#f0e6b9" strokeWidth=".18"/>
        </svg><footer><span>{overlay === 0 ? '地形' : overlay === 1 ? '网络覆盖' : '网络 + 网格'}</span><span><button onClick={() => changeZoom(-.2)} aria-label="缩小地图">−</button>{zoom.toFixed(1)}×<button onClick={() => changeZoom(.2)} aria-label="放大地图">+</button></span></footer></section>
        <span className="command-map-coordinates">{hover !== null ? `${cellName(hover)} · ${TERRAIN_NAMES[s.terrain[hover]] ?? '地形'}` : '32 × 20 / OPEN BATTLEFIELD'} · {r.connected ? 'LINK ONLINE' : 'RECONNECTING'}</span>
        {(unit || building || selectedLink || selectedWall !== null) && <section className="command-selection">
          <header className="command-selection-heading">{unit ? <img className={version === 5 && unit.kind < 3 ? 'command-turret-portrait' : 'command-portrait'} src={operatorArt(unit.kind)} alt=""/> : building ? <img src={buildingArt(building.kind)} alt=""/> : selectedLink ? <Cable size={35}/> : <img src={buildingArt(10)} alt=""/>}<div><small>{selection.length > 1 ? `${selection.length} SELECTED` : selectedGpu ? 'GPU BAY / ' + (selectedGpu.bay + 1) : unit ? unit.kind < 3 ? 'FIXED TURRET' : 'MOBILE INTELLIGENCE' : building ? `STRUCTURE / ${buildingSize(building.kind)}×${buildingSize(building.kind)} / LV.${building.tier}` : selectedLink ? 'NETWORK LINK' : 'TOPOLOGY BARRIER'}</small><h2>{selectedGpu ? GPUS[selectedGpu.model - 1].name : unit ? OPERATORS[unit.kind - 1].name : building ? selectedMeta?.name : selectedLink ? selectedLink.kind === 1 ? '电力线' : '算力线' : 'cudad 护城河'}</h2></div><button aria-label="清除选择" onClick={() => { setSelection([]); setGpuSlot(null); }}><X size={14}/></button></header>
          {selection.length > 1 && <div className="command-selection-group">{selection.slice(0, 16).map(item => { const groupUnit = activeUnits.find(u => item.kind === 'unit' && u.slot === item.slot); const groupBuilding = activeBuildings.find(b => item.kind === 'building' && b.slot === item.slot); return <button key={selectionKey(item)} onClick={() => select([item])} aria-label={`单独选择 ${groupUnit ? OPERATORS[groupUnit.kind - 1].name : '移动基站'}`}><img className={version === 5 && groupUnit && groupUnit.kind < 3 ? 'command-turret-portrait' : undefined} src={groupUnit ? operatorArt(groupUnit.kind) : buildingArt(groupBuilding?.kind ?? 7)} alt=""/></button>; })}</div>}
          {unit && <><div className="command-selection-stats"><div>耐久<strong>{number(unit.hp)}</strong></div><div>攻击耗算<strong>{number(unit.attackCost)}</strong></div><div>网络<strong>{unit.wired ? '有线驻守' : unit.covered ? '无线覆盖' : '离线电池'}</strong></div><div>技能<strong>{unit.skillCooldown > 0 ? Math.ceil(unit.skillCooldown) + 's' : number(unit.skillCost)}</strong></div></div>{unit.kind >= 3 && <><div className="command-unit-battery"><i style={{ width: `${clamp(unit.battery / Math.max(1, unit.batteryMax), 0, 1) * 100}%` }}/></div><p><BatteryCharging size={11} style={{ verticalAlign: 'middle' }}/> 随身算力 {number(unit.battery)} / {number(unit.batteryMax)}{unit.covered || unit.wired ? ' · 网络补充中' : ' · 支持覆盖外作战'}</p></>}<div className="command-selection-actions"><button title="升级 / E" disabled={!usable} onClick={upgrade}><ArrowUp size={13}/>升级</button><button title="技能 / Q" className="command-skill-action" disabled={!usable || unit.skillCooldown > 0} onClick={() => useTool('skill')}><Crosshair size={13}/>技能</button><button title="目标策略 / T" disabled={!usable} onClick={() => issue(COMMAND.targetMode(unit.slot, (unit.targetMode + 1) % 3))}><Target size={13}/>{['前锋', '强敌', '近处'][unit.targetMode]}</button><button title="回收 / R" disabled={!usable} aria-label="回收所选单位" onClick={recycle}><Trash2 size={13}/></button></div><p className="command-selection-note">{unit.kind < 3 ? '固定炮台，通过算力线连接数据中心。' : unit.wired ? '驻守有线接入点；选择算力线并回收后可移动。' : '右键目的地，或 A 后左键；返回基站覆盖补充电池。'}</p></>}
          {building && <><div className="command-selection-stats"><div>耐久<strong>{number(building.hp)}</strong></div><div>阵营<strong>{building.owner ? '对手' : '我方'}</strong></div><div>供电<strong>{building.demand > 0 ? `${number(building.demand)} / ${building.powered ? '在线' : '断电'}` : building.supply > 0 ? `+${number(building.supply)}` : '自供能'}</strong></div><div>算力接入<strong>{building.connected ? '已连接' : '未连接'}</strong></div></div>
            {building.kind === 2 && <><div className="command-rack">{Array.from({ length: 4 }, (_, bay) => { const installed = rack.find(g => g.bay === bay && g.model > 0); return <button key={bay} className={`${installed ? 'is-filled' : ''} ${installed?.slot === gpuSlot ? 'is-selected' : ''}`} aria-label={`机架 ${bay + 1} ${installed ? GPUS[installed.model - 1].name : '空槽'}`} title={installed ? `${GPUS[installed.model - 1].name} · +${rate(installed.rate)}/s` : '点击安装当前显卡'} onClick={() => installed ? setGpuSlot(installed.slot) : installGpu(building.slot, bay)} onDragOver={event => { if (event.dataTransfer.types.includes(TACTICAL_CARD_MIME)) event.preventDefault(); }} onDrop={event => { event.preventDefault(); event.stopPropagation(); setDragCard(null); try { const payload = JSON.parse(event.dataTransfer.getData(TACTICAL_CARD_MIME)) as DragCard; if (payload.kind === 'hardware' && payload.id >= 1 && payload.id <= 7 && !installed) installGpu(building.slot, bay, payload.id); } catch { /* Invalid drag payload is ignored. */ } }}>{installed ? <img src={ASSETS + GPUS[installed.model - 1].image} alt=""/> : <Plus size={16}/>}<small>{bay + 1}</small></button>; })}</div><p>{selectedGpu ? `产能 +${rate(selectedGpu.rate)}/s · ${GPUS[selectedGpu.model - 1].vram} GB ${GPUS[selectedGpu.model - 1].memory}` : 'I 显卡牌组 · 1–7 选卡 · Enter 安装'}</p></>}
            {building.kind === 2 && selectedGpu && <p className="command-hardware-source">{referenceGpu.caption}<br/>{referenceGpu.vram} GB {referenceGpu.memory} · <a href={referenceGpu.sourceUrl} target="_blank" rel="noreferrer">厂商型号与图片 ↗</a></p>}
            {building.kind === 7 && <p className={!building.powered || !building.connected ? 'command-warning' : ''}>覆盖半径 {rate(building.range)} 格。机载供电，自动连接 14 格内有效数据中心；右键移动保持回传。</p>}
            {building.kind === 8 && <p>科技 T{s.tech} · 双网接入后 E 升级科技，解锁更强显卡、电站与 AI。</p>}
            <div className="command-selection-actions"><button disabled={!usable || building.owner !== 0} onClick={upgrade} title="升级 / E"><ArrowUp size={13}/>{building.kind === 8 ? '科研' : '升级'}</button>{building.kind === 2 && <button onClick={() => { setDeck('gpus'); setDeckOpen(true); }}><Cpu size={13}/>显卡</button>}{!selectedGpu && <button disabled={!usable || building.owner !== 0} onClick={() => issue(COMMAND.repairBuilding(building.slot))} title="修复建筑"><Wrench size={13}/>修复</button>}<button disabled={!usable || building.owner !== 0 || (building.kind === 1 && !selectedGpu)} onClick={recycle} title="回收 / R" aria-label="回收选中对象"><Trash2 size={13}/></button></div>
          </>}
          {(building || unit) && <div className="command-local-links">{activeLinks.filter(link => link.fromCell === (building?.cell ?? unit?.cell) || link.toCell === (building?.cell ?? unit?.cell)).map(link => <button key={link.slot} className={link.powered ? '' : 'is-offline'} title={`选择${link.kind === 1 ? '电力' : '算力'}线 ${link.slot + 1}，再按 R 拆除`} onClick={() => select([{ kind: 'link', slot: link.slot }])}>{link.kind === 1 ? <Zap size={10}/> : <Cable size={10}/>}#{link.slot + 1}{link.powered ? '' : ' 离线'}</button>)}</div>}
          {selectedLink && <><p>{cellName(selectedLink.fromCell)} → {cellName(selectedLink.toCell)} · {selectedLink.powered ? '在线' : '供给中断'}<br/>{selectedLink.kind === 1 ? '电力网络为建筑供能，断线会中断下游供应。' : '拆线会断开下游算力，有线 AI 随之解除驻守。'}</p><div className="command-selection-actions"><button disabled={!usable} onClick={recycle}><Unplug size={13}/>拆除连线 <kbd>R</kbd></button></div></>}
          {selectedWall !== null && <><p>把防线围成闭环后，内部会形成可充盾的拓扑区域。入口被破坏将重新计算保护范围。</p><p className="command-shield-text">封闭区域 {s.closedArea} 格 · 护盾 {number(s.shield)} / {number(s.shieldCapacity)} · {s.shieldAuto ? '自动充能' : '停止充能'}</p><div className="command-selection-actions"><button disabled={!usable} onClick={() => useTool('shield')}><Shield size={13}/>护盾 F</button><button disabled={!usable} onClick={recycle}><Trash2 size={13}/>拆除 R</button></div></>}
        </section>}
        <section className={`command-deck ${deckOpen ? '' : 'is-collapsed'}`} aria-label="建造与部署牌组">
          <nav className="command-deck-tabs"><button className={deck === 'buildings' ? 'is-active' : ''} onClick={() => chooseDeck('buildings')}><Factory size={14}/>建筑<kbd>B</kbd></button><button className={deck === 'units' ? 'is-active' : ''} onClick={() => chooseDeck('units')}><Flag size={14}/>战斗单位<kbd>U</kbd></button><button className={deck === 'gpus' ? 'is-active' : ''} onClick={() => chooseDeck('gpus')}><Cpu size={14}/>显卡<kbd>I</kbd></button><button className="command-fold" aria-label={deckOpen ? '收起牌组' : '展开牌组'} title="收起或展开 / V" onClick={() => setDeckOpen(value => !value)}>{deckOpen ? <ChevronDown size={17}/> : <ChevronUp size={17}/>}</button></nav>
          {deckOpen && <><p className="command-card-info">{deck === 'buildings' ? '风随高地 · 水依河岸 · 煤依矿层 · 核电需要科技｜拖卡到地面建造' : deck === 'units' ? '编译器驻守，AI 推进 · 算力线固定接入，无线与电池支持机动作战' : building?.kind === 2 ? `当前数据中心 ${cellName(building.cell)} · 每个机架独立装卡` : '选择地图上的数据中心，再安装显卡；真实照片与规格沿用官方来源'}</p>{deck === 'gpus' && <p className="command-gpu-reference"><span>{GPUS[gpuModel - 1].name} · {GPUS[gpuModel - 1].caption} · {GPUS[gpuModel - 1].vram} GB {GPUS[gpuModel - 1].memory}</span><a href={GPUS[gpuModel - 1].sourceUrl} target="_blank" rel="noreferrer">厂商资料 ↗</a></p>}<div className="command-deck-body">
            {deck === 'buildings' ? buildCards.map((meta, index) => <button key={meta.id} className={`command-build-card ${tool === 'build' && card === meta.id ? 'is-selected' : ''} ${s.tech < meta.tech ? 'is-locked' : ''}`} title={meta.description + ` · 占地 ${buildingSize(meta.id)}×${buildingSize(meta.id)} · 科技 ${meta.tech}`} onClick={() => chooseCard('building', meta.id)} draggable onDragStart={event => { const value: DragCard = { kind: 'building', id: meta.id }; event.dataTransfer.setData(TACTICAL_CARD_MIME, JSON.stringify(value)); event.dataTransfer.effectAllowed = 'copy'; setDragCard(value); }} onDragEnd={() => setDragCard(null)}><header><kbd>{index + 1}</kbd><span>{s.tech < meta.tech ? <><LockKeyhole size={10}/>T{meta.tech}</> : <><Coins size={10}/>{meta.cost}</>}</span></header><img src={buildingArt(meta.id)} alt="" draggable={false}/><div className="command-card-copy"><small>{BUILD_ART[meta.id].toUpperCase()}</small><strong>{meta.name}</strong><p>{meta.description}</p></div></button>)
              : deck === 'units' ? OPERATORS.map((op, index) => <div className="command-card-wrap" key={op.id}><TacticalCard variant="operator" id={op.id} name={op.name} subtitle={op.id < 3 ? '固定炮台' : '机动 AI'} code={['PRECISION', 'DEBUG', 'TIDE', 'RESTORE'][index]} cost={op.cost} energyCost={op.attackCost} artwork={OLD_ART + OP_ART[index] + '.png'} accent={op.color} selected={tool === 'deploy' && card === op.id} affordable={s.credits >= op.cost} lockedReason={s.tech < UNIT_TECH[index + 1] ? `需要 T${UNIT_TECH[index + 1]} 科技（当前 T${s.tech}）` : undefined} hotkey={String(index + 1)} details={[op.description, op.id < 3 ? '固定建筑式炮台，连接数据中心算力线后开火。' : '无线覆盖补充随身算力；离开覆盖后可消耗电池继续攻击和施法。', `科技要求 T${UNIT_TECH[index + 1]} · ${op.skillName} 消耗 ${op.skillCost} 算力`, op.skillDescription]} onChoose={() => chooseCard('operator', op.id)} onDragState={dragging => setDragCard(dragging ? { kind: 'operator', id: op.id } : null)}/>{s.tech < UNIT_TECH[index + 1] && <span className="command-card-lock"><LockKeyhole size={11}/> 科技 T{UNIT_TECH[index + 1]}</span>}</div>)
                : GPUS.map((gpu, index) => <div className="command-card-wrap" key={gpu.id}><TacticalCard variant="hardware" id={gpu.id} name={gpu.name.replace('GeForce ', '').replace('NVIDIA ', '')} subtitle={`${gpu.vram} GB ${gpu.memory}`} code={'COMPUTE / ' + String(gpu.id).padStart(2, '0')} cost={gpu.cost} production={gpu.rate} artwork={gpuArt(gpu.id)} photo={ASSETS + gpu.image} accent="#b5dabd" selected={gpuModel === gpu.id} affordable={s.credits >= gpu.cost} lockedReason={s.tech < GPU_TECH[index + 1] ? `需要 T${GPU_TECH[index + 1]} 科技（当前 T${s.tech}）` : undefined} hotkey={String(index + 1)} details={[gpu.caption, `真实规格：${gpu.vram} GB ${gpu.memory}；游戏电力需求 ${GPU_GAMEPLAY[index].power}`, `需要科技 T${GPU_TECH[index + 1]}，装入地图上的数据中心。`, '必须接通电力；实际产能由原生供电与机架状态决定。']} onChoose={() => chooseCard('hardware', gpu.id)} onDragState={dragging => setDragCard(dragging ? { kind: 'hardware', id: gpu.id } : null)}/>{s.tech < GPU_TECH[index + 1] && <span className="command-card-lock"><LockKeyhole size={11}/> 科技 T{GPU_TECH[index + 1]}</span>}</div>)}
          </div></>}
        </section>
        {intention && <div className="command-intention"><Crosshair size={16}/><span>{intention}</span><button aria-label="取消当前工具" onClick={cancel}><X size={14}/></button><small>ESC</small></div>}
        {tutorial && !selection.length && <aside className="command-tutorial"><header>BUILD YOUR NETWORK <button aria-label="收起入门目标" onClick={() => setTutorial(false)}><X size={12}/></button></header>{[
          [activeBuildings.some(b => b.kind === 9), '在矿脉 / 煤层上建资源采集器'], [activeBuildings.some(b => b.kind >= 3 && b.kind <= 6), '建造风电，高地提供额外产能'], [activeBuildings.some(b => b.kind === 2), '建造数据中心，L 连接电力'], [s.gpus.some(g => g.model > 0), '点击数据中心，I 安装显卡'], [ownedUnits.length > 0, 'U 部署单位，C 连接算力'], [activeBuildings.some(b => b.kind === 7), '部署移动基站，让 AI 前进'],
        ].map(([done, label], index) => <div className={done ? 'is-done' : ''} key={index}>{done ? <Check size={12}/> : <span style={{ width: 12, fontFamily: 'monospace' }}>{index + 1}</span>}{label}</div>)}<small>空指针与死锁会破坏你的基础设施。布防完成后 N 开始入侵。</small></aside>}
        {noticeOpen && settings.showNotices && r.notice && <div className="command-notice" role="status">{r.notice}</div>}
        {r.paused && !gameOver && <section className="command-pause-overlay"><h2>战场暂停</h2><button onClick={() => void r.togglePause()}><Play size={16} style={{ verticalAlign: 'middle', marginRight: 8 }}/>继续行动 · Space</button></section>}
        {gameOver && !inspectResult && !finishingDestruction && <section className="command-end"><img src={ASSETS + `ui-v3/${s.phase === 2 ? 'victory' : 'defeat'}-stamp.svg`} alt=""/><h2>{campaignComplete ? '战役完成' : s.phase === 2 ? '网络守住了' : '指挥核心失守'}</h2><p>{campaignComplete ? '三个战区已守住。你的网络连成了完整的防线。' : '建设、供电、算力与防线，共同决定你的战场。'}<br/>可以留在地图复盘，也可以重新部署。</p><div>{s.phase === 2 && s.level < 3 && <button onClick={() => { if (r.send(COMMAND.nextLevel())) { setInspectResult(false); setSelection([]); } }}>下一战区</button>}<button onClick={() => void openHub()}>返回大厅</button><button onClick={() => void start()}>重新部署</button><button onClick={() => setInspectResult(true)}>查看战场</button></div></section>}
      </>}
      {(r.error || (r.ready && !r.nativeReady)) && <section className="command-connection" role="alert"><strong>{r.error ? '指挥连接需要恢复' : '正在同步原生战场…'}</strong><p>{r.error || '地图、经济与战斗状态由原生引擎同步。'}</p>{r.error && <button onClick={() => r.ready ? r.reconnect() : r.workspace ? void start() : location.reload()}>重新接入</button>}</section>}
    </div>
    {((!r.ready && !drawer) || drawer === 'hub') && <CommandHub state={s} active={r.ready} available={Boolean(r.workspace)}
      busy={r.busy || hubTransition || Boolean(pendingSector)} connected={r.connected && r.nativeReady}
      unlocked={Math.max(r.savedUnlocked, s.unlocked)} settings={settings} onSettings={setSettings}
      error={r.error} onReconnect={() => r.ready ? r.reconnect() : r.workspace ? void start() : location.reload()}
      onContinue={continueOperation} onDeploy={level => void start(level)} onClose={r.ready ? continueOperation : undefined}
      onMultiplayer={() => { lobbyFromHub.current = true; setDrawer('lobby'); }}/>} 
    {drawer === 'resources' && <CommandResourcePanel state={s} connected={r.connected && r.nativeReady} onClose={() => setDrawer(null)} onLocate={locateResource} onResearch={() => setDrawer('tech')}/>}
    {drawer === 'lobby' && <CommandLobby onClose={() => setDrawer(lobbyFromHub.current ? 'hub' : null)}/>}
    {(drawer === 'help' || drawer === 'tech') && <div className="command-modal-backdrop" onClick={() => setDrawer(null)}><section ref={modalRef} tabIndex={-1} className="command-modal" role="dialog" aria-modal="true" aria-label={drawer === 'help' ? '指挥手册' : '科技树'} onClick={event => event.stopPropagation()} onKeyDown={event => { if (event.key === 'Escape') { setDrawer(null); return; } if (event.key !== 'Tab') return; const items = modalRef.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],summary'); if (!items?.length) return; const first = items[0], last = items[items.length - 1]; if (event.shiftKey && (document.activeElement === first || document.activeElement === modalRef.current)) { event.preventDefault(); last.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); } }}><button className="command-modal-close" aria-label="关闭浮层" onClick={() => setDrawer(null)}><X size={21}/></button><header><span>CODE SENTINELS / COMMAND ARCHIVE</span><h2>{drawer === 'help' ? '把整片地图，变成你的防线。' : `科技网络 · 当前 T${s.tech}`}</h2></header>
      {drawer === 'help' ? <><div className="command-guide-flow"><article><b>01 / INFRASTRUCTURE</b><strong>建设发电与算力网络</strong><p>矿脉或煤层上的采集器持续获得金币。风机陆地可建、高地增产；水电依水域，煤电依煤层，核电需要高级科技与冷却水。建造数据中心后，L 从电站拉电线，I 把显卡装进它的真实机架。</p></article><article><b>02 / TACTICAL MOBILITY</b><strong>让 AI 走出基地</strong><p>C 从数据中心连接炮台或驻守 AI。编译器固定，有线 AI 不能移动。移动基站机载供电，自动回传 14 格内有效数据中心并提供 6 格覆盖；AI 离开覆盖仍能用随身算力攻击、释放技能。</p></article><article><b>03 / TOPOLOGY & TECH</b><strong>经营可守可攻的领地</strong><p>W 拖动建造 cudad 护城河，围成闭环且内部包含供电数据中心后，自动消耗算力充盾，F 可切换自动充能。敌人拆开墙体会重算保护范围。建设研究院、接入电力与算力并升级，解锁更强显卡、AI 与核电。</p></article></div><div className="command-keymap">{COMMAND_HOTKEYS.map(([key, text]) => <div key={key}><kbd>{key}</kbd><span>{text}</span></div>)}</div><p className="command-help-note">双击单位可选择同类型单位；选择连线按 R 可拆除。右键操作均可由 A + 左键替代，缩放和小地图可快速定位。金币和算力消耗、可达路径、供电与网络连接以原生结果为准。<br/>DeepSeek 与 GPT 延续已核实的真实网络人物形象与图生视频帧动画：<a href="https://www.bilibili.com/video/BV1EvKK6NEoi/" target="_blank" rel="noreferrer">DeepSeek 来源</a> · <a href="https://www.youtube.com/watch?v=xASRX37IIiY" target="_blank" rel="noreferrer">GPT 来源</a>。真实 GPU 照片来自 NVIDIA / MSI / PNY，游戏成本与产能为独立平衡值。</p><details className="command-source-drawer"><summary>BUG 类型与首领概念来源</summary><p>以下链接对应真实软件错误类型；敌人造型和战斗行为是游戏化演绎。</p><div>{BUGS.map(bug => <a key={bug.id} href={`https://cwe.mitre.org/data/definitions/${bug.cwe.replace('CWE-', '')}.html`} target="_blank" rel="noreferrer"><strong>{bug.name}</strong><span>{bug.english} · {bug.cwe} ↗</span></a>)}</div></details></>
        : <><div className="command-tech-tree">{[0, 1, 2, 3].map(level => <article key={level} className={s.tech >= level ? 'is-unlocked' : ''}><small>{s.tech >= level ? 'UNLOCKED' : 'RESEARCH REQUIRED'}</small><h3>科技 T{level}</h3><img src={level === 0 ? buildingArt(3) : level === 1 ? buildingArt(8) : level === 2 ? ASSETS + GPUS[3].image : buildingArt(6)} alt=""/><p>{GPUS.filter((_, index) => GPU_TECH[index + 1] === level).map(gpu => gpu.name.replace('GeForce ', '').replace('NVIDIA ', '')).join(' / ') || '基础设施提升'}</p><p>{OPERATORS.filter((_, index) => UNIT_TECH[index + 1] === level).map(op => op.name).join(' · ')}</p><p>{BUILDINGS.filter(meta => meta.tech === level && meta.id > 1).map(meta => meta.name).join(' · ')}</p></article>)}</div><div className="command-tech-note"><Boxes size={23}/><span>{researchLab ? `研究院位于 ${cellName(researchLab.cell)}。科研需要电力、算力与建设经费；升级结果由研究院发布。` : '在建筑牌组中建造研究院，使用 L 接入电力、C 接入数据中心算力。'}</span><button disabled={!usable || !researchLab} onClick={() => { if (issue(COMMAND.research())) setDrawer(null); }}>升级科技 <ArrowUp size={13} style={{ verticalAlign: 'middle' }}/></button></div><p className="command-help-note">联网入口预留玩家对战与基地攻防：房间、席位、阵营归属、指令序列及服务器权威接口。对战准备室可建立真实房间，战斗同步接入前不会伪装成已经可联机对战。</p></>}
    </section></div>}
  </main>;
}
