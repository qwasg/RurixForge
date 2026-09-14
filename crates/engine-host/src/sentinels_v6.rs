//! V6 session boundary. Only the local bridge reaches these RPC methods.
use crate::rpc::{HostState, PlayState};
use sentinels_v6::{Game, Order, PlaybackStatus, ReplayController, Save, Snapshot};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque,sync::{Arc,Mutex,atomic::{AtomicU64,Ordering}},time::Instant};
#[derive(Default)]
pub struct PlanningMetrics { pub wait:VecDeque<f64>,pub capture:VecDeque<f64>,pub plan:VecDeque<f64> }
#[derive(Default)]
pub struct Metrics { pub step:VecDeque<f64>,pub clone:VecDeque<f64>,pub publish:VecDeque<f64>,pub advance:VecDeque<f64>,pub planning:Arc<Mutex<PlanningMetrics>>,pub preview:Arc<Mutex<PlanningMetrics>>,pub clock_wait:VecDeque<f64>,pub clock_debt:VecDeque<f64>,pub clock_elapsed:VecDeque<f64> }
static CLOCK_EPOCH:AtomicU64=AtomicU64::new(1);
fn fresh_clock_epoch()->u64{CLOCK_EPOCH.fetch_add(1,Ordering::Relaxed)}
pub fn reset_clock(st:&mut HostState){if let Some(session)=st.sentinels_v6.as_mut(){session.clock_epoch=fresh_clock_epoch();}}
fn sample(q:&mut VecDeque<f64>,value:f64){if q.len()>=8192{q.pop_front();}q.push_back(value);}
pub(crate) fn metric(q:&VecDeque<f64>)->Value{if q.is_empty(){return json!({"samples":0});}let mut v=q.iter().copied().collect::<Vec<_>>();v.sort_by(f64::total_cmp);let at=|p:f64|v[((v.len()-1) as f64*p).ceil() as usize];json!({"samples":v.len(),"p50":at(0.5),"p95":at(0.95),"p99":at(0.99),"max":v.last()})}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct View {
    pub center_x: f64,
    pub center_y: f64,
    pub zoom: f64,
    pub layer: i32,
    pub cutaway: bool,
    pub local_player: u32,
}
impl Default for View {
    fn default() -> Self {
        Self {
            center_x: 16.,
            center_y: 48.,
            zoom: 1.,
            layer: 0,
            cutaway: true,
            local_player: 1,
        }
    }
}
pub struct Session {
    pub clock_epoch:u64,
    pub metrics:Metrics,
    pub game: Option<Game>,
    pub snapshot: Snapshot,
    pub view: View,
    pub replica: bool,
    pub replica_fresh: bool,
    pub playback: Option<ReplayController>,
}
type ResultValue = Result<Value, (i64, String)>;
fn failure(e: impl ToString) -> (i64, String) {
    (-32602, e.to_string())
}
struct CapturedSuggestion { game:Game,owner:u32,branch:String,style:String }
impl CapturedSuggestion {
    fn plan(mut self)->ResultValue {
        let tick=self.game.state.tick;
        let old=self.game.orders.len();
        self.game.bot_for_style(self.owner,&self.branch,&self.style);
        let commands:Vec<_>=self.game.orders[old..].iter().filter(|l|l.receipt.accepted).take(2).map(|l|l.order.command.clone()).collect();
        Ok(json!({"command":commands.first(),"commands":commands,"tick":tick}))
    }
}
fn with_captured_game(state:&Mutex<HostState>,p:&Value,preview:bool,work:impl FnOnce(Game,u32)->ResultValue)->ResultValue {
    let owner=p["owner"].as_u64().filter(|n|(1..=2).contains(n)).ok_or_else(||failure("玩家无效"))? as u32;
    let wait_start=Instant::now();
    let (captured,metrics)={
        let st=crate::rpc::lock(state);
        let wait=wait_start.elapsed().as_secs_f64()*1000.;
        let capture_start=Instant::now();
        let session=st.sentinels_v6.as_ref().ok_or_else(||failure("无权威会话"))?;
        if session.playback.is_some(){return Err(failure("回放期间不能规划权威指令"));}
        let game=session.game.as_ref().ok_or_else(||failure("无权威会话"))?.clone();
        let metrics=Arc::clone(if preview{&session.metrics.preview}else{&session.metrics.planning});
        {let mut rows=metrics.lock().unwrap_or_else(|e|e.into_inner());sample(&mut rows.wait,wait);sample(&mut rows.capture,capture_start.elapsed().as_secs_f64()*1000.);}
        (game,metrics)
    };
    // The planning snapshot and metrics belong to the captured session. Neither
    // computing the advice nor recording its duration reacquires HostState.
    let plan_start=Instant::now();
    let result=work(captured,owner);
    sample(&mut metrics.lock().unwrap_or_else(|e|e.into_inner()).plan,plan_start.elapsed().as_secs_f64()*1000.);
    result
}
fn with_captured_suggestion(state:&Mutex<HostState>,p:&Value,work:impl FnOnce(CapturedSuggestion)->ResultValue)->ResultValue{
    with_captured_game(state,p,false,|game,owner|work(CapturedSuggestion{game,owner,branch:p["branch"].as_str().unwrap_or("algorithm").into(),style:p["style"].as_str().unwrap_or("mixed-ai").into()}))
}
pub fn suggest(state:&Mutex<HostState>,p:&Value)->ResultValue{with_captured_suggestion(state,p,CapturedSuggestion::plan)}
pub fn preview(state:&Mutex<HostState>,p:&Value)->ResultValue{
    let command:sentinels_v6::Command=serde_json::from_value(p["command"].clone()).map_err(failure)?;
    with_captured_game(state,p,true,|game,owner|preview_projection(game,owner,command))
}
fn preview_projection(mut copy:Game,owner:u32,command:sentinels_v6::Command)->ResultValue{
    let before=copy.player(owner).unwrap().clone();
    let old_jobs:std::collections::BTreeSet<_>=copy.state.jobs.iter().map(|j|j.id).collect();
    let result=copy.execute(owner,command);
    let completion_targets:std::collections::BTreeSet<_>=if result.is_ok(){copy.state.jobs.iter().filter(|j|!old_jobs.contains(&j.id)&&matches!(j.kind.as_str(),"shell"|"room"|"build"|"expand"|"convert")).map(|j|j.target).collect()}else{std::collections::BTreeSet::new()};
    for building in copy.state.buildings.iter_mut().filter(|b|completion_targets.contains(&b.id)){building.progress=1.;}
    for room in copy.state.rooms.iter_mut().filter(|r|completion_targets.contains(&r.id)){room.progress=1.;}
    if !completion_targets.is_empty(){copy.networks();}
    let after=copy.player(owner).unwrap();
    Ok(json!({"valid":result.is_ok(),"reason":result.err().unwrap_or_default(),"completionProjection":!completion_targets.is_empty(),"cost":{"credits":(before.credits-after.credits).max(0.),"compute":(before.compute-after.compute).max(0.),"science":(before.science-after.science).max(0.)},"powerBefore":before.power,"powerAfter":after.power,"demandBefore":before.demand,"demandAfter":after.demand,"productionAfter":after.production}))
}
fn sync_replay_snapshot(st:&mut HostState){
    let paused={
        let Some(s)=st.sentinels_v6.as_mut() else{return;};
        let (Some(game),Some(replay))=(&mut s.game,&s.playback) else{return;};
        game.state.playback=Some(PlaybackStatus{paused:replay.paused,speed:replay.speed,currentTick:game.state.tick,totalTicks:replay.save.snapshot.tick});
        s.snapshot=game.state.clone();replay.paused
    };
    crate::sentinels_v6_render::set_paused(paused);
    publish(st);
}
pub fn record_clock(st:&mut HostState,wait_ms:f64,debt_seconds:f64,elapsed_seconds:f64){if let Some(s)=st.sentinels_v6.as_mut(){sample(&mut s.metrics.clock_wait,wait_ms);sample(&mut s.metrics.clock_debt,debt_seconds*1000.);sample(&mut s.metrics.clock_elapsed,elapsed_seconds*1000.);}}
fn metrics_value(s:&Session)->Value{
    let world=s.game.as_ref().map(|g|&g.state).unwrap_or(&s.snapshot);
    let planning=s.metrics.planning.lock().unwrap_or_else(|e|e.into_inner());
    let preview=s.metrics.preview.lock().unwrap_or_else(|e|e.into_inner());
    json!({"tick":world.tick,"replica":s.replica,"renderStagesByLayer":crate::sentinels_v6_render::render_metrics(),"milliseconds":{"simulationStep":metric(&s.metrics.step),"snapshotClone":metric(&s.metrics.clone),"publish":metric(&s.metrics.publish),"advanceTotal":metric(&s.metrics.advance),"suggestLockWait":metric(&planning.wait),"suggestSnapshotCapture":metric(&planning.capture),"suggestPlan":metric(&planning.plan),"previewLockWait":metric(&preview.wait),"previewSnapshotCapture":metric(&preview.capture),"previewPlan":metric(&preview.plan),"clockLockWait":metric(&s.metrics.clock_wait),"clockDebt":metric(&s.metrics.clock_debt),"clockElapsed":metric(&s.metrics.clock_elapsed)},"clockPolicy":"V6 retains elapsed wall time with at most two fixed steps per scheduler iteration; pause/session replacement resets debt. Legacy editor retains its existing250ms cap.","counts":{"shells":world.buildings.iter().filter(|b|b.kind=="shell").count(),"rooms":world.rooms.len(),"units":world.units.len(),"movingUnits":world.units.iter().filter(|u|u.moving).count(),"projectiles":world.projectiles.len()}})
}
pub fn handle(st: &mut HostState, method: &str, p: &Value) -> ResultValue {
    match method {
        "game.session.pressureBenchmark" => crate::sentinels_v6_pressure::run(p).map_err(failure),
        "game.session.metrics" => {let mut value=metrics_value(st.sentinels_v6.as_ref().ok_or_else(||failure("会话未启动"))?);value["runtimePages"]=crate::sentinels_v6_pages::metrics();value["backendTimingsByLayer"]=crate::sentinels_v6_backend_metrics::metrics();Ok(value)},
        "game.session.pick" => crate::sentinels_v6_render::pick(p).map_err(failure),
        "game.session.catalog" => Ok(sentinels_v6::catalog::catalog()),
        "game.session.open" => {
            crate::sentinels_v6_render::reset_scene();
            let replica = p["mode"] == "replica";
            let seed = p["seed"].as_u64().unwrap_or(1);
            let ai = p["opponent"] != "human";
            let theme = p["theme"].as_str().unwrap_or("river");
            if !matches!(theme, "river" | "mining" | "highland") {
                return Err(failure("未知地图主题"));
            }
            let game = Game::new_theme(seed, ai, theme);
            let snapshot = game.state.clone();
            let local = p["localPlayer"].as_u64().unwrap_or(1).clamp(1, 2) as u32;
            let view = View {
                local_player: local,
                center_x: if local == 1 { 16. } else { 112. },
                ..View::default()
            };
            st.sentinels_v6 = Some(Session {
                clock_epoch:fresh_clock_epoch(),
                metrics:Metrics::default(),
                game: if replica { None } else { Some(game) },
                snapshot: snapshot.clone(),
                view,
                replica,
                replica_fresh: replica,
                playback: None,
            });
            st.logic = None;
            st.play = PlayState::Running;
            publish(st);
            Ok(json!({"snapshot":snapshot}))
        }
        "game.session.close" => {
            crate::sentinels_v6_render::close();
            st.sentinels_v6 = None;
            st.run_scene = None;
            st.play = PlayState::Edit;
            Ok(json!({"closed":true}))
        }
        "game.session.order" => {
            let order: Order = serde_json::from_value(p.clone()).map_err(failure)?;
            let session = st
                .sentinels_v6
                .as_mut()
                .ok_or_else(|| failure("会话未启动"))?;
            if session.playback.is_some() {
                return Err(failure("回放期间不能下达游戏指令"));
            }
            let game = session
                .game
                .as_mut()
                .ok_or_else(|| failure("镜像不能执行权威指令"))?;
            let r = game.order(order);
            session.snapshot = game.state.clone();
            publish(st);
            Ok(json!(r))
        }
        "game.session.snapshot" => {
            let s = st
                .sentinels_v6
                .as_ref()
                .ok_or_else(|| failure("会话未启动"))?;
            let owner = p["owner"].as_u64().map(|v| v as u32);
            Ok(json!(if let Some(g) = &s.game {
                g.snapshot(owner)
            } else {
                s.snapshot.clone()
            }))
        }
        "game.session.applySnapshot" => {
            let incoming: Snapshot =
                serde_json::from_value(p["snapshot"].clone()).map_err(failure)?;
            incoming.validate(true).map_err(failure)?;
            let s = st
                .sentinels_v6
                .as_mut()
                .ok_or_else(|| failure("会话未启动"))?;
            if !s.replica {
                return Err(failure("不能覆盖权威世界"));
            }
            if !s.replica_fresh
                && (incoming.tick < s.snapshot.tick
                    || incoming.revision < s.snapshot.revision
                    || incoming.seed != s.snapshot.seed
                    || incoming.theme != s.snapshot.theme)
            {
                return Err(failure("旧快照"));
            }
            let applied = s.replica_fresh
                || incoming.revision != s.snapshot.revision
                || incoming.tick != s.snapshot.tick;
            if applied {
                s.snapshot = incoming;
                s.replica_fresh = false;
            }
            let result =
                json!({"applied":applied,"tick":s.snapshot.tick,"revision":s.snapshot.revision});
            publish(st);
            Ok(result)
        }
        "game.session.view" => {
            let s = st
                .sentinels_v6
                .as_mut()
                .ok_or_else(|| failure("会话未启动"))?;
            let mut v = serde_json::to_value(&s.view).unwrap();
            for (k, value) in p.as_object().ok_or_else(|| failure("view须为对象"))? {
                v[k] = value.clone();
            }
            let mut view: View = serde_json::from_value(v).map_err(failure)?;
            if !view.center_x.is_finite() || !view.center_y.is_finite() || !view.zoom.is_finite() {
                return Err(failure("无效镜头"));
            }
            view.zoom = view.zoom.clamp(0.4, 4.);
            view.layer = view.layer.clamp(-2, 5);
            view.local_player = view.local_player.clamp(1, 2);
            s.view = view;
            publish(st);
            Ok(json!({"updated":true}))
        }
        "game.session.save" => {
            if st
                .sentinels_v6
                .as_ref()
                .is_some_and(|s| s.playback.is_some())
            {
                return Err(failure("请保存原始回放文件，观看状态不能作为权威存档"));
            }
            let g = st
                .sentinels_v6
                .as_ref()
                .and_then(|s| s.game.as_ref())
                .ok_or_else(|| failure("只有权威可保存"))?;
            Ok(json!(g.save()))
        }
        "game.session.load" => {
            let save: Save = serde_json::from_value(p["save"].clone()).map_err(failure)?;
            let game = Game::load(save).map_err(failure)?;
            crate::sentinels_v6_render::reset_scene();
            let snapshot = game.state.clone();
            let view = st
                .sentinels_v6
                .as_ref()
                .map(|s| s.view.clone())
                .unwrap_or_default();
            st.sentinels_v6 = Some(Session {
                clock_epoch:fresh_clock_epoch(),
                metrics:Metrics::default(),
                game: Some(game),
                snapshot: snapshot.clone(),
                view,
                replica: false,
                replica_fresh: false,
                playback: None,
            });
            st.play = PlayState::Running;
            publish(st);
            Ok(json!({"snapshot":snapshot}))
        }
        "game.session.replay" => {
            let save: Save = serde_json::from_value(p["save"].clone()).map_err(failure)?;
            save.validate().map_err(failure)?;
            if p["playback"] == true {
                crate::sentinels_v6_render::reset_scene();
                let game =
                    Game::new_theme(save.snapshot.seed, save.initialAi, &save.snapshot.theme);
                let snapshot = game.state.clone();
                let view = st
                    .sentinels_v6
                    .as_ref()
                    .map(|s| s.view.clone())
                    .unwrap_or_default();
                st.sentinels_v6 = Some(Session {
                    clock_epoch:fresh_clock_epoch(),
                    metrics:Metrics::default(),
                    game: Some(game),
                    snapshot: snapshot.clone(),
                    view,
                    replica: false,
                    replica_fresh: false,
                    playback: Some(ReplayController::new(save).map_err(failure)?),
                });
                st.play = PlayState::Running;
                sync_replay_snapshot(st);
                let snapshot=st.sentinels_v6.as_ref().unwrap().snapshot.clone();
                Ok(json!({"snapshot":snapshot,"playback":true}))
            } else {
                Ok(
                    json!({"matched":Game::replay(&save).map_err(failure)?,"tick":save.snapshot.tick}),
                )
            }
        }
        "game.session.replayControl" => {
            let s = st
                .sentinels_v6
                .as_mut()
                .ok_or_else(|| failure("无回放会话"))?;
            let replay = s.playback.as_mut().ok_or_else(|| failure("当前不是回放"))?;
            if let Some(paused) = p["paused"].as_bool() {
                replay.paused = paused;
            }
            if let Some(speed) = p["speed"].as_f64() {
                if !speed.is_finite() {
                    return Err(failure("速度无效"));
                }
                replay.speed = speed.clamp(0.5, 8.);
                replay.phase = 0.;
            }
            if let Some(tick) = p["seekTick"].as_u64() {
                let target = tick.min(replay.save.snapshot.tick);
                let game = s.game.as_mut().ok_or_else(|| failure("回放尚未就绪"))?;
                if target < game.state.tick {
                    *game = replay.rewind();
                    crate::sentinels_v6_render::reset_scene();
                }
                replay.seek = Some(target);
                replay.paused = false;
            }
            if p.get("paused").is_some()||p.get("speed").is_some()||p.get("seekTick").is_some(){s.clock_epoch=fresh_clock_epoch();}
            let result=json!({"paused":replay.paused,"speed":replay.speed,"seeking":replay.seek});
            sync_replay_snapshot(st);
            Ok(result)
        }
        "game.session.forfeit" => {
            let s = st
                .sentinels_v6
                .as_mut()
                .ok_or_else(|| failure("会话未启动"))?;
            let g = s.game.as_mut().ok_or_else(|| failure("只有权威可判负"))?;
            g.forfeit(
                p["owner"].as_u64().unwrap_or(0) as u32,
                p["reason"].as_str().unwrap_or("退出战斗").into(),
            )
            .map_err(failure)?;
            s.snapshot = g.state.clone();
            let result = json!({"winner":s.snapshot.winner,"winReason":s.snapshot.winReason});
            publish(st);
            Ok(result)
        }
        _ => Err((-32601, "未知V6游戏方法".into())),
    }
}
pub fn advance(st: &mut HostState) {
    let advance_start=Instant::now();let mut stepped=false;
    let mut dirty = false;
    if let Some(s) = st.sentinels_v6.as_mut() {
        if let Some(g) = s.game.as_mut() {
            if let Some(replay) = s.playback.as_mut() {
                replay.advance(g);
                g.state.playback = Some(PlaybackStatus {
                    paused: replay.paused,
                    speed: replay.speed,
                    currentTick: g.state.tick,
                    totalTicks: replay.save.snapshot.tick,
                });
            } else {
                let previous=g.state.tick;let step_start=Instant::now();
                g.step();
                if g.state.tick!=previous{sample(&mut s.metrics.step,step_start.elapsed().as_secs_f64()*1000.);stepped=true;}
            }
            if (g.state.tick != s.snapshot.tick && g.state.tick % 3 == 0)
                || g.state.winner != s.snapshot.winner
                || s.playback.is_some()
            {
                let clone_start=Instant::now();s.snapshot = g.state.clone();sample(&mut s.metrics.clone,clone_start.elapsed().as_secs_f64()*1000.);
                dirty = true;
            }
        }
    }
    if dirty {
        publish(st);
    }
    if stepped{if let Some(s)=st.sentinels_v6.as_mut(){sample(&mut s.metrics.advance,advance_start.elapsed().as_secs_f64()*1000.);}}
}
pub fn publish(st: &mut HostState) {
    let publish_start=Instant::now();
    let Some(s) = st.sentinels_v6.as_ref() else {
        return;
    };
    let world = if let Some(g) = &s.game {
        g.snapshot(Some(s.view.local_player))
    } else {
        s.snapshot.clone()
    };
    st.run_scene = Some(crate::sentinels_v6_render::scene(&world, &s.view));
    let (x, y) = iso(s.view.center_x, s.view.center_y, s.view.layer as f64);
    st.camera.target = [x as f32, y as f32, 0.];
    st.camera.yaw_deg = 0.;
    st.camera.pitch_deg = 0.;
    st.camera.dist = 100.;
    st.camera.ortho = true;
    st.camera.ortho_half_h = (24. / s.view.zoom) as f32;
    st.scene_rev += 1;
    if let Some(s)=st.sentinels_v6.as_mut(){sample(&mut s.metrics.publish,publish_start.elapsed().as_secs_f64()*1000.);}
}
pub fn iso(x: f64, y: f64, z: f64) -> (f64, f64) {
    ((x - y) * 0.5, -(x + y) * 0.25 + z * 1.5)
}

