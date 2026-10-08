//! On-demand immutable viewport observations. Pixels and picks share one scene/camera snapshot.
use super::*;
use crate::render::snapshot::{FrameRequester, RenderSnapshot, SnapshotParams};
use crate::render_core::math::M4;
type V3 = [f32; 3];
const MAX_OBSERVATIONS: usize = 4;
const TTL: std::time::Duration = std::time::Duration::from_secs(300);
const MAX_TRIANGLES: usize = 250_000;

pub(super) struct Observation {
    id: String,
    created: Instant,
    snapshot: RenderSnapshot,
    identity: Value,
    inverse_view: M4,
    shapes: Vec<Shape>,
    candidates: Vec<Value>,
}
enum Shape {
    Triangles { id: u64, triangles: Vec<[V3; 3]> },
    Box { id: u64, transform: Transform },
}
impl Shape {
    fn id(&self) -> u64 {
        match self {
            Self::Triangles { id, .. } | Self::Box { id, .. } => *id,
        }
    }
    fn points(&self) -> Vec<V3> {
        match self {
            Self::Triangles { triangles, .. } => {
                triangles.iter().flat_map(|t| t.iter().copied()).collect()
            }
            Self::Box { transform, .. } => {
                let matrix = crate::render_core::math::trs_model(transform);
                let mut out = Vec::new();
                for x in [-0.5, 0.5] {
                    for y in [-0.5, 0.5] {
                        for z in [-0.5, 0.5] {
                            out.push(crate::modelrt::point(matrix, [x, y, z]));
                        }
                    }
                }
                out
            }
        }
    }
    fn hit(&self, origin: V3, dir: V3) -> Option<f32> {
        match self {
            Self::Box { transform, .. } => {
                crate::render_core::pick::ray_unit_cube(origin, dir, transform)
            }
            Self::Triangles { triangles, .. } => triangles
                .iter()
                .filter_map(|t| ray_triangle(origin, dir, t))
                .min_by(f32::total_cmp),
        }
    }
}
fn ray_triangle(origin: V3, dir: V3, t: &[V3; 3]) -> Option<f32> {
    let sub = |a: V3, b: V3| std::array::from_fn(|i| a[i] - b[i]);
    let dot = |a: V3, b: V3| a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
    let cross = crate::render_core::math::v3_cross;
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let h = cross(dir, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-8 {
        return None;
    }
    let s = sub(origin, t[0]);
    let u = dot(s, h) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, e1);
    let v = dot(dir, q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = dot(e2, q) / det;
    (distance >= 0.0).then_some(distance)
}
fn homogeneous(m: M4, p: V3) -> [f32; 4] {
    std::array::from_fn(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
}
fn unproject(m: M4, p: V3) -> Result<V3, (i64, String)> {
    let p = homogeneous(m, p);
    if p[3].abs() < 1e-9 {
        return domain_err("OBSERVATION_INVALID_CAMERA: projection is singular");
    }
    Ok([p[0] / p[3], p[1] / p[3], p[2] / p[3]])
}
fn frozen_shapes(scene: &Scene, eye: V3) -> Result<Vec<Shape>, (i64, String)> {
    let mut shapes = Vec::new();
    let mut count = 0;
    if scene
        .entities
        .iter()
        .any(|e| e.component("ModelRenderer").is_some_and(|c| c.enabled))
    {
        for draw in crate::render_core::model::collect(scene, eye, None).map_err(|e| (-32000, e))? {
            count += draw.vertices.len() / 144;
            if count > MAX_TRIANGLES {
                return domain_err("OBSERVATION_TOO_LARGE: geometry exceeds frozen picking budget");
            }
            let triangles = draw
                .vertices
                .chunks_exact(144)
                .map(|triangle| {
                    std::array::from_fn(|vi| {
                        std::array::from_fn(|i| {
                            f32::from_le_bytes(
                                triangle[vi * 48 + i * 4..vi * 48 + i * 4 + 4]
                                    .try_into()
                                    .unwrap(),
                            )
                        })
                    })
                })
                .collect();
            shapes.push(Shape::Triangles {
                id: draw.owner,
                triangles,
            });
        }
    } else {
        for entity in &scene.entities {
            if crate::render_core::sprite::is_renderable(entity) {
                shapes.push(Shape::Box {
                    id: entity.id,
                    transform: crate::render_core::sprite::sprite_render_transform(entity)
                        .unwrap_or(entity.transform),
                });
            }
        }
    }
    Ok(shapes)
}
fn bounds(shapes: &[Shape], snapshot: &RenderSnapshot, view: M4) -> Vec<Value> {
    let mut grouped: HashMap<u64, [f32; 4]> = HashMap::new();
    for shape in shapes {
        let mut b = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        let mut clipped = false;
        for point in shape.points() {
            let p = homogeneous(view, point);
            if p[3] <= 1e-6 || p[2] < 0.0 {
                clipped = true;
                continue;
            }
            let x = (p[0] / p[3] + 1.0) * 0.5;
            let y = (1.0 - p[1] / p[3]) * 0.5;
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x);
            b[3] = b[3].max(y);
        }
        if !b[0].is_finite() {
            continue;
        }
        // Near-plane crossings conservatively cover the frame; these are candidates, not visible-pixel IDs.
        if clipped {
            b = [0.0, 0.0, 1.0, 1.0];
        }
        if b[2] < 0.0 || b[3] < 0.0 || b[0] > 1.0 || b[1] > 1.0 {
            continue;
        }
        b = b.map(|v| v.clamp(0.0, 1.0));
        grouped
            .entry(shape.id())
            .and_modify(|old| {
                old[0] = old[0].min(b[0]);
                old[1] = old[1].min(b[1]);
                old[2] = old[2].max(b[2]);
                old[3] = old[3].max(b[3]);
            })
            .or_insert(b);
    }
    let mut ids: Vec<_> = grouped.into_iter().collect();
    ids.sort_by_key(|(id, _)| *id);
    ids.into_iter().filter_map(|(id,b)|snapshot.scene.entity(id).map(|e|json!({"id":id,"entityGuid":e.entity_guid,"name":e.name,"bounds":{"x":b[0],"y":b[1],"width":b[2]-b[0],"height":b[3]-b[1]},"visibility":"candidate"}))).collect()
}

pub(super) fn capture(state: &Mutex<HostState>, params: &Value) -> HResult {
    let backend = crate::render::backend();
    let (snapshot, identity) = {
        let st = lock(state);
        editor::check_expected(&st, params, false)?;
        if st.game_mode {
            return domain_err(
                "OBSERVATION_UNSUPPORTED: editor capture is unavailable in game-only mode",
            );
        }
        let (w, h) = viewport_size(params)?;
        let (_, scene_camera) = frame_options(params)?;
        let seq = match backend.path() {
            crate::render::FramePath::Pipelined(p) => p.next_seq(),
            _ => 0,
        };
        let snapshot = crate::render::snapshot(
            &st,
            SnapshotParams {
                width: w,
                height: h,
                selected: None,
                want_readback: true,
                want_stats: false,
                requester: FrameRequester::ViewportFrame,
                scene_camera,
            },
            seq,
        );
        if scene_camera && snapshot.vp_override.is_none() {
            return domain_err("OBSERVATION_INVALID_CAMERA: no enabled scene camera");
        }
        let mut identity = json!({});
        editor::stamp(&st, &mut identity);
        (snapshot, identity)
    };
    if crate::render_core::list::classify(&snapshot.scene)
        == crate::render_core::list::Leg::SentinelsV6
    {
        return domain_err(
            "OBSERVATION_UNSUPPORTED: specialized batch scene has no entity picking map",
        );
    }
    let view = snapshot.vp_override.unwrap_or_else(|| {
        snapshot
            .camera
            .view_proj(snapshot.params.width as f32 / snapshot.params.height as f32)
    });
    let inverse = crate::modelrt::inverse(view).map_err(|e| (-32000, e))?;
    let shapes = frozen_shapes(&snapshot.scene, snapshot.camera.eye())?;
    let (frame_width, frame_height, pixels_b64) = match backend.path() {
        crate::render::FramePath::Pipelined(p) => {
            let (out, _) =
                pipelined_wait(backend, p, crate::render::bus::Channel::Preview, &snapshot)?;
            (out.pixels.width, out.pixels.height, out.pixels.pixels_b64())
        }
        crate::render::FramePath::Immediate(_) => {
            let frame = crate::render::render_frame(snapshot.input()).map_err(|e| (-32000, e))?;
            (frame.width, frame.height, frame.pixels_b64())
        }
    };
    if frame_width != snapshot.params.width || frame_height != snapshot.params.height {
        return domain_err(
            "OBSERVATION_SIZE_CHANGED: rendered size differs from frozen picking frame",
        );
    }
    if crate::render_core::assets::ASSET_GENERATION.load(std::sync::atomic::Ordering::Relaxed)
        != snapshot.asset_generation
    {
        return domain_err(
            "OBSERVATION_ASSET_CHANGED: assets changed while capturing; capture again",
        );
    }
    let candidates = bounds(&shapes, &snapshot, view);
    let id = assetd::new_guid();
    let mut result = identity.clone();
    result["observationId"] = json!(id);
    result["expiresInSeconds"] = json!(TTL.as_secs());
    result["width"] = json!(frame_width);
    result["height"] = json!(frame_height);
    result["format"] = json!("rgba8");
    result["pixelsB64"] = json!(pixels_b64);
    result["candidates"] = json!(candidates);
    result["coordinateSpace"] = json!("normalized");
    result["pickingSemantics"] =
        json!("geometry; region candidates may be occluded; alpha transparency is not tested");
    result["camera"] = json!({"viewProjection":view,"source":if snapshot.vp_override.is_some(){"scene"}else{"editor"}});
    let mut st = lock(state);
    st.observations.retain(|o| o.created.elapsed() < TTL);
    while st.observations.len() >= MAX_OBSERVATIONS {
        st.observations.pop_front();
    }
    st.observations.push_back(Observation {
        id,
        created: Instant::now(),
        snapshot,
        identity,
        inverse_view: inverse,
        shapes,
        candidates,
    });
    Ok(result)
}

fn unit(value: Option<&Value>, name: &str) -> Result<f32, (i64, String)> {
    let n = value
        .and_then(Value::as_f64)
        .ok_or((-32602, format!("INVALID_REGION: {name} required")))?;
    if !n.is_finite() || !(0.0..=1.0).contains(&n) {
        return param_err(format!("INVALID_REGION: {name} must be normalized 0..1"));
    }
    Ok(n as f32)
}
pub(super) fn resolve(st: &mut HostState, params: &Value) -> HResult {
    let id = params
        .get("observationId")
        .and_then(Value::as_str)
        .ok_or((-32602, "observationId required".into()))?;
    st.observations.retain(|o| o.created.elapsed() < TTL);
    let observation = st.observations.iter().find(|o| o.id == id).ok_or((
        -32000,
        "OBSERVATION_EXPIRED: capture again; never resolve against a different frame".into(),
    ))?;
    let mut result = observation.identity.clone();
    result["observationId"] = json!(id);
    if let Some(point) = params.get("point") {
        let x = unit(point.get("x"), "point.x")?;
        let y = unit(point.get("y"), "point.y")?;
        let origin = unproject(
            observation.inverse_view,
            [2.0 * x - 1.0, 1.0 - 2.0 * y, 0.0],
        )?;
        let far = unproject(
            observation.inverse_view,
            [2.0 * x - 1.0, 1.0 - 2.0 * y, 1.0],
        )?;
        let dir = crate::modelrt::norm(std::array::from_fn(|i| far[i] - origin[i]));
        let hit = observation
            .shapes
            .iter()
            .filter_map(|s| s.hit(origin, dir).map(|t| (s.id(), t)))
            .min_by(|a, b| a.1.total_cmp(&b.1));
        result["hit"]=json!(hit.and_then(|(id,distance)|observation.snapshot.scene.entity(id).map(|e|json!({"id":id,"entityGuid":e.entity_guid,"name":e.name,"point":std::array::from_fn::<_,3,_>(|i|origin[i]+distance*dir[i])}))));
    }
    let selected = if let Some(region) = params.get("region") {
        let x = unit(region.get("x"), "region.x")?;
        let y = unit(region.get("y"), "region.y")?;
        let w = unit(region.get("width"), "region.width")?;
        let h = unit(region.get("height"), "region.height")?;
        if x + w > 1.00001 || y + h > 1.00001 {
            return param_err("INVALID_REGION: rectangle exceeds frame");
        }
        observation
            .candidates
            .iter()
            .filter(|e| {
                let b = &e["bounds"];
                let bx = b["x"].as_f64().unwrap() as f32;
                let by = b["y"].as_f64().unwrap() as f32;
                let bw = b["width"].as_f64().unwrap() as f32;
                let bh = b["height"].as_f64().unwrap() as f32;
                bx <= x + w && by <= y + h && bx + bw >= x && by + bh >= y
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        observation.candidates.clone()
    };
    result["candidates"] = json!(selected);
    result["pickingSemantics"] = json!("geometry candidates, not a visible-pixel ID buffer");
    result["staleForEditing"] = json!(
        observation.identity["hostEpoch"] != json!(st.host_epoch)
            || observation.identity["sceneGuid"] != json!(st.active().scene_guid)
            || observation.identity["contentRevision"] != json!(editor::revision(st))
            || observation.identity["targetMode"] != json!(editor::mode(st))
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> HostState {
        let mut state = HostState::new();
        entity_create(&mut state, &json!({"name":"target","components":[{"type":"MeshRenderer","props":{"mesh":"builtin:cube","material":""}}]})).unwrap();
        state.camera = crate::viewport::EditorCamera {
            target: [0.0; 3],
            yaw_deg: 0.0,
            pitch_deg: 0.0,
            ortho: true,
            dist: 9.0,
            ortho_half_h: 2.0,
            ..Default::default()
        };
        let snapshot = crate::render::snapshot(
            &state,
            SnapshotParams {
                width: 100,
                height: 100,
                selected: None,
                want_readback: true,
                want_stats: false,
                requester: FrameRequester::ViewportFrame,
                scene_camera: false,
            },
            1,
        );
        let view = snapshot.camera.view_proj(1.0);
        let shapes = frozen_shapes(&snapshot.scene, snapshot.camera.eye()).unwrap();
        let candidates = bounds(&shapes, &snapshot, view);
        let mut identity = json!({});
        editor::stamp(&state, &mut identity);
        state.observations.push_back(Observation {
            id: "frozen".into(),
            created: Instant::now(),
            snapshot,
            identity,
            inverse_view: crate::modelrt::inverse(view).unwrap(),
            shapes,
            candidates,
        });
        state
    }
    #[test]
    fn observation_point_and_region_keep_original_frame_after_scene_changes() {
        let mut state = fixture();
        let original = state.scene.entities[0].entity_guid.clone();
        state.scene.entities.clear();
        state.content_revision += 1;
        state.camera.target = [100.0; 3];
        let result=resolve(&mut state,&json!({"observationId":"frozen","point":{"x":0.5,"y":0.5},"region":{"x":0.3,"y":0.3,"width":0.4,"height":0.4}})).unwrap();
        assert_eq!(result["hit"]["entityGuid"], json!(original));
        assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(result["staleForEditing"], true);
        assert_eq!(result["contentRevision"], 0);
    }
    #[test]
    fn observation_never_falls_back_to_live_state_for_expired_id_or_bad_coordinates() {
        let mut state = fixture();
        assert!(resolve(
            &mut state,
            &json!({"observationId":"frozen","point":{"x":2.0,"y":0.5}})
        )
        .unwrap_err()
        .1
        .starts_with("INVALID_REGION"));
        state.observations[0].created = Instant::now() - TTL - std::time::Duration::from_secs(1);
        assert!(resolve(&mut state, &json!({"observationId":"frozen"}))
            .unwrap_err()
            .1
            .starts_with("OBSERVATION_EXPIRED"));
    }
}
