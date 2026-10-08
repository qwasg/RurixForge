# V3 原创工业战术界面与美术来源

最终 V3 界面采用本游戏原创的工业战术风格：大幅卡面、与战场融合的部署牌、少量贴近对象的信息和几何标记。新美术共 **14 张内置 imagegen PNG + 16 件代码原生 SVG**，最终30件实际文件及其尺寸、SHA-256、用途、同源备份核对见 [素材清单](../game/v3/artwork-inventory.md)。预览联系图与本页的官方研究截图不属于这30件交付素材。

14张插画的提示词归档在 [art-prompts.json](../Content/UI/v3/art-prompts.json)。DeepSeek 鲸鱼娘和 GPT 白龙娘沿用 [已核实的网络形象来源](sources.json)，不因界面升级而重设人物。硬件卡牌的原创插画只作背板，中央仍叠加 [真实厂商产品照片](sources-gpu.json)；这些7张复用照片不计作新生成的硬件美术。

本轮只参考《明日方舟 / Arknights》本体，没有使用 Endfield 的界面。

真实来源：

- [Arknights 全球官网](https://www.arknights.global/)：已打开网页来源并核对其官方 Yostar 资源地址。
- [Yostar 官方 App Store 页面](https://apps.apple.com/us/app/arknights/id1464872022)：开发者 YOSTAR，应用 ID1464872022。已下载并亲自查看官方上架截图，原始地址记录在 [official-source.json](arknights-ui/official-source.json)。
- 直接视觉参考：[战斗截图](arknights-ui/yostar-store-3-full.jpg)、[编队卡牌截图](arknights-ui/yostar-store-5-full.jpg)。它们是官方宣传图中嵌入的真实游戏画面，只放研究目录，未放入游戏发行素材。
- [官方动画预告](https://www.youtube.com/watch?v=zclYeBTcN7c)已核实官方频道信息；本轮界面观察依据前述实际截图，没有把动画宣传镜头当作战斗 UI。

观察到的界面规则：战场占主要画面；敌人数/核心状态在顶部少量排列；右上速度、暂停以独立几何按钮存在；单位头顶的技能提示和地面选择标记靠近对象。编队画面是高对比白底、窄长肖像卡，黑色斜切信息带、职业图形和少量黄色标记。侧重立绘本身，不把每项信息各包进圆角框。

本项目采用原创的工业战术几何：黑白/象牙白为主体，琥珀作为操作强调，青/紫只提示角色类别。已有 DeepSeek、GPT 形象保持；不复制明日方舟 logo、罗德岛标志、人物或原职业图形。

本轮直接核实的是编队窄牌与战场周边少量悬浮控件。将部署牌融入战场底边、合并硬件牌与角色牌，是本项目的布局选择，不把当前截图未呈现的状态说成亲眼观察。

## 已制作的16件 SVG

目录：packages/client/public/games/code-sentinels/ui-v3；同字节备份：projects/code-sentinels/Content/UI/v3。

- operator-card-frame：600×900，透明肖像区，下方渐暗信息区，切角与细刻度。
- hardware-card-frame：720×480，横向硬件卡套，适合覆盖真实 GPU 产品照片。
- skill-diamond-frame：256×256，技能菱形外框，中央填充不透明度为0，保留实际技能插画。
- deployment-reticle、selection-corners：256×256，场内部署准星、选中角标。
- scan-grid：128×128无缝校准纹；diagonal-hazard-strip：256×32无缝警戒条。
- operation-seal：320×320，原创代码括号行动章。
- victory-stamp、defeat-stamp：720×240，结算图章。
- credit-emblem、energy-core-emblem：128×128，经费、算力识别符号。
- class-precision、class-debug、class-tide、class-support：128×128，原创四职业图形。

色值：灰黑 #101316、象牙白 #F2F0E9、灰 #7F878A、琥珀 #EDB63B；角色点缀青 #66D3ED、紫 #B6A5EE。

用法：将边框置于立绘上方，框本身不承担按钮/状态逻辑。费用、冷却、姓名、选中态仍由真实 DOM 显示。中心留白给图像，不再叠大量说明面板。职业图形可在底部部署牌上以24–32px呈现；扫描纹理的页面最终不透明度建议不超过0.25。胜负章只在对应真实结算状态出现。

说明：本轮子代理的浏览器控制列表为空，未声称有交互式浏览器截屏；视觉核实使用官方商店提供的截图文件。SVG均为代码原生原创矢量装饰，不宣称是生成插画。
