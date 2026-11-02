//! Opt-in deterministic orchestration using a real isolated 2D Godot engine.
//! Authored fixture assets replace an LLM; handoffs, native input, QA evidence,
//! and the final reviewer gate use the actual production implementation.
use super::*;
use crate::llm::ExecFn;
use crate::ultraplan::production::{self, ProductionTaskStatus};

struct RuntimeEnv(Vec<(String, Option<std::ffi::OsString>)>);
impl RuntimeEnv {
    fn set(&mut self, name: &str, value: Option<&Path>) {
        self.0.push((name.into(), std::env::var_os(name)));
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
}
impl Drop for RuntimeEnv {
    fn drop(&mut self) {
        for (name, previous) in self.0.iter().rev() {
            if let Some(value) = previous {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}

fn node(id: &str, kind: &str, inputs: Value) -> Value {
    json!({"id":id,"type":kind,"pos":[0,0],"inputs":inputs})
}
fn edge(from: &str, pin: &str, to: &str) -> Value {
    json!({"from":[from,pin],"to":[to,"exec"]})
}
fn graph(id: &str, nodes: Vec<Value>, edges: Vec<Value>) -> Value {
    json!({"version":1,"id":id,"name":id,"exposedProps":[],"nodes":nodes,"edges":edges})
}
fn component(kind: &str, props: Value) -> Value {
    json!({"type":kind,"enabled":true,"props":props})
}
fn entity(id: u64, name: &str, translation: [f64; 3], components: Vec<Value>) -> Value {
    json!({"id":id,"name":name,"transform":{"translation":translation,"rotation":[0,0,0,1],"scale":[1,1,1]},"components":components})
}

#[tokio::test]
#[ignore = "requires installed Godot runtime and native MCP binaries; run alone with --test-threads=1; keeps evidence in C temp"]
async fn real_2d_godot_production_handoff_qa_and_review_gate() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let (state, root) = crate::test_app_state("up-native-2d-godot");
    assert!(
        root.starts_with(std::env::temp_dir()),
        "native evidence must remain in temp"
    );
    let mut env = RuntimeEnv(Vec::new());
    for (name, relative) in [
        (
            "FORGE_ENGINE_SCENE_MCP_BIN",
            "target/debug/engine-scene-mcp.exe",
        ),
        (
            "FORGE_CODE_FORGE_MCP_BIN",
            "target/debug/code-forge-mcp.exe",
        ),
        ("FORGE_ENGINE_HOST_BIN", "target/debug/engine-host.exe"),
        ("FORGE_GODOT_RUNTIME_DIR", "target/godot-runtime"),
    ] {
        let path = std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join(relative));
        assert!(
            path.exists(),
            "native smoke prerequisite missing: {}",
            path.display()
        );
        env.set(name, Some(&path));
    }
    for name in [
        "FORGE_RENDER_BACKEND",
        "FORGE_RENDER_METHOD",
        "FORGE_RENDER_DRIVER",
        "FORGE_HOST_PORT",
        "FORGE_GAME_SCENE",
    ] {
        env.set(name, None);
    }
    env.set(
        "FORGE_HOST_EVENTS_LOG",
        Some(&root.join("host-events.jsonl")),
    );
    println!("REAL_2D_GODOT_EVIDENCE={}", root.display());
    let ws = root.join("game");
    crate::project::init_project(&ws, "Two steps to win", "2d").unwrap();
    let mut project = assetd::project::ForgeProject::load(&ws).unwrap();
    assert_eq!(project.render.as_strs().0, "godot");
    project.entry_scene = "Content/Scenes/game.rxscene".into();
    project.save_manifest().unwrap();
    let source = root.join("white.png");
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]))
        .save(&source)
        .unwrap();
    let imported = assetd::import::import_assets(
        &project,
        &[source.to_string_lossy().into_owned()],
        "Textures",
        None,
    )
    .unwrap();
    assert!(imported.failed.is_empty(), "{imported:?}");
    let texture = &imported.imported[0].guid;
    let player = graph(
        "Player",
        vec![
            node("start", "event.on_start", json!({})),
            node(
                "right",
                "var.set",
                json!({"name":{"const":"right"},"value":{"const":[1,0,0]}}),
            ),
            node("input", "event.on_input", json!({})),
            node(
                "offset",
                "var.get",
                json!({"name":{"node":"input","pin":"action"}}),
            ),
            node(
                "move",
                "transform.move_tween",
                json!({"target":{"const":"$self"},"offset":{"node":"offset","pin":"out"},"duration":{"const":0}}),
            ),
        ],
        vec![
            edge("start", "exec", "right"),
            edge("input", "exec", "move"),
        ],
    );
    let goal = graph(
        "Goal",
        vec![
            node("touch", "event.on_trigger_enter", json!({})),
            node(
                "player",
                "entity.has_tag",
                json!({"entity":{"node":"touch","pin":"otherEntity"},"tag":{"const":"player"}}),
            ),
            node(
                "branch",
                "flow.branch",
                json!({"condition":{"node":"player","pin":"out"}}),
            ),
            node(
                "win",
                "entity.add_tag",
                json!({"entity":{"node":"touch","pin":"otherEntity"},"tag":{"const":"winner"}}),
            ),
        ],
        vec![
            edge("touch", "exec", "branch"),
            edge("branch", "then", "win"),
        ],
    );
    std::fs::create_dir_all(ws.join("Content/Graphs")).unwrap();
    write_json_atomic(&ws.join("Content/Graphs/player.rxgraph"), &player).unwrap();
    write_json_atomic(&ws.join("Content/Graphs/goal.rxgraph"), &goal).unwrap();
    let scene = json!({"name":"Two steps to win","mode":"2d","gravity":[0,0,0],"next_id":4,"entities":[
        entity(1,"Player",[0.0,0.0,0.0],vec![
            component("Sprite",json!({"texture":texture,"tint":[1.0,0.8,0.1,1.0],"pixelsPerUnit":64.0})),
            component("Tag",json!({"tag":"player"})),
            component("Script",json!({"module":"","graphRef":"Content/Graphs/player.rxgraph","props":{}}))]),
        entity(2,"Goal",[2.0,0.0,0.0],vec![
            component("Sprite",json!({"texture":texture,"tint":[0.1,0.9,0.3,1.0],"pixelsPerUnit":64.0})),
            component("Trigger",json!({"kind":"box","extents":[0.3,0.3,0.3]})),
            component("Script",json!({"module":"","graphRef":"Content/Graphs/goal.rxgraph","props":{}}))]),
        entity(3,"Camera",[0.0,0.0,10.0],vec![component("Camera",json!({"projection":"orthographic","orthoSize":5.0,"near":0.1,"far":100.0}))])
    ]});
    write_json_atomic(&ws.join("Content/Scenes/game.rxscene"), &scene).unwrap();
    let workspace = state
        .workspaces
        .create("Native 2D smoke", ws.to_str().unwrap())
        .unwrap();
    let session = state.sessions.create(
        "Native 2D production verification",
        "coding",
        None,
        false,
        Some(workspace.id.clone()),
    );
    let sid = &session.id;
    state.permissions.set_mode(sid, "bypass").unwrap();
    let (run, _) = state.runs.begin(sid, "deterministic_native_smoke");
    let rid = &run.id;
    state.sessions.claim_active_run(sid, rid).unwrap();
    let mut up = UltraPlanState::new_flow("Native 2D smoke", Some(&workspace.id), &ws);
    up.plan_rev = 1;
    up.stage = STAGE_PRODUCTION.into();
    up.phase = PHASE_RUNNING.into();
    up.production_run_id = Some(rid.clone());
    let dir = up.dir_abs(&ws).unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let tasks = json!([
        {"id":"assets","title":"Import 2D sprites","role":"asset-wrangler","prompt":"Import white sprite for player and goal","stage":"build","deps":[],"verify":"qa","checkIds":["visual"],"requiredTools":["mcp__asset-pipeline__asset_import"],"outputs":["Content/Textures/white.png"],"handoffChecks":[{"kind":"file","path":"Content/Textures/white.png"}]},
        {"id":"logic","title":"Input and win graph","role":"logic-programmer","prompt":"Implement right input and goal trigger","stage":"build","deps":["assets"],"verify":"qa","checkIds":["gameplay","feel"],"requiredTools":["mcp__code-forge__graph_validate"],"outputs":["Content/Graphs/player.rxgraph","Content/Graphs/goal.rxgraph"],"handoffChecks":[{"kind":"graph","path":"Content/Graphs/player.rxgraph"},{"kind":"graph","path":"Content/Graphs/goal.rxgraph"}]},
        {"id":"scene","title":"2D scene","role":"scene-builder","prompt":"Bind sprites and graphs in the approved 2D scene","stage":"build","deps":["logic"],"verify":"qa","checkIds":["visual"],"requiredTools":["mcp__engine-scene__scene_save"],"outputs":["Content/Scenes/game.rxscene"],"handoffChecks":[{"kind":"scene","path":"Content/Scenes/game.rxscene"},{"kind":"matrix","path":"Content/Scenes/game.rxscene","matrix":{"scene":"Content/Scenes/game.rxscene","enter_play":false,"cases":[{"name":"Scene has player goal camera","assert":{"kind":"entity_count","expected":3}}]}}]}
    ]);
    let checks = json!({"automated":[
        {"id":"visual","kind":"visual","scene":"Content/Scenes/game.rxscene","steps":"Render approved 2D scene","expected":"Visible sprites"},
        {"id":"gameplay","kind":"gameplay","scene":"Content/Scenes/game.rxscene","steps":"Press right twice","expected":"Player reaches goal and wins"}],
        "manual":[{"id":"feel","title":"Game feel","steps":"Play two steps and restart","expected":"Responsive","required":true}]});
    let delivery = json!({"entry":"Content/Scenes/game.rxscene","controls":"Right twice to reach the goal. Stop/restart play to reset."});
    validate_tasks(&json!({"tasks":tasks,"checks":checks,"delivery":delivery})).unwrap();
    save(&dir, TEAM_PLAN_FILE, &tasks).unwrap();
    save(&dir, CHECKS_FILE, &checks).unwrap();
    save(&dir, DELIVERY, &delivery).unwrap();
    save(
        &dir,
        TARGET,
        &json!({"projectRoot":".","gameMode":"2d","renderBackend":"godot","stackConfirmed":true}),
    )
    .unwrap();
    let plan_path = up.reserved_plan_path();
    write_atomic(&crate::plan_doc::abs_path(&ws,&plan_path),"Deterministic native fixture: sprites -> input/win graph -> scene -> centralized QA -> reviewer gate. No LLM or human approval.").unwrap();
    up.plan_path = Some(plan_path);
    up.plan_hash = Some(plan_hash(&ws, up.plan_path.as_deref().unwrap(), &dir).unwrap());
    state
        .sessions
        .update_ultraplan(sid, |slot| *slot = Some(up.clone()))
        .unwrap();
    materialize(&state, sid, &up, &dir, ProductionPhase::Start).unwrap();
    let rt = UltraRuntime {
        kind: TurnKind::Production(ProductionPhase::Start),
        flow_id: up.id.clone(),
        dir_rel: up.dir.clone(),
        dir_abs: dir.clone(),
        facts: project_facts(&ws, &ws),
        deep: DeepPlanning::default(),
        deep_fallback: None,
        tracker: TurnTracker::default(),
        prompt_suffix: String::new(),
        exit_tool_specs: vec![],
        preamble: String::new(),
        sections: vec![],
    };
    let project_root = ws.clone();
    let exec: Arc<ExecFn> = Arc::new(move |name, args| {
        let root = project_root.clone();
        Box::pin(async move {
            match crate::mcp::call_tool_in(&root, &name, Some(args)).await {
                Ok(value) => match crate::playtest::unwrap_envelope(&value) {
                    Ok(value) => (true, value.to_string().into()),
                    Err(error) => (false, error.into()),
                },
                Err(error) => (false, error.to_string().into()),
            }
        })
    });
    let mut handoffs = Vec::new();
    for contract in production::contracts(&dir).unwrap() {
        let todo = state
            .todos
            .list_by_session(sid)
            .into_iter()
            .find(|todo| {
                todo.plan_todo_id
                    .as_deref()
                    .is_some_and(|id| id.ends_with(&format!(":{}", contract.id)))
            })
            .unwrap();
        let submitted=production::submission(&json!({"status":"completed","summary":"Deterministic fixture outputs written","outputs":contract.outputs,"evidence":[]})).unwrap();
        let result =
            production::verify_handoff(&ws, &contract, &todo.id, 1, submitted, exec.as_ref()).await;
        production::save_task_result(&dir, &todo.id, 1, &result).unwrap();
        assert_eq!(
            result.status,
            ProductionTaskStatus::Completed,
            "{}: {result:?}",
            contract.id
        );
        assert!(result.receipt.is_some());
        state
            .todos
            .patch(
                &todo.id,
                &PatchTodoRequest {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        handoffs.push(serde_json::to_value(result).unwrap());
    }
    let camera = json!({"target":[1,0,0],"yaw":0,"pitch":0,"dist":7,"fovY":55});
    let matrices = [
        (
            "visual",
            json!({"scene":"Content/Scenes/game.rxscene","camera":camera,"enter_play":false,"cases":[{"name":"Visible 2D sprites","assert":{"kind":"screenshot_nonblank","width":640,"height":360}}]}),
        ),
        (
            "gameplay",
            json!({"scene":"Content/Scenes/game.rxscene","camera":camera,"enter_play":true,"inputs":[{"action":"right","value":1,"settle":3},{"action":"right","value":1,"settle":3}],"settle_frames":3,"cases":[
                {"name":"Two right inputs move the player","assert":{"kind":"transform_near","entity":"Player","translation":[2,0,0],"tolerance":0.01}},
                {"name":"Goal trigger awards victory","assert":{"kind":"component_field","entity":"Player","type":"Tag","field":"props.tag","expected":"winner"}}
            ]}),
        ),
    ];
    let mut reports = Vec::new();
    for (id, matrix) in matrices {
        let (ok, feedback) = crate::ultraplan::verify::execute(
            &state,
            sid,
            rid,
            &rt,
            &ws,
            &json!({"id":id,"matrix":matrix}),
        )
        .await;
        assert!(ok, "real {id} check: {}", feedback.text);
        reports.push(serde_json::from_str::<Value>(&feedback.text).unwrap());
    }
    let review = crate::ultraplan::verify::reviewer_feedback(&rt).unwrap();
    assert!(
        !review.images.is_empty(),
        "reviewer must receive real verified screenshots"
    );
    let blocked = complete_production(
        &state,
        sid,
        rid,
        &rt,
        "No reviewer approval has been supplied",
    )
    .unwrap_err();
    assert!(blocked.contains("审阅批准证据"), "{blocked}");
    assert_eq!(
        state.sessions.get(sid).unwrap().ultraplan.unwrap().stage,
        STAGE_PRODUCTION
    );
    let summary = json!({"ok":true,"scope":"Deterministic authored 2D fixture with real Godot MCP. No live model creation or human/reviewer approval.","workspace":ws,"handoffs":handoffs,"checks":reports,"reviewerImages":review.images.len(),"reviewerGate":{"blocked":true,"reason":blocked}});
    write_json_atomic(&root.join("native-smoke-summary.json"), &summary).unwrap();
    state.sessions.release_active_run(sid, rid);
    state.runs.finish(rid, "completed");
    println!(
        "REAL_2D_GODOT_PASS={}",
        root.join("native-smoke-summary.json").display()
    );
    // Retain the isolated artifacts and screenshots as evidence, even on failure.
}
