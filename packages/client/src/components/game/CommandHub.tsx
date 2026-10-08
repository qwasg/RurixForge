import { useEffect, useRef, useState } from 'react';
import { ArrowRight, BookOpen, Boxes, Check, ChevronRight, Cpu, Flag, LockKeyhole, Radio, Settings2, Shield, Volume2, X } from 'lucide-react';
import { BUILDINGS, GPUS as GPU_GAMEPLAY, UNITS, type V4State } from '@/lib/sentinelsV4';
import { BUGS, GPUS, OPERATORS } from '@/lib/sentinelsV2';
import { COMMAND_HOTKEYS } from '@/lib/useCommandHotkeys';
import { DEFAULT_COMMAND_SETTINGS, playCommandTone, type CommandSettings } from '@/lib/commandSettings';
import './command-hub.css';

const BASE = '/games/code-sentinels/';
const UNIT_TACTICS = [
  ['快速精准射击，依照目标策略拦截进入射程的 BUG。', '在目标区域实施穿透打击，并施加标记与减速。'],
  ['普通攻击具有范围溅射，适合压制聚集的 BUG。', '范围穿透打击，对内存泄漏造成更高伤害，并施加标记与减速。'],
  ['普通攻击施加减速与标记，为防线争取输出时间。', '释放推理潮汐，造成范围穿透伤害，并延长标记与减速。'],
  ['以算力驱动支援射击；无线网络和随身电池支持机动作战。', '修复目标附近的友方建筑与单位，解除单位干扰，同时压制附近敌人。'],
] as const;
const BUG_TACTICS = [
  '沿可达路线逼近核心，遇到阻挡会攻击防御设施。保持基础防线连续。',
  '未被标记时持续窃取算力并恢复生命；用标记和集中火力压制它。',
  '周期性加速冲刺。使用减速和有纵深的火力覆盖阻止突进。',
  '周期性干扰附近单位与建筑，可能使电网和算力网络失效。',
  '初代个体被击破后分裂为两个子体，保留应对后续目标的火力。',
  '森林首领会持续威胁附近单位与建筑；保护核心，留出修复与算力余量。',
  '湿地首领兼具范围伤害与干扰。避免把关键供能设施集中在前线。',
  '高地首领会召唤子体并威胁周边设施，持续产能与前沿补给尤为关键。',
] as const;
const SECTORS = [
  { id: 1, name: '断点森林', code: 'BREAKPOINT / FOREST', art: 'forest', description: '在林间建立最初的电网，把矿脉、算力和防御连成整体。', terrain: '森林 · 矿脉 · 基础布防' },
  { id: 2, name: '泄漏湿地', code: 'MEMORY / MARSH', art: 'marsh', description: '沿水域铺设设施，守住脆弱通路，让算力跨越湿地。', terrain: '河道 · 水力发电 · 狭窄通路' },
  { id: 3, name: '递归高地', code: 'RECURSIVE / HIGHLAND', art: 'highland', description: '利用高地产能建立前沿网络，为更长的防线提供持续支援。', terrain: '高地 · 风力增产 · 前沿推进' },
] as const;
type Tab = 'operations' | 'archive' | 'settings';
type Archive = 'units' | 'buildings' | 'gpus' | 'bugs';
type Props = {
  state: V4State; active: boolean; busy: boolean; available: boolean; connected: boolean; unlocked: number;
  settings: CommandSettings; onSettings(settings: CommandSettings): void;
  error?: string; onReconnect(): void;
  onContinue(): void; onDeploy(level: number): void; onMultiplayer(): void; onClose?(): void;
};