#[cfg(test)]
mod planning_contracts {
    use super::*;
    fn session(seed:u64)->Session{
        let game=Game::new_theme(seed,false,"river");
        Session{clock_epoch:fresh_clock_epoch(),metrics:Metrics::default(),snapshot:game.state.clone(),game:Some(game),view:View::default(),replica:false,replica_fresh:false,playback:None}
    }
    fn host()->Mutex<HostState>{let mut st=HostState::new();st.sentinels_v6=Some(session(811));st.play=PlayState::Running;Mutex::new(st)}
    #[test]
    fn suggestion_releases_host_lock_before_computation_and_keeps_authority_unchanged(){
        let state=host();
        let before=serde_json::to_value(crate::rpc::lock(&state).sentinels_v6.as_ref().unwrap().game.as_ref().unwrap().save()).unwrap();
        let result=with_captured_suggestion(&state,&json!({"owner":1}),|captured|{
            let guard=state.try_lock().expect("planning must not retain HostState");drop(guard);
            captured.plan()
        }).unwrap();
        assert!(!result["commands"].as_array().unwrap().is_empty());
        assert_eq!(result["tick"],0);
        let st=crate::rpc::lock(&state);let s=st.sentinels_v6.as_ref().unwrap();
        assert_eq!(serde_json::to_value(s.game.as_ref().unwrap().save()).unwrap(),before);
        let metrics=s.metrics.planning.lock().unwrap();assert_eq!(metrics.capture.len(),1);assert_eq!(metrics.plan.len(),1);
    }
    #[test]
    fn stale_advice_does_not_replace_current_authority_or_skip_its_order_checks(){
        let state=host();
        let result=with_captured_suggestion(&state,&json!({"owner":1}),|captured|{
            std::thread::scope(|scope|scope.spawn(||{
                let mut guard=state.try_lock().expect("authority remains accessible during planning");
                guard.sentinels_v6.as_mut().unwrap().game.as_mut().unwrap().forfeit(1,"planning-contract".into()).unwrap();
            }).join().unwrap());
            captured.plan()
        }).unwrap();
        let mut st=crate::rpc::lock(&state);let game=st.sentinels_v6.as_mut().unwrap().game.as_mut().unwrap();
        let credits=game.player(1).unwrap().credits;
        let receipt=game.order(Order{owner:1,sequence:1,command:serde_json::from_value(result["command"].clone()).unwrap()});
        assert!(!receipt.accepted);assert_eq!(game.state.winner,Some(2));assert_eq!(game.player(1).unwrap().credits,credits);
    }
    #[test]
    fn replaced_session_does_not_receive_old_planning_metrics_and_invalid_owner_cannot_wrap(){
        let state=host();let old=Arc::clone(&crate::rpc::lock(&state).sentinels_v6.as_ref().unwrap().metrics.planning);
        with_captured_suggestion(&state,&json!({"owner":2}),|captured|{
            crate::rpc::lock(&state).sentinels_v6=Some(session(812));captured.plan()
        }).unwrap();
        assert_eq!(old.lock().unwrap().plan.len(),1);
        {let st=crate::rpc::lock(&state);let metrics=st.sentinels_v6.as_ref().unwrap().metrics.planning.lock().unwrap();assert!(metrics.plan.is_empty());assert!(metrics.capture.is_empty());}
        assert!(suggest(&state,&json!({"owner":4294967297u64})).is_err());
        crate::rpc::lock(&state).sentinels_v6.as_mut().unwrap().game=None;
        assert!(suggest(&state,&json!({"owner":1})).is_err());
    }
    #[test]
    fn pause_resume_between_scheduler_polls_still_invalidates_v6_clock_debt(){
        let state=host();let before=crate::sentinels_v6_clock::domain(&crate::rpc::lock(&state));
        for method in ["play.pause","play.resume"]{let result=crate::rpc::dispatch(&state,&json!({"id":1,"method":method,"params":{}}));assert!(result.get("error").is_none(),"{result}");}
        let after=crate::sentinels_v6_clock::domain(&crate::rpc::lock(&state));assert_ne!(before,after);
        let mut clock=crate::sentinels_v6_clock::Clock::default();clock.observe(0.,before);clock.observe(2.,before);clock.observe(10.,after);assert_eq!(clock.debt(),0.);
    }
    #[test]
    fn construction_preview_releases_lock_and_projects_only_its_private_copy(){
        let state=host();let before=serde_json::to_value(crate::rpc::lock(&state).sentinels_v6.as_ref().unwrap().game.as_ref().unwrap().save()).unwrap();
        let command=sentinels_v6::Command::Shell{rect:sentinels_v6::Rect{x:13,y:46,level:0,width:6,height:4}};
        let result=with_captured_game(&state,&json!({"owner":1}),true,|copy,owner|{
            assert!(state.try_lock().is_ok(),"network/placement preview must be outside HostState");preview_projection(copy,owner,command)
        }).unwrap();
        assert_eq!(result["valid"],true,"{result}");assert_eq!(result["completionProjection"],true);assert!(result["cost"]["credits"].as_f64().unwrap()>0.);
        let st=crate::rpc::lock(&state);let s=st.sentinels_v6.as_ref().unwrap();assert_eq!(serde_json::to_value(s.game.as_ref().unwrap().save()).unwrap(),before);assert_eq!(s.metrics.preview.lock().unwrap().plan.len(),1);assert!(s.metrics.planning.lock().unwrap().plan.is_empty());
    }
    #[test]
    fn paused_replay_publishes_controller_state_without_requiring_a_simulation_step(){
        let state=host();let mut st=crate::rpc::lock(&state);let save=st.sentinels_v6.as_ref().unwrap().game.as_ref().unwrap().save();
        let opened=handle(&mut st,"game.session.replay",&json!({"save":save,"playback":true})).unwrap();assert_eq!(opened["snapshot"]["playback"]["paused"],false);assert!(matches!(crate::sentinels_v6_clock::domain(&st),crate::sentinels_v6_clock::Domain::V6(_)));
        handle(&mut st,"game.session.replayControl",&json!({"paused":false,"speed":2.})).unwrap();let snapshot=handle(&mut st,"game.session.snapshot",&json!({})).unwrap();assert_eq!(snapshot["playback"]["paused"],false);assert_eq!(snapshot["playback"]["speed"],2.);assert_eq!(snapshot["tick"],0);
        handle(&mut st,"game.session.replayControl",&json!({"paused":true})).unwrap();let snapshot=handle(&mut st,"game.session.snapshot",&json!({})).unwrap();assert_eq!(snapshot["playback"]["paused"],true);assert_eq!(snapshot["tick"],0);assert_eq!(crate::sentinels_v6_clock::domain(&st),crate::sentinels_v6_clock::Domain::Stopped);assert!(st.sentinels_v6.as_ref().unwrap().metrics.step.is_empty());
    }
}
