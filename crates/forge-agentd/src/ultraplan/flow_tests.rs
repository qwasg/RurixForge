//! Persistent coordinator contracts, not LLM or engine-renderer end-to-end tests.
//! Model exit payloads and production verification reports are explicit fixtures.
//! The ignored browser variant additionally executes the real Demo probe.

use super::*;

const DEMO: &str = r#"<!doctype html><meta charset="utf-8"><title>Fixture game</title>
<canvas width="960" height="540"></canvas><script>
let x=0;const draw=()=>{const c=document.querySelector('canvas').getContext('2d');c.fillStyle='#162136';c.fillRect(0,0,960,540);c.fillStyle='#ed8';c.fillRect(20+x,30,50,50)};
window.__demo={reset(){x=0;draw()},getState(){return {player:{x},phase:'playing'}}};
addEventListener('keydown',e=>{if(e.key==='ArrowRight'){x+=10;draw()}});draw();
</script>"#;

struct FlowFixture {
    state: Arc<AppState>,
    root: PathBuf,
    ws: PathBuf,
    sid: String,
    scope: crate::scope::ScopeProject,
    engine: String,
    backend: String,
}

impl FlowFixture {
    fn new(engine: &str, backend: &str) -> Self {
        let (state, root) = crate::test_app_state(&format!("up-flow-{engine}-{backend}"));
        let ws = root.join("empty-workspace");
        std::fs::create_dir_all(&ws).unwrap();
        // Real scope resolution points an empty workspace at a separate engine
        // fallback project. It points at the workspace itself only after init.
        let fallback = root.join("engine-fallback");
        crate::project::init_project(&fallback, "Unrelated fallback", "3d").unwrap();
        let workspace = state
            .workspaces
            .create("Contract game", ws.to_str().unwrap())
            .unwrap();
        let mut session = state.sessions.create(
            "Game brief",
            "coding",
            None,
            false,
            Some(workspace.id.clone()),
        );
        session.agent_engine = engine.into();
        state.sessions.save(&session);
        state.permissions.set_mode(&session.id, "bypass").unwrap();
        let scope = crate::scope::ScopeProject {
            workspace_id: Some(workspace.id),
            name: "Contract game".into(),
            workspace_root: ws.clone(),
            project_root: fallback,
            game_mode: assetd::project::GameMode::ThreeD,
        };
        Self {
            state,
            root,
            ws,
            sid: session.id,
            scope,
            engine: engine.into(),
            backend: backend.into(),
        }
    }

    fn up(&self) -> UltraPlanState {
        self.state
            .sessions
            .get(&self.sid)
            .unwrap()
            .ultraplan
            .unwrap()
    }

    fn persisted(&self, stage: &str) -> UltraPlanState {
        // Reload the actual persisted session file instead of inspecting only memory.
        let reloaded =
            crate::sessions::SessionStore::load(self.root.join("agent-sessions/sessions.json"));
        let session = reloaded.get(&self.sid).unwrap();
        assert_eq!(session.agent_engine, self.engine);
        let up = session.ultraplan.unwrap();
        assert_eq!(up.stage, stage);
        assert_eq!(up.id, self.up().id);
        assert_eq!(up.plan_rev, self.up().plan_rev);
        assert_eq!(up.acceptance_round, self.up().acceptance_round);
        up
    }

    fn request(&self, action: Action, answers: Option<Value>) -> UltraplanReq {
        let up = self.up();
        UltraplanReq {
            id: up.id.clone(),
            action: action.as_str().into(),
            rev: action.expected_rev(&up).map(Value::from),
            answers,
            acknowledge_approvals: false,
        }
    }

    fn resolve(&self, req: Option<&UltraplanReq>, input: &str) -> UltraTurn {
        let mode = req
            .and_then(|r| Action::parse(&r.action))
            .map(Action::required_mode)
            .unwrap_or("ultraplan");
        resolve_request(
            &self.state.sessions.get(&self.sid).unwrap(),
            &self.scope,
            mode,
            req,
            input,
            None,
        )
        .unwrap_or_else(|r| panic!("route failed: {}", r.status()))
        .unwrap()
    }