export default function CommandHub(props: Props) {
  const { state: s, active, busy, available, connected, settings, onSettings } = props;
  const [tab, setTab] = useState<Tab>('operations'), [sector, setSector] = useState(s.level || 1);
  const [archive, setArchive] = useState<Archive>('units'), [entry, setEntry] = useState(0);
  const dialog = useRef<HTMLElement>(null), currentSector = SECTORS[sector - 1];
  const unlocked = Math.max(1, Math.min(3, Math.floor(props.unlocked)));
  const locked = sector > unlocked;
  useEffect(() => { const previous = document.activeElement as HTMLElement | null; dialog.current?.focus(); return () => previous?.focus(); }, []);
  const archiveItems = archive === 'units' ? OPERATORS.map((operator, index) => ({
    name: operator.name, code: index < 2 ? 'FIXED TURRET / 编译器' : 'MOBILE AI / 移动智能',
    image: BASE + 'ui-v3/art/' + ['operator-vscode', 'operator-pycharm', 'operator-deepseek', 'operator-gpt'][index] + '.png',
    description: UNITS[index].description, detail: UNIT_TACTICS[index][0], extra: `技能 / ${operator.skillName} · ${UNIT_TACTICS[index][1]}`,
    stats: [`部署 ${UNITS[index].cost} 金币`, `科技 T${UNITS[index].tech}`, `技能 ${UNITS[index].skillCost} 算力`], source: index === 2 ? 'https://www.bilibili.com/video/BV1EvKK6NEoi/' : index === 3 ? 'https://www.youtube.com/watch?v=xASRX37IIiY' : '',
  })) : archive === 'buildings' ? BUILDINGS.map(building => ({
    name: building.name, code: 'INFRASTRUCTURE / 基础设施', image: building.image, description: building.description,
    detail: building.id === 10 ? '按 W 拖动建设。闭环内部包含已供电的数据中心时，可消耗算力充能拓扑护盾。' : '在建筑牌组选择后放置到地形上。设施供能、连线和生产由战场实际网络决定。', extra: '',
    stats: [`建设 ${building.cost} 金币`, `科技 T${building.tech}`, `占地 ${building.id === 6 ? '3×3' : [1, 2, 4, 5, 8].includes(building.id) ? '2×2' : '1×1'}`], source: '',
  })) : archive === 'gpus' ? GPUS.map((gpu, index) => ({
    name: gpu.name, code: gpu.caption, image: BASE + gpu.image, description: `${gpu.vram} GB ${gpu.memory}。安装到数据中心的空闲机架，获得算力产能和储量。`,
    detail: 'GPU 必须随数据中心接入有余量的电网。图像与硬件规格保留厂商来源，建设成本和产能是游戏平衡数值。', extra: `储量 +${GPU_GAMEPLAY[index].capacity} · 需求 ${GPU_GAMEPLAY[index].power} 电力`,
    stats: [`安装 ${GPU_GAMEPLAY[index].cost} 金币`, `产能 +${GPU_GAMEPLAY[index].rate}/s`, `科技 T${GPU_GAMEPLAY[index].tech}`], source: gpu.sourceUrl,
  })) : BUGS.map(bug => ({ name: bug.name, code: bug.english + ' / ' + bug.cwe, image: BASE + bug.image,
    description: bug.description.split('。')[0] + '。', detail: BUG_TACTICS[bug.id], extra: '形象与战斗行为是基于软件错误概念的游戏化演绎。', stats: [bug.id < 5 ? '常规 BUG' : '战区首领', bug.cwe],
    source: `https://cwe.mitre.org/data/definitions/${bug.cwe.replace('CWE-', '')}.html`,
  }));
  const selectedEntry = archiveItems[Math.min(entry, archiveItems.length - 1)];
  const setting = <K extends keyof CommandSettings>(key: K, value: CommandSettings[K]) => onSettings({ ...settings, [key]: value });
  return <section className={`command-hub hub-${tab}`} ref={dialog} tabIndex={-1} role="dialog" aria-modal="true" aria-label="编译防线行动大厅"
    onKeyDown={event => {
      event.stopPropagation(); if (event.key === 'Escape') { if (tab !== 'operations') setTab('operations'); else if (!busy) props.onClose?.(); return; }
      if (event.key !== 'Tab') return;
      const items = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href],input,select'); if (!items?.length) return;
      if (event.shiftKey && (document.activeElement === items[0] || document.activeElement === dialog.current)) { event.preventDefault(); items[items.length - 1].focus(); }
      else if (!event.shiftKey && document.activeElement === items[items.length - 1]) { event.preventDefault(); items[0].focus(); }
    }}>
    <div className="hub-art"/>
    <header className="hub-top"><div className="hub-brand"><img src={BASE + 'ui-v3/operation-seal.svg'} alt=""/><div><strong>编译防线</strong><span>CODE SENTINELS / COMMAND</span></div></div><div className="hub-session"><i className={active && connected ? 'is-live' : ''}/>{active ? connected ? '当前战局在线' : '当前战局等待连接' : '行动准备就绪'}{active && <span>SECTOR {String(s.level).padStart(2, '0')} · WAVE {String(s.wave).padStart(2, '0')}</span>}</div>{props.onClose && <button className="hub-close" disabled={busy} onClick={props.onClose} aria-label="关闭行动大厅"><X size={22}/></button>}</header>
    <nav className="hub-tabs" aria-label="大厅导航">{([['operations', '行动', Flag], ['archive', '档案', BookOpen], ['settings', '设置', Settings2]] as const).map(([id, label, Icon]) => <button key={id} className={tab === id ? 'is-active' : ''} aria-pressed={tab === id} onClick={() => setTab(id)}><Icon size={17}/>{label}<span>{id.toUpperCase()}</span></button>)}</nav>
    {props.error && <div className="hub-error" role="alert"><span>{props.error}</span><button disabled={busy} onClick={props.onReconnect}>重新接入<ArrowRight size={14}/></button></div>}
    <div className="hub-content">
      {tab === 'operations' ? <>
        <div className="hub-hero"><div className="hub-hero-copy"><span className="hub-eyebrow">OPERATION / READY TO DEPLOY</span><h1>防线，<br/>从这里延伸。</h1><p>从一根电线，到一座算力要塞。<br/>让建设、能源与 AI，成为你的作战方式。</p>
          {active && <button className="hub-continue" disabled={busy || !connected} onClick={props.onContinue}><span><small>LIVE OPERATION · 仅当前运行</small><strong>{s.phase >= 2 ? '返回战场复盘' : '继续当前行动'}</strong></span><ArrowRight size={24}/></button>}
          <div className="hub-traits"><span><Shield size={15}/>经营与防御</span><span><Cpu size={15}/>实体算力网络</span><span><Boxes size={15}/>地形与科技</span></div>
        </div><aside className="hub-briefing"><span>SELECTED OPERATION / 0{sector}</span><h2>{currentSector.name}</h2><p>{currentSector.description}</p><small>{currentSector.terrain}</small><div className="hub-briefing-status">{locked ? <><LockKeyhole size={15}/>通关前一战区后解锁</> : <><Check size={15}/>已解锁 · 可以部署</>}</div>
          <button className="hub-deploy" disabled={busy || !available || locked} onClick={() => props.onDeploy(sector)}><span>{busy ? '连接战场中…' : active ? '重新部署此战区' : '开始单人行动'}</span><ArrowRight size={20}/></button>
          {active ? <p className="hub-save-note">重新部署将替换当前战局。返回大厅不会创建战局存档。</p> : <p className="hub-save-note">保存战区解锁进度；当前战局随本次运行保留。</p>}
        </aside></div>
        <section className="hub-operations" aria-label="战区任务"><header><span>CAMPAIGN / 战区任务</span><small>{unlocked} / 3 已解锁</small></header><div className="hub-sector-grid">{SECTORS.map(item => <button key={item.id} className={`hub-sector ${sector === item.id ? 'is-selected' : ''} ${item.id > unlocked ? 'is-locked' : ''}`} aria-pressed={sector === item.id} aria-label={`查看${item.name}${item.id > unlocked ? '，未解锁' : ''}`} onClick={() => setSector(item.id)}>
          <img src={BASE + 'ui-v3/art/mission-' + item.art + '.png'} alt=""/><span className="hub-sector-number">0{item.id}</span><div><small>{item.code}</small><strong>{item.name}</strong><span>{item.id > unlocked ? <><LockKeyhole size={12}/>未解锁</> : active && s.level === item.id ? '当前战区' : '可部署'}<ChevronRight size={16}/></span></div></button>)}</div></section>
        <button className="hub-multiplayer" onClick={props.onMultiplayer}><Radio size={22}/><span><strong>联机准备室</strong><small>创建房间、邀请同伴与准备席位 · PVP 战斗尚未开放</small></span><ArrowRight size={19}/></button>
      </> : tab === 'archive' ? <div className="hub-archive"><header className="hub-section-title"><span>INTELLIGENCE / FIELD ARCHIVE</span><h1>了解你的防线。</h1><p>单位、设施、硬件与 BUG 的战场资料。</p></header>
        <nav className="hub-archive-tabs" aria-label="档案分类">{([['units', '作战单位'], ['buildings', '基础建筑'], ['gpus', '显卡硬件'], ['bugs', '错误档案']] as const).map(([id, name]) => <button key={id} aria-pressed={archive === id} className={archive === id ? 'is-active' : ''} onClick={() => { setArchive(id); setEntry(0); }}>{name}</button>)}</nav>
        <div className="hub-archive-layout"><div className="hub-archive-list">{archiveItems.map((item, index) => <button key={item.name} className={entry === index ? 'is-selected' : ''} aria-pressed={entry === index} onClick={() => setEntry(index)}><img src={item.image} alt=""/><span><strong>{item.name}</strong><small>{item.code}</small></span><ChevronRight size={15}/></button>)}</div>
          <article className={`hub-dossier dossier-${archive}`}><div className="hub-dossier-art"><img src={selectedEntry.image} alt={selectedEntry.name}/></div><div className="hub-dossier-copy"><span>{selectedEntry.code}</span><h2>{selectedEntry.name}</h2><div className="hub-dossier-stats">{selectedEntry.stats.map(stat => <b key={stat}>{stat}</b>)}</div><p>{selectedEntry.description}</p><p>{selectedEntry.detail}</p>{selectedEntry.extra && <p className="hub-dossier-extra">{selectedEntry.extra}</p>}{selectedEntry.source && <a href={selectedEntry.source} target="_blank" rel="noreferrer">{archive === 'gpus' ? '厂商资料与图片来源' : archive === 'bugs' ? '查看 CWE 错误资料' : '查看形象来源'} ↗</a>}</div></article>
        </div></div> : <div className="hub-settings"><header className="hub-section-title"><span>PREFERENCES / COMMAND EXPERIENCE</span><h1>找到自己的指挥节奏。</h1><p>界面与声音偏好保存在此设备。</p></header><div className="hub-settings-grid">
          <section><h2><Volume2 size={19}/>声音</h2><label className="hub-volume">界面音效 <strong>{Math.round(settings.volume * 100)}%</strong><input aria-label="界面音效音量" type="range" min="0" max="100" value={Math.round(settings.volume * 100)} onChange={event => setting('volume', Number(event.target.value) / 100)}/></label><p>控制按钮的轻提示音，不包含战斗配乐。</p><button data-preview-tone="true" className="hub-setting-button" onClick={() => playCommandTone(settings.volume)}>试听界面音效</button>
          <h2><Boxes size={19}/>战场显示</h2><label className="hub-select-setting">默认覆盖图层<select aria-label="默认覆盖图层" value={settings.overlay} onChange={event => setting('overlay', Number(event.target.value))}><option value={0}>隐藏覆盖</option><option value={1}>网络与护盾</option><option value={2}>网络与地形网格</option></select></label>
          {([['reducedMotion', '减少界面动态', '关闭装饰动效，保留原生战斗帧动画。'], ['showTutorial', '显示建设指引', '新行动提供电站、机架与布防步骤。'], ['showNotices', '显示战场提示', '显示部署和指令结果的短提示。']] as const).map(([key, title, description]) => <label className="hub-toggle" key={key}><span><strong>{title}</strong><small>{description}</small></span><input type="checkbox" checked={settings[key]} onChange={event => setting(key, event.target.checked)}/></label>)}
          <button className="hub-setting-button" onClick={() => onSettings({ ...DEFAULT_COMMAND_SETTINGS })}>恢复默认偏好</button></section>
          <section><h2><Flag size={19}/>快捷键</h2><label className="hub-toggle"><span><strong>启用战场快捷键</strong><small>关闭后仍可使用界面按钮指挥。</small></span><input type="checkbox" checked={settings.shortcuts} onChange={event => setting('shortcuts', event.target.checked)}/></label><div className="hub-keymap">{COMMAND_HOTKEYS.map(([key, label]) => <div key={key}><kbd>{key}</kbd><span>{label}</span></div>)}</div></section>
        </div></div>}
    </div>
    <footer className="hub-footer"><span>BUILD THE NETWORK. HOLD THE LINE.</span><small>实时战略 · 建设与防御</small></footer>
  </section>;
}
