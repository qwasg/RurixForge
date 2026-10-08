//! Isolated unearned native trajectory fixtures, not campaign or balance evidence.
use sentinels_v6::{catalog, Game, Pos, Projectile, Unit, Wall};
use serde_json::json;

const SHOOTER: u64 = 10001;
const TARGET: u64 = 10002;

fn actor(id: u64, owner: u32, kind: &str, x: i32, y: i32) -> Unit {
    let d = catalog::unit_ref(kind).unwrap();
    serde_json::from_value(json!({
        "id":id,"owner":owner,"kind":kind,"pos":{"x":x,"y":y,"z":0},
        "x":x as f64 + 0.5,"y":y as f64 + 0.5,"z":0,"tier":d.tier,
        "hp":1000.,"maxHp":1000.,"battery":1000.,"batteryMax":1000.,"covered":false,"wired":false,
        "ammo":1000.,"ammoMax":1000.,"energy":1000.,"energyMax":1000.,"fuel":1000.,"fuelMax":1000.,
        "route":[],"target":null,"cooldown":1000.,"skillCooldown":0.,"plugins":[],"statuses":{},
        "invested":d.cost,"moving":false,"attackCount":0,"branch":d.branch,"facing":0,
        "altitude":0.,"flightState":"ground","sourceFacility":0
    })).unwrap()
}

fn world(kind: &str, distance: i32, moving: Option<(i32, i32)>) -> Game {
    let mut g = Game::new(602113, false);
    g.state.terrain.fill(0);
    g.state.rooms.clear();
    g.state.units.clear();
    g.state.links.clear();
    g.state.walls.clear();
    g.state.entrances.clear();
    g.state.jobs.clear();
    g.state.resources.clear();
    g.state.shipments.clear();
    g.state.projectiles.clear();
    g.state.events.clear();
    g.state.defenseFields.clear();
    g.state.shieldRegions.clear();
    g.next_id = 20000;
    for b in &mut g.state.buildings {
        b.stock.clear();
        b.inventory = 0.;
    }
    let mut shooter = actor(SHOOTER, 1, kind, 40, 45);
    shooter.cooldown = 0.;
    shooter.target = Some(TARGET);
    let mut target = actor(TARGET, 2, "algorithm-scout", 40 + distance, 45);
    if let Some((dx, dy)) = moving {
        target.route = (1..=8).map(|n| Pos::new(40 + distance + n * dx, 45 + n * dy, 0)).collect();
        target.goal = target.route.last().copied();
        target.moving = true;
    }
    g.state.units.extend([shooter, target]);
    g.refresh_fog();
    g
}

fn hp(g: &Game, id: u64) -> f64 {
    g.state.units.iter().find(|u| u.id == id).unwrap().hp
}

fn steps(g: &mut Game, count: usize) {
    for _ in 0..count { g.step(); }
}

fn fire_once(g: &mut Game) -> Vec<Projectile> {
    g.step();
    let shooter = g.state.units.iter_mut().find(|u| u.id == SHOOTER).unwrap();
    assert_eq!(shooter.attackCount, 1, "fixture must fire through the real native weapon path");
    shooter.cooldown = 1000.;
    g.state.projectiles.iter().filter(|p| p.source == SHOOTER).cloned().collect()
}

// Use the native range metric: an inter-floor displacement is four cells.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0]-b[0]).powi(2)+(a[1]-b[1]).powi(2)+((a[2]-b[2])*4.).powi(2)).sqrt()
}

fn move_fixture_actor(g: &mut Game, id: u64, x: f64, y: f64) {
    let u=g.state.units.iter_mut().find(|u|u.id==id).unwrap();
    u.x=x;u.y=y;u.pos=Pos::new(x.floor() as i32,y.floor() as i32,0);
    u.route.clear();u.goal=None;u.moving=false;
}

