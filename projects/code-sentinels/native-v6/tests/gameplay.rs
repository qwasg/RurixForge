use sentinels_v6::{Command, Game, Order, Pos, Rect};
#[test]
fn paid_floor_transit_refuses_wire_without_losing_motion_or_save_validity(){
    let mut g=opening();let lab=g.state.rooms.iter().find(|r|r.owner==1&&r.kind=="research-lab").unwrap().id;
    wait(&mut g,20);cmd(&mut g,1,Command::Research{room:lab,branch:"algorithm".into()});wait(&mut g,70);
    cmd(&mut g,1,Command::Shell{rect:Rect{x:13,y:46,level:1,width:6,height:4}});
    cmd(&mut g,1,Command::Entrance{pos:Pos::new(15,48,0),to_level:1,kind:"stairs".into(),width:1});wait(&mut g,30);
    while g.player(1).unwrap().credits<sentinels_v6::catalog::unit_ref("deepseek").unwrap().cost+30.||g.player(1).unwrap().compute<120.{wait(&mut g,1);assert!(g.state.tick<900*60);}
    cmd(&mut g,1,Command::Deploy{room:lab,kind:"deepseek".into(),pos:Pos::new(14,48,0)});
    let actor=g.state.units.iter().find(|u|u.owner==1&&u.kind=="deepseek").unwrap().id;
    cmd(&mut g,1,Command::Move{ids:vec![actor],pos:Pos::new(15,48,1)});
    for _ in 0..180{if g.state.units.iter().find(|u|u.id==actor).unwrap().transitProgress>0.{break;}g.step();}
    let before=g.state.units.iter().find(|u|u.id==actor).unwrap().clone();assert!(before.transitProgress>0.&&before.transitProgress<1.);assert_eq!((before.velocityX,before.velocityY),(0.,0.));
    let cash=g.player(1).unwrap().credits;let compute=g.player(1).unwrap().compute;let links=g.state.links.len();
    let path=vec![before.pos,Pos::new(before.pos.x+1,before.pos.y,before.pos.level)];
    // A malformed external save must also fail if it disguises the physical
    // binding by leaving Unit.wired=false while retaining a valid shaft edge.
    let mut bad=g.save();bad.snapshot.links.push(sentinels_v6::Link{id:bad.nextId,owner:1,kind:"compute".into(),path:path.clone(),hp:150.,active:false,unitEndpoints:vec![actor],invested:6.});bad.nextId+=1;
    assert!(Game::load(bad).is_err());
    let receipt=g.order(Order{owner:1,sequence:g.sequences[0]+1,command:Command::Wire{kind:"compute".into(),path,unit_endpoints:vec![actor]}});
    assert!(!receipt.accepted&&receipt.reason.contains("换层"),"{}",receipt.reason);
    assert_eq!(g.player(1).unwrap().credits,cash);assert_eq!(g.player(1).unwrap().compute,compute);assert_eq!(g.state.links.len(),links);
    let after=g.state.units.iter().find(|u|u.id==actor).unwrap();assert_eq!(after.route,before.route);assert_eq!(after.transitProgress,before.transitProgress);assert!(!after.wired);
    let mut restored=Game::load(g.save()).unwrap();
    for _ in 0..180{let old_level=g.state.units.iter().find(|u|u.id==actor).unwrap().level;g.step();restored.step();let current=g.state.units.iter().find(|u|u.id==actor).unwrap();if current.level!=old_level||current.transitProgress>0.{assert_eq!((current.velocityX,current.velocityY),(0.,0.));}}
    let arrived=g.state.units.iter().find(|u|u.id==actor).unwrap();assert_eq!(arrived.level,1);assert_eq!(arrived.transitProgress,0.);assert_eq!(arrived.pos,Pos::new(15,48,1));
    assert_eq!(serde_json::to_value(&restored.state).unwrap(),serde_json::to_value(&g.state).unwrap());
    // Ordinary same-floor movement is still a legal time to install a tether.
    cmd(&mut g,1,Command::Move{ids:vec![actor],pos:Pos::new(16,48,1)});g.step();
    let moving=g.state.units.iter().find(|u|u.id==actor).unwrap();assert_eq!(moving.transitProgress,0.);assert!(!moving.route.is_empty());let p=moving.pos;
    cmd(&mut g,1,Command::Wire{kind:"compute".into(),path:vec![p,Pos::new(p.x,p.y+1,p.level)],unit_endpoints:vec![actor]});
    assert!(g.state.units.iter().find(|u|u.id==actor).unwrap().wired);Game::load(g.save()).unwrap();assert!(Game::replay(&g.save()).unwrap());
}
#[test]
fn save_rule_identity_is_required_by_load_and_both_replay_entries(){
    let save=Game::new(79,false).save();assert_eq!(save.rulesVersion,sentinels_v6::RULES_VERSION);assert_eq!(save.rulesFingerprint,sentinels_v6::RULES_FINGERPRINT);assert_eq!(save.rulesFingerprint.len(),64);assert!(save.rulesFingerprint.bytes().all(|c|c.is_ascii_hexdigit()));
    assert!(Game::load(save.clone()).is_ok());assert!(Game::replay(&save).unwrap());assert!(sentinels_v6::ReplayController::new(save.clone()).is_ok());
    let mut legacy=serde_json::to_value(&save).unwrap();legacy.as_object_mut().unwrap().remove("rulesVersion");legacy.as_object_mut().unwrap().remove("rulesFingerprint");legacy["allowLegacy"]=serde_json::json!(true);
    let legacy:sentinels_v6::Save=serde_json::from_value(legacy).unwrap();assert!(legacy.rulesFingerprint.is_empty());
    let mut wrong_hash=save.clone();wrong_hash.rulesFingerprint.replace_range(0..1,if save.rulesFingerprint.starts_with('0'){"1"}else{"0"});
    let mut wrong_version=save;wrong_version.rulesVersion.push_str("-other");
    for rejected in [legacy,wrong_hash,wrong_version]{assert!(rejected.validate().unwrap_err().contains("指纹"));assert!(Game::load(rejected.clone()).is_err());assert!(Game::replay(&rejected).is_err());assert!(sentinels_v6::ReplayController::new(rejected).is_err());}
}
#[test]
fn paid_independent_wings_collapse_only_above_removed_support_and_replay(){
    // New ordinary-command integration coverage, not a relocated synthetic collapse fixture.
    fn paid(g:&mut Game,c:Command){
        loop{let mut preview=g.clone();match preview.execute(1,c.clone()){Ok(())=>break,Err(reason) if reason.contains("金币不足")||reason.contains("算力不足")=>{wait(g,1);assert!(g.state.tick<900*60,"paid scenario cannot fund command: {reason}");},Err(reason)=>panic!("public command preview rejected: {reason}")}}
        cmd(g,1,c);
    }
    fn complete(g:&mut Game,r:Rect)->u64{for _ in 0..90{if let Some(b)=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell"&&b.rect==r&&b.progress>=1.){return b.id;}wait(g,1);}panic!("paid floor did not complete: {r:?}; jobs {:?}",g.state.jobs);}
    let mut g=opening();let lab=g.state.rooms.iter().find(|r|r.owner==1&&r.kind=="research-lab").unwrap().id;
    paid(&mut g,Command::Research{room:lab,branch:"algorithm".into()});wait(&mut g,70);assert_eq!(g.tech(1,"algorithm"),2);
    let lower_a=g.state.buildings.iter().find(|b|b.owner==1&&b.kind=="shell").unwrap().clone();
    let upper_a=Rect{level:1,..lower_a.rect};paid(&mut g,Command::Shell{rect:upper_a});let a_top=complete(&mut g,upper_a);
    paid(&mut g,Command::Entrance{pos:Pos::new(14,48,0),to_level:1,kind:"stairs".into(),width:1});
    let lower_b=Rect{x:2,y:38,level:0,width:6,height:4};paid(&mut g,Command::Shell{rect:lower_b});let b_base=complete(&mut g,lower_b);
    paid(&mut g,Command::Entrance{pos:Pos::new(3,41,0),to_level:0,kind:"door".into(),width:2});
    let upper_b=Rect{level:1,..lower_b};paid(&mut g,Command::Shell{rect:upper_b});let b_top=complete(&mut g,upper_b);
    paid(&mut g,Command::Entrance{pos:Pos::new(4,39,0),to_level:1,kind:"stairs".into(),width:1});
    paid(&mut g,Command::Deploy{room:lab,kind:"deepseek".into(),pos:Pos::new(14,48,0)});let actor=g.state.units.iter().find(|u|u.owner==1&&u.kind=="deepseek").unwrap().id;
    cmd(&mut g,1,Command::Move{ids:vec![actor],pos:Pos::new(15,48,1)});wait(&mut g,10);
    assert_eq!(g.state.units.iter().find(|u|u.id==actor).unwrap().level,1);assert!(g.walkable(Pos::new(15,48,1),"ai"));
    cmd(&mut g,1,Command::Recycle{id:lower_a.id});wait(&mut g,6);
    assert!(!g.state.buildings.iter().any(|b|b.id==a_top));
    assert!(g.state.buildings.iter().any(|b|b.id==b_base&&b.hp>0.));assert!(g.state.buildings.iter().any(|b|b.id==b_top&&b.hp>0.&&b.supportRatio>=0.99));
    let survivor=g.state.units.iter().find(|u|u.id==actor).expect("one-floor fall should leave a survivor");assert_eq!(survivor.level,0);assert!(survivor.hp>0.&&survivor.hp<survivor.maxHp);assert!(survivor.route.is_empty());
    assert!(g.state.events.iter().any(|e|e.kind=="collapse"&&e.subject==a_top));assert!(!g.walkable(Pos::new(15,48,1),"ai"));assert!(g.walkable(Pos::new(4,39,1),"ai"));
    let save=g.save();assert_eq!(serde_json::to_value(&Game::load(save.clone()).unwrap().state).unwrap(),serde_json::to_value(&g.state).unwrap());assert!(Game::replay(&save).unwrap(),"paid construction/collapse must replay exactly");
}
#[test]
fn event_ring_preserves_exact_array_order_after_wrap_and_save_load(){
    let mut g=Game::new(78,false);let mut expected=Vec::new();
    for i in 0..5000{g.event("ring-fixture",Pos::new(10,48,0),1,i as f64,0);expected.push(g.state.events.last().unwrap().clone());}
    let expected=expected.split_off(expected.len()-4096);
    assert_eq!(g.state.events.len(),4096);
    assert_eq!(serde_json::to_value(&g.state.events).unwrap(),serde_json::to_value(&expected).unwrap());
    let save=g.save();let encoded=serde_json::to_string(&save).unwrap();let loaded=Game::load(serde_json::from_str(&encoded).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(&loaded.state).unwrap(),serde_json::to_value(&g.state).unwrap());
    assert_eq!(g.state.events.iter().rev().map(|e|e.id).collect::<Vec<_>>(),expected.iter().rev().map(|e|e.id).collect::<Vec<_>>());
}
#[test]
fn extractor_orders_and_preview_clones_require_remaining_ore_or_coal(){
    // Isolated resource-state cases; the ordinary initial credits and Build order are real.
    for (kind,remaining,valid) in [("ore",100.,true),("coal",100.,true),("ore",0.,false),("coal",0.,false),("salvage-ore",100.,false),("salvage",100.,false)] {
        let mut g=Game::new(77,false);g.state.terrain.fill(0);g.state.resources.clear();g.next_id=1000;
        g.state.resources.push(sentinels_v6::Resource{id:900,pos:Pos::new(17,41,0),kind:kind.into(),remaining,owner:0,capture:0.,contested:false,capturer:0});g.refresh_fog();
        let command=Command::Build{pos:Pos::new(16,40,0),kind:"extractor".into()};let cash=g.player(1).unwrap().credits;let buildings=g.state.buildings.len();
        // game.session.preview invokes the identical execute on a private Game clone.
        let mut preview=g.clone();assert_eq!(preview.execute(1,command.clone()).is_ok(),valid,"preview {kind}/{remaining}");assert_eq!(g.player(1).unwrap().credits,cash);
        let receipt=g.order(Order{owner:1,sequence:1,command});assert_eq!(receipt.accepted,valid,"order {kind}/{remaining}: {}",receipt.reason);
        if valid{assert_eq!(g.state.buildings.len(),buildings+1);assert!(g.player(1).unwrap().credits<cash);}else{assert_eq!(g.state.buildings.len(),buildings);assert_eq!(g.player(1).unwrap().credits,cash);assert_eq!(preview.player(1).unwrap().credits,cash);}
    }
}
fn cmd(g: &mut Game, owner: u32, command: Command) {
    let sequence = g.sequences[(owner - 1) as usize] + 1;
    let receipt = g.order(Order {
        owner,
        sequence,
        command,
    });
    assert!(receipt.accepted, "{}", receipt.reason);
}
fn wait(g: &mut Game, seconds: u64) {
    for _ in 0..seconds * 60 {
        g.step();
    }
}
fn line(a: Pos, b: Pos) -> Vec<Pos> {
    let mut p = a;
    let mut out = vec![p];
    while p.x != b.x {
        p.x += (b.x - p.x).signum();
        out.push(p);
    }
    while p.y != b.y {
        p.y += (b.y - p.y).signum();
        out.push(p);
    }
    out
}
fn opening() -> Game {
    let mut g = Game::new(1, false);
    cmd(
        &mut g,
        1,
        Command::Shell {
            rect: Rect {
                x: 13,
                y: 46,
                level: 0,
                width: 6,
                height: 4,
            },
        },
    );
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(13, 42, 0),
            kind: "wind-power".into(),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Build {
            pos: Pos::new(16, 40, 0),
            kind: "extractor".into(),
        },
    );
    wait(&mut g, 20);
    let shell = g
        .state
        .buildings
        .iter()
        .find(|b| b.kind == "shell")
        .unwrap()
        .id;
    assert_eq!(
        g.state
            .buildings
            .iter()
            .find(|b| b.id == shell)
            .unwrap()
            .progress,
        1.
    );
    cmd(
        &mut g,
        1,
        Command::Entrance {
            pos: Pos::new(13, 48, 0),
            to_level: 0,
            kind: "door".into(),
            width: 1,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Room {
            shell,
            rect: Rect {
                x: 13,
                y: 46,
                level: 0,
                width: 2,
                height: 2,
            },
            kind: "data-center".into(),
            branch: None,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Room {
            shell,
            rect: Rect {
                x: 17,
                y: 46,
                level: 0,
                width: 2,
                height: 2,
            },
            kind: "research-lab".into(),
            branch: Some("algorithm".into()),
        },
    );
    wait(&mut g, 25);
    let dc = g
        .state
        .rooms
        .iter()
        .find(|r| r.kind == "data-center")
        .unwrap()
        .id;
    let lab = g
        .state
        .rooms
        .iter()
        .find(|r| r.kind == "research-lab")
        .unwrap()
        .id;
    assert!(
        g.state.rooms.iter().all(|r| r.progress >= 1.),
        "{:?}",
        g.state.jobs
    );
    let mut power = line(Pos::new(13, 42, 0), Pos::new(13, 46, 0));
    power.extend(
        line(Pos::new(13, 46, 0), Pos::new(18, 46, 0))
            .into_iter()
            .skip(1),
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "power".into(),
            path: power,
        },
    );
    cmd(
        &mut g,
        1,
        Command::InstallGpu {
            room: dc,
            model: "rtx-5060".into(),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: line(Pos::new(13, 46, 0), Pos::new(18, 46, 0)),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Deploy {
            room: lab,
            kind: "vscode".into(),
            pos: Pos::new(12, 52, 0),
        },
    );
    cmd(
        &mut g,
        1,
        Command::Deploy {
            room: lab,
            kind: "pycharm".into(),
            pos: Pos::new(10, 52, 0),
        },
    );
    let mut cable = line(Pos::new(13, 46, 0), Pos::new(12, 46, 0));
    cable.extend(
        line(Pos::new(12, 46, 0), Pos::new(12, 52, 0))
            .into_iter()
            .skip(1),
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: cable,
        },
    );
    cmd(
        &mut g,
        1,
        Command::Wire {
            unit_endpoints: vec![],
            kind: "compute".into(),
            path: line(Pos::new(12, 52, 0), Pos::new(10, 52, 0)),
        },
    );
    wait(&mut g, 2);
    g
}
#[test]
fn public_opening_has_real_power_compute_and_cash_reserve() {
    let g = opening();
    assert!(
        g.player(1).unwrap().credits >= 500.,
        "credits {}",
        g.player(1).unwrap().credits
    );
    assert!(g.state.rooms.iter().all(|r| r.powered && r.connected));
    assert!(g.player(1).unwrap().production >= 12.);
    assert_eq!(g.state.units.len(), 2);
    assert!(g.state.units.iter().all(|u| u.covered));
    assert_eq!(
        g.state
            .resources
            .iter()
            .filter(|r| r.kind == "node")
            .count(),
        3
    );
}
#[test]
fn first_lab_required_and_tier_one_cannot_build_basement_or_tall_floor() {
    let mut g = Game::new(1, false);
    let before = g.player(1).unwrap().credits;
    for command in [
        Command::Deploy {
            room: 0,
            kind: "vscode".into(),
            pos: Pos::new(14, 45, 0),
        },
        Command::Excavate {
            rect: Rect {
                x: 14,
                y: 44,
                level: -1,
                width: 4,
                height: 4,
            },
        },
        Command::Shell {
            rect: Rect {
                x: 14,
                y: 44,
                level: 1,
                width: 4,
                height: 4,
            },
        },
        Command::Shell {
            rect: Rect {
                x: 14,
                y: 44,
                level: 0,
                width: 2,
                height: 4,
            },
        },
    ] {
        let s = g.sequences[0] + 1;
        assert!(
            !g.order(Order {
                owner: 1,
                sequence: s,
                command
            })
            .accepted
        );
    }
    assert_eq!(g.player(1).unwrap().credits, before);
}
#[test]
fn duplicate_command_is_idempotent_and_wrong_owner_cannot_recycle() {
    let mut g = opening();
    let command = Command::Build {
        pos: Pos::new(15, 54, 0),
        kind: "wind-power".into(),
    };
    let order = Order {
        owner: 1,
        sequence: g.sequences[0] + 1,
        command,
    };
    let first = g.order(order.clone());
    assert!(first.accepted);
    let credits = g.player(1).unwrap().credits;
    assert!(g.order(order).accepted);
    assert_eq!(credits, g.player(1).unwrap().credits);
    let target = g
        .state
        .buildings
        .iter()
        .find(|b| b.owner == 1 && b.kind == "shell")
        .unwrap()
        .id;
    assert!(
        !g.order(Order {
            owner: 2,
            sequence: 1,
            command: Command::Recycle { id: target }
        })
        .accepted
    );
}
#[test]
fn save_roundtrip_and_public_command_replay_match() {
    let g = opening();
    let save = g.save();
    let json = serde_json::to_string(&save).unwrap();
    let restored = Game::load(serde_json::from_str(&json).unwrap()).unwrap();
    let a = serde_json::to_value(restored.state).unwrap();
    let b = serde_json::to_value(&g.state).unwrap();
    assert!(a == b, "{}", first_diff(&a, &b, "root"));
    assert!(Game::replay(&save).unwrap());
}
#[test]
fn gpu_removal_recomputes_network_without_duplicating_refund() {
    let mut g = opening();
    let dc = g
        .state
        .rooms
        .iter()
        .find(|r| r.kind == "data-center")
        .unwrap()
        .id;
    cmd(&mut g, 1, Command::RemoveGpu { room: dc, bay: 0 });
    let credits = g.player(1).unwrap().credits;
    assert_eq!(g.player(1).unwrap().production, 0.);
    assert!(
        !g.order(Order {
            owner: 1,
            sequence: g.sequences[0] + 1,
            command: Command::RemoveGpu { room: dc, bay: 0 }
        })
        .accepted
    );
    assert_eq!(credits, g.player(1).unwrap().credits);
}

fn first_diff(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> String {
    if a == b {
        return String::new();
    }
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in a {
                let other = b.get(k).unwrap_or(&serde_json::Value::Null);
                if v != other {
                    return first_diff(v, other, &format!("{path}.{k}"));
                }
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            if a.len() != b.len() {
                return format!("{path} lengths {} != {}", a.len(), b.len());
            }
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                if x != y {
                    return first_diff(x, y, &format!("{path}[{i}]"));
                }
            }
        }
        _ => return format!("{path}: {a} != {b}"),
    }
    format!("{path}: mismatch")
}
