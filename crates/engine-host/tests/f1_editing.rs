//! engine-host F1 集成测试:真实 spawn 进程 + 长度前缀帧全链路。
//! ①确定性重载逐字节同态 ②undo/redo ③checkpoint/rollback ④play FSM ⑤batchApply 原子性。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::{json, Value};

struct HostProc {
    child: Child,
    port: u16,
}

impl Drop for HostProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// spawn engine-host(--port 0),从 stdout 就绪行解析实际端口。
fn spawn_host() -> HostProc {
    let exe = env!("CARGO_BIN_EXE_engine-host");
    let mut child = Command::new(exe)
        .args(["--port", "0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn engine-host 失败");
    let stdout = child.stdout.take().expect("无 stdout 管道");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("读就绪行失败");
    let port: u16 = line
        .trim()
        .strip_prefix("FORGE_HOST_LISTENING port=")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("就绪行格式非法:{line:?}"));
    HostProc { child, port }
}

/// 帧协议客户端。
struct Client {
    stream: TcpStream,
    next_id: u64,
}

impl Client {
    fn connect(port: u16) -> Self {
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("连接失败");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Client {
            stream,
            next_id: 1,
        }
    }

    fn write_frame(&mut self, v: &Value) {
        let payload = serde_json::to_vec(v).unwrap();
        let len = u32::try_from(payload.len()).unwrap();
        self.stream.write_all(&len.to_le_bytes()).unwrap();
        self.stream.write_all(&payload).unwrap();
        self.stream.flush().unwrap();
    }

    fn read_frame(&mut self) -> Value {
        let mut len_buf = [0u8; 4];
        self.stream.read_exact(&mut len_buf).unwrap();
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).unwrap();
        serde_json::from_slice(&buf).unwrap()
    }

    /// 正常调用:断言无 error,返回 result。
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.write_frame(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }));
        let resp = self.read_frame();
        assert_eq!(resp["id"], id, "响应 id 须回显");
        assert!(resp.get("error").is_none(), "{method} 不应报错:{resp}");
        resp["result"].clone()
    }

    /// 错误调用:返回 (code, message)。
    fn call_err(&mut self, method: &str, params: Value) -> (i64, String) {
        let id = self.next_id;
        self.next_id += 1;
        self.write_frame(&json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        }));
        let resp = self.read_frame();
        (
            resp["error"]["code"].as_i64().expect("应有 error.code"),
            resp["error"]["message"].as_str().unwrap_or("").to_string(),
        )
    }
}

/// 本测试进程私有的临时场景路径。
fn tmp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("f1_host_test_{}_{tag}.rxscene", std::process::id()))
}

/// 建一个带组件的演示实体并返回 id。
fn create_demo(c: &mut Client, name: &str, x: f64) -> u64 {
    let r = c.call(
        "entity.create",
        json!({
            "name": name,
            "components": [
                { "type": "MeshRenderer", "props": { "mesh": "cube.fbx", "material": "gray.mat" } },
                { "type": "RigidBody", "props": { "kind": "dynamic", "mass": 1.5 } }
            ],
            "translation": [x, 0.0, 0.0]
        }),
    );
    r["id"].as_u64().unwrap()
}