    fn start(&self, req: Option<&UltraplanReq>, input: &str) -> (String, UltraRuntime) {
        let ut = self.resolve(req, input);
        let (run, _) = self.state.runs.begin(&self.sid, "stage_contract_fixture");
        let rid = run.id;
        self.state
            .sessions
            .claim_active_run(&self.sid, &rid)
            .unwrap();
        revalidate(
            self.state
                .sessions
                .get(&self.sid)
                .unwrap()
                .ultraplan
                .as_ref(),
            ut.kind.mode(),
            &ut,
        )
        .unwrap();
        let runtime = begin_turn(&self.state, &self.sid, &rid, &self.scope, input, &ut).unwrap();
        (rid, runtime)
    }

    fn finish(&self, rid: &str, failed: bool) {
        finish_turn(
            &self.state,
            &self.sid,
            rid,
            &DeepPlanning::default(),
            if failed { "failed" } else { "completed" },
            failed.then_some("fixture interrupted"),
        );
        self.state.sessions.release_active_run(&self.sid, rid);
        self.state
            .runs
            .finish(rid, if failed { "failed" } else { "completed" });
    }

    fn exit(&self, rid: &str, rt: &UltraRuntime, name: &str, payload: &Value) {
        let (ok, message) = handle_exit_tool(&self.state, &self.sid, rid, Some(rt), name, payload);
        assert!(ok, "{name}: {message}");
    }

    fn reject_request(&self, req: &UltraplanReq) {
        let before = self.up();
        let mode = Action::parse(&req.action).unwrap().required_mode();
        assert!(resolve_request(
            &self.state.sessions.get(&self.sid).unwrap(),
            &self.scope,
            mode,
            Some(req),
            "",
            None
        )
        .is_err());
        let after = self.up();
        assert_eq!(before.id, after.id);
        assert_eq!(before.stage, after.stage);
        assert_eq!(before.plan_rev, after.plan_rev);
        assert_eq!(before.acceptance_round, after.acceptance_round);
    }

    fn acceptance(&self, round: u32, status: &str) -> Value {
        let up = self.up();
        json!({"id":up.id,"rev":up.plan_rev,"round":round,"results":[{"id":"feel","status":status,"note":"Fixture feedback: movement must respond"}]})
    }

    fn approve_production_fixtures(&self, rt: &UltraRuntime, rid: &str) {
        // These reports model successful trusted verifier results. No renderer is
        // invoked and this fixture must never be reported as rendered-game evidence.
        let up = self.up();
        let production = read_json(&rt.dir_abs.join(PRODUCTION_FILE)).unwrap();
        for id in production["activeTodoIds"].as_array().unwrap() {
            self.state
                .todos
                .patch(
                    id.as_str().unwrap(),
                    &PatchTodoRequest {
                        status: Some("completed".into()),
                        ..Default::default()
                    },
                )
                .unwrap();
        }
        let fingerprint = crate::ultraplan::evidence::project_fingerprint(&self.ws).unwrap();
        let shot = rt.dir_abs.join(format!("contract-{rid}.png"));
        std::fs::write(&shot, b"explicit simulated screenshot bytes").unwrap();
        let shot_rel = shot
            .strip_prefix(&self.ws)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let checks = read_json(&rt.dir_abs.join(CHECKS_FILE)).unwrap();
        for check in checks["automated"].as_array().unwrap() {
            let id = check["id"].as_str().unwrap();
            let file = rt.dir_abs.join(format!("contract-{rid}-{id}.json"));
            let report = json!({"ok":true,"id":id,"kind":check["kind"],"flowId":up.id,"runId":rid,"approvedCheck":check,"projectRoot":".","projectFingerprint":fingerprint,"backendInfo":{"renderBackend":self.backend},"reportPath":file.strip_prefix(&self.ws).unwrap().to_string_lossy().replace('\\',"/"),"screenshots":[{"path":shot_rel,"sha256":sha256_hex(b"explicit simulated screenshot bytes")}],"fixture":true});
            write_json_atomic(&file, &report).unwrap();
            record_check(&rt.dir_abs, id, report).unwrap();
        }
        record_review(&rt.dir_abs, json!({"ok":true,"flowId":up.id,"planRev":up.plan_rev,"planHash":up.plan_hash,"fixture":true})).unwrap();
    }
}