fn wall(g: &mut Game, x: i32) {
    g.state.walls.push(serde_json::from_value::<Wall>(json!({
        "id":10003,"owner":2,"pos":{"x":x,"y":45,"z":0},"kind":"physical",
        "hp":1000.,"maxHp":1000.,"shield":0.,"invested":0.
    })).unwrap());
}

fn branch_chassis(branch: &str, chassis: &str) -> String {
    catalog::units().into_iter().find(|d|d.branch==branch&&d.chassis==chassis).unwrap().id
}

fn orbital_prerequisites(g: &mut Game) {
    // Explicit unearned controller, local compute reservoir and powered pad.
    // These are real required native entities, not a bypass of can-fire checks.
    g.state.rooms.push(serde_json::from_value(json!({
        "id":30001,"shell":1,"owner":1,"rect":{"x":20,"y":20,"z":0,"w":4,"h":4},
        "kind":"orbital-control","equipmentShare":1.,"branch":null,"tier":5,"hp":1000.,"maxHp":1000.,
        "powered":true,"connected":true,"online":true,"capacity":16,"gpus":[],"inventory":0.,
        "progress":1.,"buildTime":1.,"cooldown":0.,"stock":{},"invested":0.
    })).unwrap());
    g.state.networkStores.push(serde_json::from_value(json!({
        "owner":1,"anchor":{"x":22,"y":22,"z":0},"cells":[{"x":22,"y":22,"z":0}],
        "compute":1000.,"capacity":1000.,"production":0.
    })).unwrap());
    let mut pad=g.state.buildings[0].clone();
    pad.id=30002;pad.owner=1;pad.kind="launch-pad".into();pad.rect.x=24;pad.rect.y=20;
    pad.powered=true;pad.progress=1.;pad.hp=1000.;pad.maxHp=1000.;
    g.state.buildings.push(pad);
}

#[test]
fn direct_catches_retreating_target_before_maximum_range() {
    println!("compiled rules {} {}", sentinels_v6::RULES_VERSION, sentinels_v6::RULES_FINGERPRINT);
    let mut g = world("light-tank", 10, Some((1, 0)));
    let launched = fire_once(&mut g);
    assert_eq!(launched.len(), 1);
    let ammo_after_launch = g.state.units[0].ammo;
    steps(&mut g, 40);
    assert!(hp(&g, TARGET) < 1000.,
        "28-speed direct round expired at the old aim before catching the 3-speed retreating target within range14; hp={}, rules={}", hp(&g, TARGET), sentinels_v6::RULES_FINGERPRINT);
    assert_eq!(g.state.units[0].ammo, ammo_after_launch, "no extra shot or payment may explain the hit");
}

#[test]
fn stationary_target_still_takes_one_paid_catalogue_payload() {
    for kind in ["light-tank","algorithm-tank","laser-tank"] {
        let mut g=world(kind,10,None);
        let d=catalog::unit_ref(kind).unwrap();
        fire_once(&mut g);
        steps(&mut g,40);
        assert!((1000.-hp(&g,TARGET)-d.damage).abs()<1e-8,"{kind}");
        assert_eq!(g.state.units[0].attackCount,1);
        assert!((g.state.units[0].ammo-(1000.-d.ammo_per_shot)).abs()<1e-8);
        assert!((g.state.units[0].energy-(1000.-d.energy_per_attack)).abs()<1e-8);
    }
}

#[test]
fn lateral_target_can_evade_without_bending_the_direct_ray() {
    let mut g=world("light-tank",10,Some((0,1)));
    let p=fire_once(&mut g).remove(0);
    let aim=p.aimPosition.unwrap();
    for _ in 0..40 {
        g.step();
        for flying in &g.state.projectiles {
            assert_eq!(flying.aimPosition,Some(aim),"unguided aim must not track the moving target");
        }
    }
    assert_eq!(hp(&g,TARGET),1000.);
    let impact=g.state.events.iter().find(|e|e.kind=="projectile-impact"&&e.subject==p.id).unwrap();
    assert_eq!(impact.presentationPosition,Some(aim));
    assert!((distance(p.launchPosition.unwrap(),aim)-14.).abs()<1e-8);
}

