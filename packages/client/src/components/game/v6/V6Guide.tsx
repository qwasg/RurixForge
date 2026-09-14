import {useState} from 'react';
import {ArrowRight,Check,Layers3,Link2,Shield,Swords,Truck,Users,Warehouse} from 'lucide-react';
import type {V6Catalog} from '@/lib/sentinelsV6';
import {v6CardImage} from './V6Card';

const CHAPTERS=[
  {id:'opening',name:'开局生产线',tag:'01 / FIRST NETWORK',art:'wind-power',Icon:Warehouse,title:'把第一座机房接通。',steps:[
    ['取得收入','在核心附近的矿脉旁布置采集器。矿物先进入采集器，运抵核心或仓库后才变成经费。'],
    ['留下通路','B 拖出毛坯，例如6×4。等施工完成，在外墙开门，再划出机房和研究室；预留走廊与竖井位置。'],
    ['接通电力','建风机，按 L 依次点风机与机房端口。也可以选中设施，用“从此处拉电线”直接开始。两端顺序不影响供电。'],
    ['安装与回传','选中数据中心，I 安装5060。发电与线路满足负载后，C 把算力接向研究所、炮台或无线接入设施。'],
    ['部署防线','初始研究所建成后，U 可部署基础炮台。保护矿物运输，留出研究与补给资金。'],
  ],note:'总发电量不代表这条线路有电。选中机房可直接看到它所在电网的缺口。'},
  {id:'floors',name:'楼层与房间',tag:'02 / VERTICAL BASE',art:'data-center',Icon:Layers3,title:'先有结构，再布置用途。',steps:[
    ['自由规划','地基长宽按一格调整，范围4—24。房间划在已完成的毛坯内，走廊、支柱和竖井都占实际空间。'],
    ['向上与向下','PageUp / PageDown 切层，Tab 剖视。地下先挖掘；岩层需要钻掘设备，含水区域需要排水设施。'],
    ['连接楼层','楼梯供步行单位使用；货梯要接电才能通行；车辆需要足够宽的门和至少3格宽坡道。跨层需要时间，断路会中止通行。'],
    ['计算净面积','数据中心每4格净面积提供一个GPU槽。拆分、合并保留实际设备份额与库存，不会免费复制整套设备。'],
    ['守住支撑','T1—T5分别允许地上1/2/3/4/6层、地下0/1/1/2/2层。上层不能超出支撑；失稳出现预警后应撤离或维修。'],
  ],note:'先开门再装修。施工人员无法到达房间时，工程会等待可用通路。'},
  {id:'networks',name:'电力与算力',tag:'03 / CONNECTED SYSTEMS',art:'mobile-relay',Icon:Link2,title:'每一处消耗都有真实来源。',steps:[
    ['电力网络','L 连接发电设施和负载。同一个电网过载会影响其设备；没有连接的另一座电站不能隔空补上余量。'],
    ['算力网络','数据中心装卡并供电后才产算力。C 连接研究、网络防御及固定接入点，跨层线必须经过真实竖井。'],
    ['移动作战','有线AI驻守接入点；无线覆盖内可移动，离网后消耗随身缓存。一次攻击或技能可合并使用当前接入网络与缓存。'],
    ['楼内无线','地表可建移动基站。地下和高层可在毛坯内设置T2无线接入室，接好本层回传与电力。覆盖不会直接穿过楼板。'],
    ['能量武器','武器蓄能和算力分别结算。蓄能装备需要停靠可用供电设施或接入电网；飞机要真实降落后补充。'],
  ],note:'选中对象查看原因：“没有接线”“电网过载”“算力不足”“弹药耗尽”分别处理。'},
  {id:'research',name:'科技与精英',tag:'04 / RESEARCH DOCTRINE',art:'research-lab',Icon:Swords,title:'用投入形成主修与兼修。',steps:[
    ['选择方向','每个研究所绑定一个方向，可建多所发展速度、网安、算法、科研和轻量分支。没有全局单分支硬锁。'],
    ['持续投入','每条分支独立支付经费、算力、科研数据和时间。研究所断供会暂停研究；知识保留，重建后可恢复。'],
    ['部署AI','T2开始解锁各方向AI，T3增加进阶选择。同名角色可多次部署，但数量增加会抬高采购和维护支出。'],
    ['安装插件','先升级单位本身，再安装相应等级、同一分支的核心、攻击或支援插件。同类槽位不能重复叠加。'],
    ['组合军队','用少量精英的技能配合机械部队。对局中的卡牌显示当次AI采购价格；资源栏单独显示精英维护支出。'],
  ],note:'科技面板展示下一阶的实际费用、研究时间与算力占用。'},
  {id:'logistics',name:'地面与空运',tag:'05 / SUPPLY LINES',art:'cargo-aircraft',Icon:Truck,title:'货物抵达，优势才算到手。',steps:[
    ['真实库存','仓库、工坊、机场各自保有库存。选择补给工具，先点有货的己方源设施，再点需要补给的对象。'],
    ['指定路线','选中在途运输设置最多16个航点。依次点击地图，Enter确认，Backspace撤销。载具会真实经过航点，堵塞不会让货物瞬移。'],
    ['机场间空运','选择空运后，两端都必须是己方已完成、供电的机场跑道。运输机实际装燃油，经历起飞、巡航与降落。'],
    ['前线分拨','机场收到矿物只增加库存；再运到核心或仓库才结算经费。到站的弹药、燃料和维修物资可继续地面分拨。'],
    ['截击与回收','运输机和运输车可被击毁，货物会损失或成为可回收残货。弹药耗尽会停火，燃料不足会限制行动。'],
  ],note:'地面载具从真实工厂出发，再前往集结点；飞机从机场起降。'},
  {id:'defense',name:'攻防与目标',tag:'06 / BREAK THE NETWORK',art:'energy-defense',Icon:Shield,title:'击穿防线，也可以切断它的来源。',steps:[
    ['四类防御','实体墙提供阻挡与耐久；网络防御依赖机房；能量防御消耗蓄能；CUDA墙与护城河保护本层封闭区域。'],
    ['管理护盾','CUDA区域内需有运行机房。门和竖井敞开会破坏封闭；资源面板可停止自动充能，为技能与研究留出算力。'],
    ['选择攻击方式','直射会被墙拦截，曲射可越过低障碍。弹体真正命中才结算，爆炸有衰减，楼板能保护地下空间。'],
    ['反制天基','高阶打击有预警。摧毁发射平台和轨道控制设施能阻断后续发射，已经发出的弹体仍会继续。'],
    ['两种胜利','摧毁敌方核心，或18分钟后控制三个战略节点中的至少两个，完成360秒压制。争夺时暂停，失去多数每秒回退2秒。'],
  ],note:'矿点带来后续采购，战略节点带来经费、科研数据与压制进度。两者用途不同。'},
  {id:'multiplayer',name:'指挥与联机',tag:'07 / COMMAND TOGETHER',art:'factory',Icon:Users,title:'各自指挥，同一个战场。',steps:[
    ['镜头操作','滚轮缩放，Alt拖动或鼠标中键平移；Home回到基地，PageUp / PageDown切层，Tab剖视。'],
    ['下达指令','左键或拖框选择；右键移动或攻击，Shift右键地面追加移动点，S停止并清空队列，Q选择技能目标。'],
    ['编队与牌组','Ctrl＋数字保存编队，数字召回。B建筑、U单位、I显卡；L电力线、C算力线、W防御墙。'],
    ['建立房间','房主选择游戏端口创建房间，把地址和六位房间码交给朋友。对方用同版游戏包加入，双方准备后开始。局域网直接用大厅地址；公网直连需房主已将游戏端口映射至本机。'],
    ['保存与恢复','双方镜头和楼层独立。房主程序需持续运行；连接中断保留席位60秒，期间对局继续。存档、读取与回放在大厅管理。'],
  ],note:'单人可用空格暂停阅读手册；双人对战不能单方面暂停。'},
];
export default function V6Guide({catalog}:{catalog:V6Catalog}){
  const[chapter,setChapter]=useState('opening'),chosen=CHAPTERS.find(c=>c.id===chapter)!;
  const item=catalog.items.find(c=>c.id===chosen.art)??catalog.items.find(c=>c.chassis===chosen.art);
  const art=item?v6CardImage(item):`/games/code-sentinels/ui-v6/units/${chosen.art}.png`;
  return <div className="v6-handbook"><nav aria-label="手册章节">{CHAPTERS.map(c=><button key={c.id} aria-pressed={chapter===c.id} onClick={()=>setChapter(c.id)}><c.Icon size={17}/><span>{c.name}</span><ArrowRight size={13}/></button>)}</nav>
    <article><header><div><span className="v6-eyebrow">{chosen.tag}</span><h2>{chosen.title}</h2></div><img src={art} alt=""/></header><ol>{chosen.steps.map(([title,body],i)=><li key={title}><b>{String(i+1).padStart(2,'0')}</b><div><h3>{title}</h3><p>{body}</p></div></li>)}</ol><footer><Check size={16}/>{chosen.note}</footer></article>
  </div>;
}
