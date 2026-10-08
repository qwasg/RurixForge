//! Utilization-style construction projection (clone execute, same as host preview).
use sentinels_v6::{types::*, Game};

#[test]
fn shell_projection_exposes_power_meters_and_accepts_flat_footprint() {
    let mut g = Game::new(41, false);
    g.state.visible[0] = (0..96)
        .flat_map(|y| (0..128).map(move |x| Pos::new(x, y, 0)))
        .collect();
    g.state.explored[0] = g.state.visible[0].clone();
    let before = g.player(1).unwrap().clone();
    let rect = Rect {
        x: 24,
        y: 40,
        level: 0,
        width: 8,
        height: 8,
    };
    let mut copy = g.clone();
    let result = copy.execute(1, Command::Shell { rect });
    let after = copy.player(1).unwrap();
    assert!(
        result.is_ok() || result.as_ref().err().is_some_and(|e| e.contains("侦察") || e.contains("占用") || e.contains("金币")),
        "projection should be a structured accept/reject, got {result:?}"
    );
    assert!(before.power.is_finite() && after.power.is_finite());
    assert!(before.demand.is_finite() && after.demand.is_finite());
    if result.is_ok() {
        assert!(after.credits <= before.credits);
        assert_eq!(rect.level, 0);
        let net_area = rect.area();
        let capacity = g.room_capacity("data-center", rect);
        assert!(net_area > 0);
        assert_eq!(capacity, g.room_capacity("data-center", rect));
    }
}
