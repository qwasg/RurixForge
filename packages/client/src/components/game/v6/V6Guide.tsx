import {useState} from 'react';
import {ArrowRight,Check,Building2,Link2,Shield,Swords,Zap,Users} from 'lucide-react';
import type {V6Catalog} from '@/lib/sentinelsV6';
import {v6CardImage} from './V6Card';

const CHAPTERS=[
  {id:'opening',name:'开局生产线',tag:'01 / FIRST NETWORK',art:'wind-power',Icon:Building2,title:'把第一座机房接通。',steps:[
    ['取得收入','在核心附近的矿脉旁布置采集器。完工后，矿石每秒直接结算为经费。'],
    ['划分房间','B 拖出毛坯，例如6×4。等施工完成，在毛坯内划出机房和研究室。建筑是实体占地，单位从外侧通行。'],
    ['接通电力','建风机，按 L 依次点风机与机房端口。也可以选中设施，用“从此处拉电线”直接开始。两端顺序不影响供电。'],
    ['安装与回传','选中数据中心，I 安装5060。发电与线路满足负载后，C 把算力接向研究所、炮台或无线接入设施。'],
    ['部署防线','初始研究所建成后，U 可部署基础炮台。保护矿点与机房，留出研究资金。'],
  ],note:'总发电量不代表这条线路有电。选中机房可直接看到它所在电网的缺口与空间利用率。'},
  {id:'space',name:'空间与利用率',tag:'02 / SPACE BUDGET',art:'data-center',Icon:Building2,title:'先看占地，再看产出。',steps:[
    ['自由规划','地基长宽按一格调整，范围4—24。房间划在已完成的毛坯内，净面积等于毛面积。'],
    ['计算容量','数据中心每4格净面积提供一个GPU槽。拆分、合并保留实际设备份额，不会免费复制整套设备。'],
    ['利用率预览','建造或改建时查看预览卡：净面积→容量、每容量造价，以及电网负载/输出、算力占用前后对比。'],
    ['相邻扩建','毛坯可向相邻空地扩建。施工无人机必须能到达工地边缘，否则工程等待通路。'],
    ['清理废墟','摧毁后的残骸占用空间，需要清理后才能重建。'],
  ],note:'建楼只需要考虑空间与电力、算力、经费利用率。'},
  {id:'networks',name:'电力与算力',tag:'03 / CONNECTED SYSTEMS',art:'mobile-relay',Icon:Link2,title:'每一处消耗都有真实来源。',steps:[
    ['电力网络','L 连接发电设施和负载。同一个电网过载会影响其设备；没有连接的另一座电站不能隔空补上余量。'],
    ['算力网络','数据中心装卡并供电后才产算力。C 连接研究、网络防御及固定接入点。'],
    ['移动作战','有线AI驻守接入点；无线覆盖内可移动，离网后消耗随身缓存。一次攻击或技能可合并使用当前接入网络与缓存。'],
    ['维护费用','煤电与核电运行时按秒扣除经费；经费不足时停机。维修站在通电在线时为附近友军提供被动回复。'],
    ['能量武器','武器蓄能和算力分别结算。蓄能装备需要停靠可用供电设施或接入电网；飞机要真实降落后整备出动时长。'],
  ],note:'选中对象查看原因：“没有接线”“电网过载”“算力不足”分别处理。'},
  {id:'research',name:'科技与精英',tag:'04 / RESEARCH DOCTRINE',art:'research-lab',Icon:Swords,title:'用投入形成主修与兼修。',steps:[
    ['选择方向','每个研究所绑定一个方向，可建多所发展速度、网安、算法、科研和轻量分支。没有全局单分支硬锁。'],
    ['持续投入','每条分支独立支付经费、算力、科研数据和时间。研究所断供会暂停研究；知识保留，重建后可恢复。'],
    ['部署AI','T2开始解锁各方向AI，T3增加进阶选择。同名角色可多次部署，但数量增加会抬高采购和维护支出。'],
    ['安装插件','先升级单位本身，再安装相应等级、同一分支的核心、攻击或支援插件。同类槽位不能重复叠加。'],
    ['组合军队','用少量精英的技能配合机械部队。对局中的卡牌显示当次AI采购价格；资源栏单独显示精英维护支出。'],
  ],note:'科技面板展示下一阶的实际费用、研究时间与算力占用。'},
  {id:'sortie',name:'飞机整备',tag:'05 / AIR READINESS',art:'airstrip',Icon:Zap,title:'出动时长耗尽就回场。',steps:[
    ['露天跑道','飞机需要己方已完成、供电的露天机场。没有运输货物，只有作战出动。'],
    ['出动时长','空中飞行消耗出动时长；低于约20%时自动返回最近可用跑道。'],
    ['落地整备','停在通电路道上自动恢复出动时长与电能。快速整备中枢可提高附近整备与维修站回复速度。'],
    ['无弹药补给','地面单位不消耗弹药；动能武器按冷却开火，电能武器消耗自身蓄能。'],
  ],note:'维修指令只花经费并需要施工无人机到达，不再运输维修材料。'},
  {id:'defense',name:'攻防与目标',tag:'06 / BREAK THE NETWORK',art:'energy-defense',Icon:Shield,title:'击穿防线，也可以切断它的来源。',steps:[
    ['四类防御','实体墙提供阻挡与耐久；网络防御依赖机房；能量防御消耗蓄能；CUDA墙与护城河保护地面封闭区域。'],
    ['管理护盾','CUDA区域内需有运行机房。墙环缺口会破坏封闭；资源面板可停止自动充能，为技能与研究留出算力。'],
    ['选择攻击方式','直射会被墙拦截，曲射可越过低障碍。弹体真正命中才结算，爆炸有衰减。'],
    ['反制天基','高阶打击有预警。摧毁发射平台和轨道控制设施能阻断后续发射，已经发出的弹体仍会继续。'],
    ['两种胜利','摧毁敌方核心，或18分钟后控制三个战略节点中的至少两个，完成360秒压制。争夺时暂停，失去多数每秒回退2秒。'],
  ],note:'矿点带来后续采购，战略节点带来经费、科研数据与压制进度。两者用途不同。'},
  {id:'multiplayer',name:'指挥与联机',tag:'07 / COMMAND TOGETHER',art:'factory',Icon:Users,title:'各自指挥，同一个战场。',steps:[
    ['镜头操作','WASD 或方向键平移地图，滚轮缩放，Alt拖动或鼠标中键平移；Home回到基地。'],
    ['下达指令','左键或拖框选择；右键移动或攻击，Shift右键地面追加移动点，X停止并清空队列，Q选择技能目标。'],
    ['编队与牌组','Ctrl＋数字保存编队，数字召回。B建筑、U单位、I显卡；L电力线、C算力线、T防御墙、M移动。'],
    ['建立房间','房主选择游戏端口创建房间，把地址和六位房间码交给朋友。对方用同版游戏包加入，双方准备后开始。局域网直接用大厅地址；公网直连需房主已将游戏端口映射至本机。'],
    ['保存与恢复','双方镜头独立。房主程序需持续运行；连接中断保留席位60秒，期间对局继续。存档、读取与回放在大厅管理。'],
  ],note:'单人可用空格暂停阅读手册；双人对战不能单方面暂停。'},
];
const CLASSIC_CHAPTERS=[
  {id:'opening',name:'开局与炮台',tag:'01 / FIRST LINE',art:'extractor',Icon:Building2,title:'先有收入，再有防线。',steps:[
    ['取得收入','B 打开露天设施，在矿脉旁建采集器。完工后矿石每秒直接结算为经费，没有运输环节。'],
    ['部署炮台','U 打开战斗单位，点地面部署 VSCode 或 PyCharm。开局即可部署，不需要研究所。'],
    ['守住矿点','采集器被摧毁就断收入。围绕矿点和核心布置炮台，必要时 T 加一段实体墙。'],
    ['维修与回收','选中受损建筑可花经费维修，施工无人机需要能到达工地。不再需要维修材料。'],
  ],note:'单层塔防模式没有毛坯、房间、电力与算力线路，一切只看经费。'},
  {id:'deploy',name:'部署半径与科技',tag:'02 / RADIUS AND TECH',art:'research-lab',Icon:Swords,title:'锚点决定你能推进多远。',steps:[
    ['12格锚点','单位必须部署在指挥核心或任意已完工己方建筑的12格内。多建采集器与跑道就能把防线向前推。'],
    ['核心研究','选中核心或点右上科技等级，在核心研究五个分支。只消耗经费和时间，不需要研究所、算力或科研数据。'],
    ['分支单位','分支达到 T2 起解锁该方向的 AI 与载具。每个分支独立计费，同时最多研究一项。'],
    ['升级与插件','选中单位可升级到已研究的等级，并安装同分支、同等级的核心/攻击/支援插件。'],
  ],note:'兼修多个分支会抬高后续研究费用，先想清楚主修方向。'},
  {id:'battle',name:'攻防与目标',tag:'03 / BREAK THE LINE',art:'factory',Icon:Shield,title:'摧毁核心，或压制节点。',steps:[
    ['指挥操作','WASD 或方向键平移地图。左键或拖框选择；右键移动或攻击，Shift 右键追加移动点，X 停止，Q 选择技能目标，Ctrl＋数字编队。'],
    ['空中力量','研究到 T2 后可建露天跑道，再部署飞机。飞机消耗出动时长，低于约20%自动返回跑道整备。'],
    ['两种胜利','摧毁敌方核心，或18分钟后控制三个战略节点中的至少两个，完成360秒压制。'],
    ['联机对战','房主选择"单层塔防部署"创建房间，把地址和六位房间码交给朋友，双方规则必须一致。'],
  ],note:'固定炮台不能占领节点，需要可移动的 AI 或载具驻留。'},
];
export default function V6Guide({catalog,classic}:{catalog:V6Catalog;classic?:boolean}){
  const chapters=classic?CLASSIC_CHAPTERS:CHAPTERS;
  const[chapter,setChapter]=useState('opening'),chosen=chapters.find(c=>c.id===chapter)??chapters[0];
  const item=catalog.items.find(c=>c.id===chosen.art)??catalog.items.find(c=>c.chassis===chosen.art);
  const art=item?v6CardImage(item):`/games/code-sentinels/ui-v6/units/${chosen.art}.png`;
  return <div className="v6-handbook"><nav aria-label="手册章节">{chapters.map(c=><button key={c.id} aria-pressed={chosen.id===c.id} onClick={()=>setChapter(c.id)}><c.Icon size={17}/><span>{c.name}</span><ArrowRight size={13}/></button>)}</nav>
    <article><header><div><span className="v6-eyebrow">{chosen.tag}</span><h2>{chosen.title}</h2></div><img src={art} alt=""/></header><ol>{chosen.steps.map(([title,body],i)=><li key={title}><b>{String(i+1).padStart(2,'0')}</b><div><h3>{title}</h3><p>{body}</p></div></li>)}</ol><footer><Check size={16}/>{chosen.note}</footer></article>
  </div>;
}