#[test]
fn algorithm_lead_changes_initial_direction_without_changing_speed() {
    for movement in [(1,0),(0,1)] {
        let mut g=world("algorithm-tank",10,Some(movement));
        let p=fire_once(&mut g).remove(0);
        let launch=p.launchPosition.unwrap();let aim=p.aimPosition.unwrap();
        assert!((distance(launch,aim)-14.).abs()<1e-8);
        assert!((p.duration-14./28.).abs()<1e-12);
        let first=[p.x,p.y,p.z];
        move_fixture_actor(&mut g,TARGET,65.5,70.5);
        g.step();
        let next=g.state.projectiles.iter().find(|q|q.id==p.id).unwrap();
        assert_eq!(next.aimPosition,Some(aim));
        assert!((distance(first,[next.x,next.y,next.z])*60.-28.).abs()<1e-8);
        if movement.1!=0 { assert!(aim[1]>46.,"algorithm must retain its one-time lateral lead"); }
    }
}

#[test]
fn range_uses_the_swept_body_near_face_not_the_target_center() {
    for kind in ["light-tank","algorithm-tank"] {
        for (x,should_hit) in [(54.9,true),(55.0,false)] {
            let mut g=world(kind,13,None);
            let p=fire_once(&mut g).remove(0);
            // Both centers are beyond range14, but only the first AABB reaches inside it.
            move_fixture_actor(&mut g,TARGET,x,45.5);
            steps(&mut g,35);
            assert_eq!(hp(&g,TARGET)<1000.,should_hit,"{kind}: target x={x}");
            for e in g.state.events.iter().filter(|e|e.kind=="projectile-impact"&&e.subject==p.id) {
                assert!(distance(p.launchPosition.unwrap(),e.presentationPosition.unwrap())<=14.+1e-8);
            }
            assert!(!g.state.projectiles.iter().any(|q|q.id==p.id));
        }
    }
}

#[test]
fn physical_wall_still_intercepts_direct_and_unchanged_beam() {
    for kind in ["light-tank","algorithm-tank","laser-tank"] {
        let mut g=world(kind,10,None);wall(&mut g,45);
        let p=fire_once(&mut g).remove(0);steps(&mut g,40);
        assert_eq!(hp(&g,TARGET),1000.);
        assert!(g.state.walls[0].hp<1000.);
        assert!(!g.state.projectiles.iter().any(|q|q.id==p.id));
    }
}

#[test]
fn direct_penetration_keeps_collision_damage_and_remaining_payload() {
    let kind=branch_chassis("algorithm","rail-accelerator");
    let mut g=world(&kind,10,None);wall(&mut g,45);
    let p=fire_once(&mut g).remove(0);assert!(p.penetration>0.);
    steps(&mut g,60);
    assert!(g.state.walls[0].hp<1000.);
    assert!(hp(&g,TARGET)<1000.,"penetration must preserve its remaining target damage");
}

#[test]
fn breacher_explodes_at_range_after_missing_the_old_target_point() {
    let kind=branch_chassis("speed","breacher");
    let mut g=world(&kind,4,None);
    let p=fire_once(&mut g).remove(0);assert!((p.radius-1.4).abs()<1e-9);
    move_fixture_actor(&mut g,TARGET,60.5,60.5);
    g.state.units.push(actor(10004,2,"algorithm-scout",47,46));
    steps(&mut g,25);
    assert_eq!(hp(&g,TARGET),1000.);
    assert!(hp(&g,10004)<1000.,"range-end splash must still damage a nearby off-ray body");
    let impact=g.state.events.iter().find(|e|e.kind=="projectile-impact"&&e.subject==p.id).unwrap();
    assert_eq!(impact.presentationPosition,p.aimPosition);
    assert!((distance(p.launchPosition.unwrap(),impact.presentationPosition.unwrap())-7.).abs()<1e-8);
}

