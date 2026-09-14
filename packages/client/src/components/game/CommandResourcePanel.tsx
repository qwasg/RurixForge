import { useEffect, useRef } from 'react';
import { ArrowRight, Boxes, Coins, Cpu, Crosshair, Factory, Shield, X, Zap } from 'lucide-react';
import { BUILDINGS, type V4Building, type V4State } from '@/lib/sentinelsV4';
import { GPUS } from '@/lib/sentinelsV2';
import './command-resource-panel.css';

const format = (value: number) => value.toLocaleString('en-US', { maximumFractionDigits: 1 });
const cellName = (cell: number) => `${String.fromCharCode(65 + cell % 32 % 26)}${cell % 32 >= 26 ? '2' : ''}${Math.floor(cell / 32) + 1}`;
const ratio = (value: number, capacity: number) => Math.max(0, Math.min(100, capacity > 0 ? value / capacity * 100 : 0));
type Props = { state: V4State; connected: boolean; onClose(): void; onLocate(slot: number): void; onResearch(): void };

export default function CommandResourcePanel({ state: s, connected, onClose, onLocate, onResearch }: Props) {
  const dialog = useRef<HTMLElement>(null);
  useEffect(() => { const previous = document.activeElement as HTMLElement | null; dialog.current?.focus(); return () => previous?.focus(); }, []);
  const buildings = s.buildings.filter(building => building.active && building.owner === 0);
  const centers = buildings.filter(building => building.kind === 2), plants = buildings.filter(building => building.kind >= 3 && building.kind <= 6);
  const extractors = buildings.filter(building => building.kind === 9), labs = buildings.filter(building => building.kind === 8);
  const gpus = s.gpus.filter(gpu => gpu.model > 0 && centers.some(center => center.slot === gpu.centerSlot));
  const reserve = s.powerGenerated - s.powerDemand;
  const buildingButton = (building: V4Building, detail: string) => <button className="resource-building" key={building.slot}
    onClick={() => onLocate(building.slot)} aria-label={`定位${BUILDINGS.find(meta => meta.id === building.kind)?.name} ${cellName(building.cell)}`}>
    <img src={BUILDINGS.find(meta => meta.id === building.kind)?.image} alt=""/><span><strong>{BUILDINGS.find(meta => meta.id === building.kind)?.name}</strong>
      <small>{cellName(building.cell)} · LV.{building.tier} · {detail}</small></span><Crosshair size={16}/></button>;
  return <div className="resource-backdrop" onClick={onClose}><section className="command-resource-panel" ref={dialog} role="dialog" aria-modal="true" aria-label="资源控制面板" tabIndex={-1}
    onClick={event => event.stopPropagation()} onKeyDown={event => {
      event.stopPropagation(); if (event.key === 'Escape') { onClose(); return; } if (event.key !== 'Tab') return;
      const items = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled),a[href]'); if (!items?.length) return;
      if (event.shiftKey && (document.activeElement === items[0] || document.activeElement === dialog.current)) { event.preventDefault(); items[items.length - 1].focus(); }
      else if (!event.shiftKey && document.activeElement === items[items.length - 1]) { event.preventDefault(); items[0].focus(); }
    }}>
    <header className="resource-heading"><div><span>LOGISTICS / RESOURCE NETWORK</span><h1>让每一份资源，抵达防线。</h1><p>{connected ? '战场实时数据' : '连接中断 · 显示最后一次同步'} · 点击设施即可定位</p></div><button aria-label="关闭资源面板" onClick={onClose}><X size={22}/></button></header>
    <div className="resource-overview">
      <article className="resource-total is-credit"><span><Coins size={18}/>建设金币</span><strong data-testid="resource-credits">{format(s.credits)}</strong><small>矿脉收入 <b>+{format(s.incomePerSecond)} /s</b></small></article>
      <article className={`resource-total is-power ${reserve < 0 ? 'is-shortage' : ''}`}><span><Zap size={18}/>电力网络</span><strong data-testid="resource-power">{format(s.powerGenerated)} <em>/ {format(s.powerDemand)}</em></strong><small>供给 / 需求 <b>{reserve < 0 ? '缺口' : '总余量'} {format(Math.abs(reserve))}</b></small></article>
      <article className="resource-total is-compute"><span><Cpu size={18}/>算力储量</span><strong data-testid="resource-compute">{format(s.compute)} <em>/ {format(s.capacity)}</em></strong><div className="resource-meter" role="meter" aria-label="算力储量" aria-valuenow={Math.max(0, Math.min(s.capacity, s.compute))} aria-valuemin={0} aria-valuemax={Math.max(1, s.capacity)}><i style={{ width: ratio(s.compute, s.capacity) + '%' }}/></div><small>产能 <b>+{format(s.production)} /s</b></small></article>
    </div>
    <div className="resource-flow"><span><Factory size={16}/>矿脉 → 建设经费</span><ArrowRight size={16}/><span><Zap size={16}/>发电站 → 数据中心</span><ArrowRight size={16}/><span><Cpu size={16}/>GPU → 炮台 / AI / 护盾</span></div>
    <div className="resource-columns">
      <section><header><span>01 / POWER</span><h2>电网调度 <b>{plants.length} 座电站</b></h2></header>
        <p className="resource-hint">总量充足时，孤立电网仍可能缺电；检查每座设施的连接。</p>
        {plants.length ? plants.map(plant => buildingButton(plant, `供给 ${format(plant.supply)}`)) : <p className="resource-empty">尚未建设发电站。B 打开建筑牌组，先部署风力发电。</p>}
        {buildings.filter(building => building.demand > 0).map(building => buildingButton(building, `${building.powered ? '供电正常' : '未获供电'} · 需求 ${format(building.demand)}`))}
      </section>
      <section><header><span>02 / COMPUTE</span><h2>算力机架 <b>{gpus.length} / {centers.length * 4} 槽</b></h2></header>
        <p className="resource-hint">{gpus.filter(gpu => gpu.powered).length} 块 GPU 获供电 · {Math.max(0, centers.length * 4 - gpus.length)} 个空槽</p>
        {centers.length ? centers.map(center => <div className="resource-center" key={center.slot}>
          {buildingButton(center, `${center.powered ? '在线' : '供电中断'} · +${format(center.rate)}/s`)}
          <div className="resource-bays">{Array.from({ length: 4 }, (_, bay) => { const gpu = gpus.find(gpu => gpu.centerSlot === center.slot && gpu.bay === bay); return <button key={bay} onClick={() => onLocate(center.slot)} aria-label={`定位${cellName(center.cell)}机架 ${bay + 1}${gpu ? ' ' + GPUS[gpu.model - 1]?.name : ' 空槽'}`} className={gpu ? gpu.powered ? 'is-online' : 'is-offline' : ''}>
            {gpu ? <img src={'/games/code-sentinels/' + GPUS[gpu.model - 1]?.image} alt=""/> : <Cpu size={19}/>}<span>{gpu ? `+${format(gpu.powered ? gpu.rate : 0)}/s` : '空槽'}</span></button>; })}</div>
        </div>) : <p className="resource-empty">建造数据中心，接入电力，再把 GPU 装进机架。</p>}
        <div className="resource-consumption"><strong>本战区累计算力消耗</strong><span>攻击 <b>{format(s.spentAttack)}</b></span><span>技能 <b>{format(s.spentSkill)}</b></span><span>充盾 <b>{format(s.spentShield)}</b></span></div>
      </section>
      <section><header><span>03 / EXPANSION</span><h2>开采与研究 <b>科技 T{s.tech}</b></h2></header>
        <p className="resource-hint">矿脉和煤层提供持续建设经费；数据来自当前采集器。</p>
        {extractors.length ? extractors.map(extractor => buildingButton(extractor, `${s.terrain[extractor.cell] === 6 ? '煤层' : '矿脉'} · +${format(extractor.rate)}/s`)) : <p className="resource-empty">尚无持续矿脉收入。将资源采集器部署到矿脉或煤层。</p>}
        {labs.map(lab => buildingButton(lab, lab.powered && lab.connected ? '电力与算力就绪' : '需要电力与算力接入'))}
        <button className="resource-tech" onClick={onResearch}><Boxes size={22}/><span><strong>科技网络 · T{s.tech}</strong><small>查看 GPU、AI 与高级电站解锁</small></span><ArrowRight size={18}/></button>
        <div className="resource-shield"><Shield size={21}/><div><strong>拓扑护盾 {format(s.shield)} / {format(s.shieldCapacity)}</strong><small>{s.closedArea} 格闭环区域 · 自动充能{s.shieldAuto ? '开启' : '关闭'}</small></div></div>
      </section>
    </div>
  </section></div>;
}