impl Drop for FlowFixture {
    fn drop(&mut self) {
        if let Some(up) = self.state.sessions.get(&self.sid).and_then(|s| s.ultraplan) {
            crate::demo_host::global().unregister(&up.token);
        }
        if let (Ok(root), Ok(temp)) = (
            self.root.canonicalize(),
            std::env::temp_dir().canonicalize(),
        ) {
            if root.parent() == Some(temp.as_path())
                && root
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("agentd-state-up-flow-"))
            {
                let _ = std::fs::remove_dir_all(root);
            }
        }
    }
}

fn questionnaire() -> Value {
    json!({"title":"Confirm the game","understanding":"A playable keyboard game with restart and victory feedback.","sections":[
        {"id":"loop","title":"Core loop","questions":[{"id":"goal","kind":"text","question":"Describe the goal","required":true}]},
        {"id":"style","title":"Presentation","questions":[{"id":"visual","kind":"text","question":"Visual direction","required":true}]}
    ]})
}

fn plan() -> Value {
    json!({"tasks":[
        {"id":"logic","title":"Game loop","role":"logic-programmer","prompt":"Implement controls and victory","stage":"build","deps":["scene"],"verify":"qa","checkIds":["gameplay","feel"]},
        {"id":"scene","title":"Scene setup","role":"scene-builder","prompt":"Create a readable game scene","stage":"build","deps":[],"verify":"qa","checkIds":["visual"]}],
        "checks":{"automated":[{"id":"visual","kind":"visual","scene":"Content/Scenes/main.rxscene","steps":"Capture game","expected":"Nonblank scene"},{"id":"gameplay","kind":"gameplay","scene":"Content/Scenes/main.rxscene","steps":"Move right","expected":"Player moves"}],"manual":[{"id":"feel","title":"Control feel","steps":"Play the loop and restart","expected":"Responsive movement","required":true}]},
        "delivery":{"entry":"Content/Scenes/main.rxscene","controls":"Arrow keys to move; R to restart"}})
}