// ①确定性:3 实体+组件+摆位 → save → 新进程 load → 再 save → 两文件逐字节同态;
// 且重载后新建实体 id 不撞号。
#[test]
fn determinism_save_reload_byte_identical_across_processes() {
    let p1 = tmp_path("a");
    let p2 = tmp_path("b");
    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_file(&p2);

    let host1 = spawn_host();
    let mut c1 = Client::connect(host1.port);
    c1.call("scene.new", json!({ "name": "确定性场景" }));
    let e1 = create_demo(&mut c1, "甲", 1.0);
    let e2 = create_demo(&mut c1, "乙", 2.0);
    let e3 = create_demo(&mut c1, "丙", 3.0);
    c1.call(
        "component.add",
        json!({ "id": e3, "type": "Light", "props": { "kind": "point", "color": [1.0, 0.5, 0.25], "intensity": 3.0 } }),
    );
    c1.call("transform.set", json!({ "id": e2, "translation": [2.0, 1.5, -0.5] }));
    c1.call("scene.save", json!({ "path": p1.to_string_lossy() }));
    let list1 = c1.call("entity.list", json!({}));
    drop(c1);
    drop(host1);

    // 新进程:load → 再 save → 与磁盘逐字节同态。
    let host2 = spawn_host();
    let mut c2 = Client::connect(host2.port);
    let loaded = c2.call("scene.load", json!({ "path": p1.to_string_lossy() }));
    assert_eq!(loaded["entityCount"], 3);
    c2.call("scene.save", json!({ "path": p2.to_string_lossy() }));
    assert_eq!(
        std::fs::read(&p1).unwrap(),
        std::fs::read(&p2).unwrap(),
        "跨进程 load→save 须逐字节同态"
    );
    let list2 = c2.call("entity.list", json!({}));
    assert_eq!(list1, list2, "两进程 entity.list 须一致");

    // 重载后 id 接续:新建实体不撞号。
    let e4 = create_demo(&mut c2, "丁", 4.0);
    assert!(e4 > e3, "重载后新 id({e4})须大于已持久化最大 id({e3})");
    assert!(!list2["entities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["id"] == e4));

    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_file(&p2);
    let _ = e1;
}

// ②undo/redo:create → undo 消失 → redo 回来;空栈报错。
#[test]
fn undo_redo_command_stack() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "撤销测试" }));

    let id = create_demo(&mut c, "可撤销", 0.0);
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 1);

    // undo → 实体消失
    let u = c.call("edit.undo", json!({}));
    assert_eq!(u["undone"], true);
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 0);
    assert_eq!(c.call_err("entity.get", json!({ "id": id })).0, -32000);

    // redo → 原样回来(同 id/同组件)
    let r = c.call("edit.redo", json!({}));
    assert_eq!(r["redone"], true);
    let e = c.call("entity.get", json!({ "id": id }));
    assert_eq!(e["name"], "可撤销");
    assert_eq!(e["components"].as_array().unwrap().len(), 2);

    // transform.set 亦可撤销
    c.call("transform.set", json!({ "id": id, "translation": [9.0, 0.0, 0.0] }));
    assert_eq!(c.call("transform.get", json!({ "id": id }))["translation"][0], 9.0);
    c.call("edit.undo", json!({}));
    assert_eq!(c.call("transform.get", json!({ "id": id }))["translation"][0], 0.0);

    // 新变更清空 redo 栈
    c.call("transform.set", json!({ "id": id, "translation": [5.0, 0.0, 0.0] }));
    assert_eq!(c.call_err("edit.redo", json!({})).0, -32000, "新变更后 redo 栈须为空");
}

// ③checkpoint:checkpoint 后改场景 → rollback 逐字段一致。
#[test]
fn checkpoint_rollback_restores_field_by_field() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "快照测试" }));
    let id = create_demo(&mut c, "快照实体", 7.0);
    let before = c.call("entity.list", json!({}));

    let cp = c.call("scene.checkpoint", json!({}));
    assert_eq!(cp["depth"], 1);

    // 变更:加实体 + 改 transform + 删组件
    create_demo(&mut c, "多余实体", 8.0);
    c.call("transform.set", json!({ "id": id, "translation": [99.0, 0.0, 0.0] }));
    c.call("component.remove", json!({ "id": id, "type": "RigidBody" }));
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 2);

    // rollback → 与 checkpoint 前逐字段一致
    let rb = c.call("scene.rollback", json!({}));
    assert_eq!(rb["entityCount"], 1);
    let after = c.call("entity.list", json!({}));
    assert_eq!(before, after, "rollback 后场景须与 checkpoint 时逐字段一致");

    // 栈空后再 rollback → 错误
    assert_eq!(c.call_err("scene.rollback", json!({})).0, -32000);
}