#[test]
fn breacher_collision_still_detonates_its_area_payload() {
    let kind=branch_chassis("speed","breacher");
    let mut g=world(&kind,6,None);wall(&mut g,44);
    // The intended wall takes the remaining payload; intervening thin cover
    // would correctly consume a non-splash penetration fragment instead.
    g.state.units[0].target=Some(10003);
    g.state.units.push(actor(10004,2,"algorithm-scout",43,46));
    let p=fire_once(&mut g).remove(0);steps(&mut g,25);
    assert!(g.state.walls[0].hp<1000.);
    assert!(hp(&g,10004)<1000.,"collision splash must remain active");
    let impact=g.state.events.iter().find(|e|e.kind=="projectile-impact"&&e.subject==p.id).unwrap();
    assert!(distance(p.launchPosition.unwrap(),impact.presentationPosition.unwrap())<7.);
}

#[test]
fn cone_direct_rounds_keep_their_radius_and_range_end_impacts() {
    for kind in ["pycharm","minimax"] {
        let mut g=world(kind,7,None);
        g.state.units.push(actor(10004,2,"algorithm-scout",47,47));
        g.refresh_fog();
        let shots=fire_once(&mut g);
        assert_eq!(shots.len(),2,"{kind}: the real cone attack must emit both payloads");
        g.state.units.retain(|u|u.id==SHOOTER);
        steps(&mut g,80);
        for p in shots {
            assert_eq!(p.kind,"direct");assert!(p.radius>0.);
            let impact=g.state.events.iter().find(|e|e.kind=="projectile-impact"&&e.subject==p.id).unwrap();
            assert_eq!(impact.magnitude,p.radius);
            assert_eq!(impact.presentationPosition,p.aimPosition);
            assert!((distance(p.launchPosition.unwrap(),impact.presentationPosition.unwrap())-catalog::unit_ref(kind).unwrap().range).abs()<1e-8);
        }
    }
}

#[test]
fn other_launched_trajectories_keep_their_original_duration_rules() {
    for (chassis,trajectory) in [("artillery","arc"),("aa-launcher","guided"),("orbital-strike","orbital"),("particle-cannon","beam")] {
        let kind=branch_chassis("algorithm",chassis);let mut g=world(&kind,10,None);
        if trajectory=="orbital" { orbital_prerequisites(&mut g); }
        let p=fire_once(&mut g).remove(0);assert_eq!(p.kind,trajectory);
        if trajectory=="orbital" { assert!(g.state.rooms.iter().any(|r|r.kind=="orbital-control"&&r.online)); }
        let travel=(distance(p.launchPosition.unwrap(),p.aimPosition.unwrap())/if trajectory=="arc"{12.}else{28.}).max(0.1);
        let expected=match trajectory{"beam"=>0.04,"orbital"=>4.,"guided"=>travel*2.+0.2,_=>travel};
        assert!((p.duration-expected).abs()<1e-12,"{trajectory}");
    }
}

#[test]
fn in_flight_direct_save_load_preserves_every_following_state() {
    let mut original=world("light-tank",10,Some((1,0)));
    fire_once(&mut original);steps(&mut original,5);
    assert_eq!(original.state.projectiles.len(),1);
    let bytes=serde_json::to_vec(&original.save()).unwrap();
    let mut restored=Game::load(serde_json::from_slice(&bytes).unwrap()).unwrap();
    for _ in 0..40 {
        original.step();restored.step();
        assert_eq!(serde_json::to_value(&original.state).unwrap(),serde_json::to_value(&restored.state).unwrap());
    }
    assert!(hp(&restored,TARGET)<1000.);
}

#[test]
fn prior_rules_save_is_rejected_by_the_new_fingerprint() {
    let mut save=world("light-tank",10,None).save();
    save.rulesFingerprint="0acffa83ef75bfeb39efeaf9a49b706c0b02446d4399dab14048b1e8398ab08a".into();
    assert!(Game::load(save).is_err());
}