async fn run_flow(engine: &str, mode: &str, backend: &str, real_browser: bool) {
    let mut f = FlowFixture::new(engine, backend);
    assert!(!f.ws.join("forge.toml").exists());
    let (discovery_id, discovery) = f.start(
        None,
        "Build a playable keyboard game. Preserve this complete original brief.",
    );
    assert!(!discovery.facts.has_content);
    assert!(discovery_tasks(&discovery).is_empty());
    assert!(
        !handle_exit_tool(
            &f.state,
            &f.sid,
            &discovery_id,
            Some(&discovery),
            PLAN_DOC_TOOL,
            &json!({})
        )
        .0
    );
    f.persisted(STAGE_DISCOVERY);
    f.exit(
        &discovery_id,
        &discovery,
        QUESTIONNAIRE_TOOL,
        &questionnaire(),
    );
    f.exit(
        &discovery_id,
        &discovery,
        QUESTIONNAIRE_TOOL,
        &questionnaire(),
    );
    assert_eq!(f.persisted(STAGE_QUESTIONNAIRE).questionnaire_rev, 1);
    f.finish(&discovery_id, false);

    let mut expired = f.request(Action::Answer, None);
    expired.rev = Some(json!(0));
    f.reject_request(&expired);
    let mut answers = json!({"goal":{"text":"Move to the goal, win, and restart"},"visual":{"text":"Bright player against a dark arena"}});
    // The server adds a mandatory explicit implementation choice even when the
    // model's fixture questionnaire omits it.
    let saved_questionnaire = read_json(&discovery.dir_abs.join(QUESTIONNAIRE_FILE)).unwrap();
    assert!(validate_answers(&saved_questionnaire, &answers).is_err());
    assert!(target_for_answers(&f.scope, &saved_questionnaire, &answers).is_err());
    answers[IMPLEMENTATION_STACK_QUESTION] = json!({"choice":[format!("{mode}_{backend}")]});
    let answer = f.request(Action::Answer, Some(answers));
    let (answer_id, demo_rt) = f.start(Some(&answer), "");
    let target = read_json(&demo_rt.dir_abs.join(TARGET)).unwrap();
    assert_eq!(target["gameMode"], mode);
    assert_eq!(target["renderBackend"], backend);
    assert_eq!(target["stackConfirmed"], true);
    if backend == "godot" {
        assert_eq!(target["renderMethod"], "forward_plus");
        assert_eq!(target["renderDriver"], "d3d12");
    } else {
        assert!(target["renderMethod"].is_null());
        assert!(target["renderDriver"].is_null());
    }
    let wrong_backend = if backend == "godot" { "rurix" } else { "godot" };
    assert!(!handle_spec(&f.state, &f.sid, &answer_id, &demo_rt,
        &json!({"spec":"Must not change confirmed stack","gameMode":mode,"renderBackend":wrong_backend})).0);
    assert!(!demo_rt.dir_abs.join(SPEC_FILE).exists());
    assert_eq!(read_json(&demo_rt.dir_abs.join(TARGET)).unwrap(), target);
    f.exit(&answer_id,&demo_rt,SPEC_TOOL,&json!({"spec":"Complete fixture requirements: controls, goal, win, restart, readable player.","gameMode":mode,"renderBackend":backend}));
    assert!(
        !f.ws.join("forge.toml").exists(),
        "Demo/requirements must not initialize production"
    );
    let (work, _) = prepare_demo(&demo_rt).unwrap();
    std::fs::write(work.join("index.html"), DEMO).unwrap();
    save(&work,"probe.json",&json!({"script":[{"call":"reset"}],"inputs":[{"kind":"keyPress","key":"ArrowRight"}],"assertions":[{"path":"player.x","op":"eq","expected":10}]})).unwrap();
    let manifest = read_json(&work.join("requirements.json")).unwrap();
    assert!(manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| work.join(c["path"].as_str().unwrap()).is_file()));
    assert!(complete_demo(
        &f.state,
        &f.sid,
        &answer_id,
        &demo_rt,
        &work,
        false,
        "simulated builder failed"
    )
    .await
    .is_err());
    assert_eq!(f.up().demo_iteration, 0);
    if real_browser {
        complete_demo(
            &f.state,
            &f.sid,
            &answer_id,
            &demo_rt,
            &work,
            true,
            "Real keyboard fixture probe",
        )
        .await
        .unwrap();
        assert!(f.up().demo_verified);
    } else {
        // Explicit test seam: simulate only the browser-success boundary. The
        // ignored variant above exercises complete_demo and the actual probe.
        replace_demo(&demo_rt.dir_abs, &work, 0).unwrap();
        advance(&f.state, &f.sid, &answer_id, &demo_rt, |up| {
            up.demo_iteration = 1;
            up.demo_verified = false;
            up.stage = STAGE_DEMO_REVIEW.into();
        })
        .unwrap();
        save(
            &demo_rt.dir_abs,
            "demo-digest.json",
            &json!({"iteration":1,"sha256":demo_digest(&demo_rt.dir_abs.join(DEMO_DIR)).unwrap()}),
        )
        .unwrap();
    }
    f.finish(&answer_id, false);
    f.persisted(STAGE_DEMO_REVIEW);
    assert!(!f.ws.join("forge.toml").exists());
    let stale_rollback = json!({"id":f.up().id,"rev":0});
    assert_eq!(
        rest_action(&f.state, &f.sid, "rollback_demo", Some(&stale_rollback)).status(),
        StatusCode::CONFLICT
    );

    let approval = f.request(Action::ApproveDemo, None);
    let (plan_id, plan_rt) = f.start(Some(&approval), "");
    let doc = json!({"name":"Keyboard MVP","overview":"One complete playable loop","plan":"Technology: selected Forge renderer. Team: scene-builder then logic-programmer. Steps: scene, controls, loop, automated visual/gameplay verification, reviewer, final manual acceptance."});
    f.exit(&plan_id, &plan_rt, PLAN_DOC_TOOL, &doc);
    assert_ne!(
        f.up().stage,
        STAGE_PLAN_REVIEW,
        "half a plan must not advance"
    );
    f.exit(&plan_id, &plan_rt, PLAN_TASKS_TOOL, &plan());
    f.exit(&plan_id, &plan_rt, PLAN_TASKS_TOOL, &plan());
    f.exit(&plan_id, &plan_rt, PLAN_DOC_TOOL, &doc);
    assert_eq!(f.persisted(STAGE_PLAN_REVIEW).plan_rev, 1);
    assert_eq!(
        read_json(&plan_rt.dir_abs.join(TARGET)).unwrap()["renderBackend"],
        backend
    );
    f.finish(&plan_id, false);
    f.reject_request(&approval);

    let start = f.request(Action::StartProduction, None);
    let ut = f.resolve(Some(&start), "");
    let (run, _) = f.state.runs.begin(&f.sid, "stage_contract_fixture");
    let rid = run.id;
    f.state.sessions.claim_active_run(&f.sid, &rid).unwrap();
    f.state.permissions.set_mode(&f.sid, "plan").unwrap();
    assert!(
        initialize_production_project(&f.state, &f.sid, &rid, &f.scope, &ut)
            .await
            .is_err()
    );
    assert!(!f.ws.join("forge.toml").exists());
    f.state.permissions.set_mode(&f.sid, "bypass").unwrap();
    initialize_production_project(&f.state, &f.sid, &rid, &f.scope, &ut)
        .await
        .unwrap();
    let project = assetd::project::ForgeProject::load(&f.ws).unwrap();
    assert_eq!(project.mode.as_str(), mode);
    assert_eq!(project.render.as_strs().0, backend);
    f.scope.project_root = f.ws.clone();
    f.scope.game_mode = project.mode;
    // The scaffold is real. The declared entry below models a task's authored file.
    let entry = f.ws.join("Content/Scenes/main.rxscene");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(&entry, "fixture authored scene, no renderer invoked").unwrap();
    let production = begin_turn(&f.state, &f.sid, &rid, &f.scope, "", &ut).unwrap();
    f.persisted(STAGE_PRODUCTION);
    assert!(complete_production(&f.state, &f.sid, &rid, &production, "not verified").is_err());
    assert_eq!(f.up().acceptance_round, 0);
    f.finish(&rid, true);
    let resume = f.request(Action::ResumeProduction, None);
    let (resume_id, resumed) = f.start(Some(&resume), "");
    assert_eq!(
        f.state.todos.list_by_session(&f.sid).len(),
        2,
        "resume may not duplicate tasks"
    );
    f.approve_production_fixtures(&resumed, &resume_id);
    complete_production(
        &f.state,
        &f.sid,
        &resume_id,
        &resumed,
        "Fixture production complete",
    )
    .unwrap();
    assert!(complete_production(&f.state, &f.sid, &resume_id, &resumed, "duplicate").is_err());
    assert_eq!(f.up().acceptance_round, 1);
    f.finish(&resume_id, false);
    f.persisted(STAGE_ACCEPTANCE);

    let mut invalid = f.acceptance(1, "pass");
    invalid["id"] = json!("expired-flow");
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&invalid)).status(),
        StatusCode::CONFLICT
    );
    invalid = f.acceptance(1, "pass");
    invalid["rev"] = json!(0);
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&invalid)).status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        rest_action(
            &f.state,
            &f.sid,
            "acceptance",
            Some(&f.acceptance(1, "skip"))
        )
        .status(),
        StatusCode::CONFLICT
    );
    let failed = f.acceptance(1, "fail");
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&failed)).status(),
        StatusCode::OK
    );
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&failed)).status(),
        StatusCode::CONFLICT
    );
    assert_eq!(f.up().stage, STAGE_ACCEPTANCE);

    let fix = f.request(Action::FixProduction, None);
    let (fix_id, fix_rt) = f.start(Some(&fix), "");
    let todos = f.state.todos.list_by_session(&f.sid);
    assert_eq!(todos.len(), 3);
    let repairs: Vec<_> = todos
        .iter()
        .filter(|t| {
            t.plan_todo_id
                .as_deref()
                .is_some_and(|id| id.contains(":fix:1:"))
        })
        .collect();
    assert_eq!(repairs.len(), 1);
    assert_eq!(repairs[0].role.as_deref(), Some("logic-programmer"));
    assert_eq!(
        read_json(&fix_rt.dir_abs.join(PRODUCTION_FILE)).unwrap()["checks"],
        json!({})
    );
    f.finish(&fix_id, true);
    let resume = f.request(Action::ResumeProduction, None);
    let (repair_id, repair_rt) = f.start(Some(&resume), "");
    assert_eq!(
        f.state.todos.list_by_session(&f.sid).len(),
        3,
        "repair retry may not duplicate or remake all tasks"
    );
    f.approve_production_fixtures(&repair_rt, &repair_id);
    complete_production(
        &f.state,
        &f.sid,
        &repair_id,
        &repair_rt,
        "Fixture repair complete",
    )
    .unwrap();
    f.finish(&repair_id, false);
    assert_eq!(f.persisted(STAGE_ACCEPTANCE).acceptance_round, 2);
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&failed)).status(),
        StatusCode::CONFLICT
    );
    let accepted = f.acceptance(2, "pass");
    // Approval is not enough if the authored game or approved plan changed
    // after automated verification. Restore exact bytes to finish this fixture.
    let entry_bytes = std::fs::read(&entry).unwrap();
    std::fs::write(&entry, "unverified edit after production").unwrap();
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&accepted)).status(),
        StatusCode::CONFLICT
    );
    f.persisted(STAGE_ACCEPTANCE);
    std::fs::write(&entry, entry_bytes).unwrap();
    let plan_path = crate::plan_doc::abs_path(&f.ws, f.up().plan_path.as_deref().unwrap());
    let plan_bytes = std::fs::read(&plan_path).unwrap();
    std::fs::write(&plan_path, "unapproved scope change").unwrap();
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&accepted)).status(),
        StatusCode::CONFLICT
    );
    f.persisted(STAGE_ACCEPTANCE);
    std::fs::write(&plan_path, plan_bytes).unwrap();
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&accepted)).status(),
        StatusCode::OK
    );
    let done = f.persisted(STAGE_DONE);
    assert_eq!(done.phase, PHASE_WAITING);
    assert_eq!(
        rest_action(&f.state, &f.sid, "acceptance", Some(&accepted)).status(),
        StatusCode::CONFLICT
    );
    let rounds = read_json(&repair_rt.dir_abs.join(ACCEPTANCE_FILE)).unwrap();
    assert_eq!(rounds["rounds"].as_array().unwrap().len(), 2);
    assert_eq!(rounds["rounds"][0]["failed"], json!(["feel"]));
    assert_eq!(rounds["rounds"][1]["failed"], json!([]));
    assert!(f
        .state
        .events
        .persisted(&f.sid)
        .iter()
        .any(|event| event.event_type == "ultraplan.done"));
}

#[tokio::test]
async fn stage_contract_local_rurix() {
    run_flow("local", "3d", "rurix", false).await;
}
#[tokio::test]
async fn stage_contract_local_godot() {
    run_flow("local", "2d", "godot", false).await;
}
#[tokio::test]
async fn stage_contract_codex_rurix() {
    run_flow("codex", "3d", "rurix", false).await;
}
#[tokio::test]
async fn stage_contract_codex_godot() {
    run_flow("codex", "2d", "godot", false).await;
}

#[tokio::test]
async fn stage_contract_local_3d_godot() {
    run_flow("local", "3d", "godot", false).await;
}
#[tokio::test]
async fn stage_contract_codex_3d_godot() {
    run_flow("codex", "3d", "godot", false).await;
}

#[tokio::test]
#[ignore = "requires installed Node/Playwright runtime and a system browser; not a live LLM or renderer test"]
async fn stage_contract_with_real_browser_probe() {
    run_flow("local", "3d", "rurix", true).await;
}