// ④play FSM:合法迁移 + 非法迁移拒绝 + 运行态改动不污染编辑态。
#[test]
fn play_fsm_transitions_and_edit_state_isolation() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "PIE 测试" }));
    let id = create_demo(&mut c, "玩家", 1.0);

    // edit 态非法迁移
    assert_eq!(c.call("play.state", json!({}))["state"], "edit");
    assert_eq!(c.call_err("play.pause", json!({})).0, -32000, "edit 态禁止 pause");
    assert_eq!(c.call_err("play.step", json!({})).0, -32000, "edit 态禁止 step");
    assert_eq!(c.call_err("play.exit", json!({})).0, -32000, "edit 态禁止 exit");

    // enter → running
    assert_eq!(c.call("play.enter", json!({}))["state"], "play_running");
    let (code, _) = c.call_err("play.enter", json!({}));
    assert_eq!(code, -32000, "running 态禁止重复 enter");
    assert_eq!(c.call_err("play.resume", json!({})).0, -32000, "running 态禁止 resume");
    assert_eq!(c.call_err("play.step", json!({})).0, -32000, "running 态禁止 step");

    // 运行态改 transform(只作用运行态)
    c.call("transform.set", json!({ "id": id, "translation": [42.0, 0.0, 0.0] }));
    assert_eq!(c.call("transform.get", json!({ "id": id }))["translation"][0], 42.0);

    // pause → step → resume
    assert_eq!(c.call("play.pause", json!({}))["state"], "play_paused");
    assert_eq!(c.call_err("play.pause", json!({})).0, -32000, "paused 态禁止重复 pause");
    let step = c.call("play.step", json!({}));
    assert_eq!(step["state"], "play_paused");
    assert!(step["steps"].as_u64().is_some());
    assert_eq!(c.call("play.resume", json!({}))["state"], "play_running");

    // exit → 编辑态原值恢复
    assert_eq!(c.call("play.exit", json!({}))["state"], "edit");
    let t = c.call("transform.get", json!({ "id": id }));
    assert_eq!(t["translation"][0], 1.0, "exit 后编辑态 transform 须恢复原值");
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 1);
}

// ⑤batchApply:10 立方体排一列全成功;任一坏 op 全回滚。
#[test]
fn batch_apply_atomic_all_or_nothing() {
    let host = spawn_host();
    let mut c = Client::connect(host.port);
    c.call("scene.new", json!({ "name": "批量测试" }));

    // 全成功:10 个立方体 x = 0..9
    let ops: Vec<Value> = (0..10)
        .map(|i| {
            json!({
                "op": "create",
                "name": format!("立方体{i}"),
                "components": [
                    { "type": "MeshRenderer", "props": { "mesh": "cube.fbx", "material": "gray.mat" } }
                ],
                "translation": [i as f64, 0.0, 0.0]
            })
        })
        .collect();
    let r = c.call("entity.batchApply", json!({ "ops": ops }));
    assert_eq!(r["applied"], 10);
    let list = c.call("entity.list", json!({}));
    let entities = list["entities"].as_array().unwrap();
    assert_eq!(entities.len(), 10);
    for (i, e) in entities.iter().enumerate() {
        assert_eq!(e["transform"]["translation"][0].as_f64().unwrap(), i as f64,
            "第 {i} 个立方体 x 坐标须为 {i}");
    }
    // 整批一次 undo 全部消失
    c.call("edit.undo", json!({}));
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 0);
    c.call("edit.redo", json!({}));
    assert_eq!(c.call("entity.list", json!({}))["entities"].as_array().unwrap().len(), 10);

    // 任一坏 op → 全回滚(第 11 个 op 引用不存在实体)
    let mut bad_ops: Vec<Value> = (0..5)
        .map(|i| json!({ "op": "create", "name": format!("幽灵{i}") }))
        .collect();
    bad_ops.push(json!({ "op": "transform_set", "id": 999, "translation": [0.0, 0.0, 0.0] }));
    let (code, msg) = c.call_err("entity.batchApply", json!({ "ops": bad_ops }));
    assert_eq!(code, -32000);
    assert!(msg.contains("999"), "错误信息须含坏实体 id:{msg}");
    let list2 = c.call("entity.list", json!({}));
    assert_eq!(
        list2["entities"].as_array().unwrap().len(),
        10,
        "坏批须全回滚,不留幽灵实体"
    );

    // 参数面:ops 非数组 → -32602
    assert_eq!(c.call_err("entity.batchApply", json!({ "ops": 1 })).0, -32602);
}
