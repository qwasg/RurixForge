# V4 玩家对战基础设施

本版提供可使用的双人准备室，以及用于后续原生 PVP 的服务器接口。**双玩家战斗尚未接入**，`combatAvailable` 默认且实际为 `false`。房间准备成功不表示存在可进行的多人战斗。

## 使用

运行独立包中的 `Start-PVP-Lobby.cmd`。启动窗口会列出本机局域网地址，双方在浏览器打开同一个服务器地址。创建者建立房间并分享六位代码，对方填写代码加入；两个席位固定为蓝方与红方，双方可改变准备状态。需要位于同一网络且系统允许该 Node 进程接收局域网连接。服务不会自动改变防火墙。

普通游戏入口自带仅本机的准备室服务；局域网入口为单独进程，只提供静态页面与房间 API，不启动引擎、不暴露 Forge RPC、不代理游戏帧或本机文件。关闭准备室窗口会结束所有房间。

席位凭据只存在当前标签页的 `sessionStorage`，刷新同一页面可恢复。关闭面板、关闭标签页或断网后保留席位 120 秒；超时移除席位，房主离开时转交给另一位玩家。凭据不写日志，房间不会在进程重启后恢复。

## 协议与接口

协议名 `code-sentinels-pvp/1`，统一前缀 `/api/sentinels/net`。创建与加入请求必须携带一致的 `protocol`。房间受限为 2 位玩家，单进程最多 16 个房间。

- `GET /capabilities`：明确返回准备室、PVP 模式、协议和战斗能力标志。
- `POST /rooms`：`nickname`、可选 `seed` 和 `map`，返回服务端分配的 `playerId`、`token` 与房间快照。
- `POST /join`：`nickname` 与 `code`，返回另一阵营的席位凭据。
- `GET /rooms/:id`：携带 `Authorization: Bearer <token>`，读取快照并刷新在线心跳。
- `POST /rooms/:id/ready`：`ready: boolean`。
- `POST /rooms/:id/leave`：主动释放当前席位。
- `GET /rooms/:id/events?since=N`：读取最多 256 个保留事件以及当前快照；超出窗口时 `reset: true`。
- `POST /rooms/:id/start`：为未来战斗预留，当前返回 501。
- `POST /rooms/:id/orders`：为未来权威指令预留，当前返回 501。
- `GET /rooms/:id/snapshot`：为未来战斗状态预留，当前不返回模拟状态。

准备室路由没有跨源 CORS 授权，变更请求受到 Origin 检查、32 KiB 大小限制和按来源地址的频率限制。LAN 服务器对可接受的 Host 地址进行约束，静态文件路径必须留在 Web 目录。

## 接入原生权威服务器

`createMultiplayerService({ authority })` 接受四个必须同时存在的适配器方法：

- `start({roomId, seed, map, players})`：启动隔离的双玩家原生对局。使用服务器分配的 `owner` 与种子，不能借用单人会话。
- `execute({roomId, playerId, owner, order})`：处理一条指令并返回 `{accepted: boolean, tick: integer, ...}`。拒绝的合法序列指令也返回回执。
- `snapshot({roomId, playerId, owner})`：返回原生状态快照；若增加战争迷雾，需要按玩家裁剪信息。
- `close(roomId)`：释放该对局及相关引擎资源。

传输指令包含 `sequence`、`kind`、`entityIds`、`cells`、`model`。预留的类型为 `build`、`install-gpu`、`upgrade`、`recycle`、`move`、`stop`、`skill`、`wire`、`wall`、`shield`、`research`、`attack`。服务端将客户端身份绑定到阵营，不接受客户端自行指定 owner。

准备室的席位 `owner` 使用 1/2（蓝/红），V4 原生核心的 owner 预留使用 0/1；未来适配器必须显式映射 `nativeOwner = owner - 1`，并在原生侧再次核验归属。

每个房间串行下发命令；各玩家序号从 1 连续递增。最近 256 条相同序号、相同内容的重发返回已有回执；同一序号改变内容或跳号会拒绝。原生适配器也必须按 `(roomId, playerId, sequence)` 保证幂等，处理执行成功而传输超时的情况。浏览器不得自行扣费、预测战斗成功或重放断线购买。

准备室事件与指令回执写入 `.forge/multiplayer/<roomId>.jsonl`，可用于追踪和后续重放开发；这是事件日志，不是完整战斗存档，也不作为崩溃后的自动恢复数据库。原生资源消耗、科技、地形合法性和实体归属必须由实际双玩家核心再次验证。

本轮按用户要求没有追加网络测试或双人试玩；上述边界由实际实现定义，不将协议预留描述为已验证的完整联机功能。
