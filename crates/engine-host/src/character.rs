//! Basic capsule controller: fixed-step swept movement against static map colliders.
use forge_scene::{Entity, Scene};
use rurix_physics::{
    BodyDesc, BodyId, BodyKind, MassProps, PhysicsTransform, PhysicsWorld, QueryShape, ShapeDesc,
    SyncBudget,
};
use serde_json::{json, Value};
use std::collections::HashMap;
#[derive(Default)]
pub struct Controllers {
    keys: HashMap<String, f64>,
    vertical: HashMap<u64, f32>,
}
fn num(v: &Value, k: &str, d: f32) -> f32 {
    v.get(k).and_then(Value::as_f64).unwrap_or(f64::from(d)) as f32
}
fn capsule(v: &Value) -> Result<(ShapeDesc, f32), String> {
    let r = num(v, "radius", 0.35);
    let h = num(v, "height", 1.8);
    if !r.is_finite() || !h.is_finite() || r <= 0. || h < 2. * r {
        return Err("CHARACTER_INVALID: height must be at least twice radius".into());
    }
    Ok((
        ShapeDesc::Capsule {
            radius: r,
            half_height: h * 0.5 - r,
        },
        h,
    ))
}
pub fn body_desc(scene: &Scene, e: &Entity) -> Result<Option<BodyDesc>, String> {
    let chara = e.component("CharacterController").filter(|c| c.enabled);
    let collider = e.component("Collider").filter(|c| c.enabled);
    if chara.is_none() && collider.is_none() {
        return Ok(None);
    }
    let mut transform = PhysicsTransform {
        translation: e.transform.translation,
        rotation: e.transform.rotation,
    };
    let (kind, shape, layer) = if let Some(c) = chara {
        let (s, h) = capsule(&c.props)?;
        transform.translation[1] += h * 0.5;
        (BodyKind::Kinematic, s, 1)
    } else {
        let c = collider.unwrap();
        let kind = match e
            .component("RigidBody")
            .and_then(|c| c.props["kind"].as_str())
            .unwrap_or("static")
        {
            "dynamic" => BodyKind::Dynamic,
            "kinematic" => BodyKind::Kinematic,
            _ => BodyKind::Static,
        };
        let shape = match c.props["shape"].as_str().unwrap_or("box") {
            "mesh" => {
                if kind != BodyKind::Static {
                    return Err("COLLIDER_INVALID: mesh collider must be static".into());
                }
                let model_ref = c.props["model"]
                    .as_str()
                    .filter(|r| !r.is_empty())
                    .or_else(|| {
                        e.component("ModelRenderer")
                            .and_then(|c| c.props["model"].as_str())
                    })
                    .ok_or("mesh collider model missing")?;
                let m = crate::modelrt::load(model_ref)?;
                let worlds = crate::modelrt::node_worlds(&m, "", 0., false)?;
                let entity = crate::modelrt::entity_world(scene, e)?;
                let mut positions = Vec::new();
                let mut triangles = Vec::new();
                let mut stack = m.roots.clone();
                let mut visited = std::collections::HashSet::new();
                let explicit = m
                    .nodes
                    .iter()
                    .any(|n| n.collision && !n.primitives.is_empty());
                while let Some(ni) = stack.pop() {
                    if !visited.insert(ni) {
                        continue;
                    }
                    let n = &m.nodes[ni];
                    stack.extend(&n.children);
                    if explicit && !n.collision {
                        continue;
                    }
                    for &pi in &n.primitives {
                        let p = &m.primitives[pi];
                        let base = positions.len() as u32;
                        let owner_root = e
                            .component("PrefabInstance")
                            .and_then(|c| c.props["rootId"].as_u64());
                        let proxy = scene.entities.iter().find(|candidate| {
                            candidate.component("ModelNode").is_some_and(|c| {
                                c.props["nodeId"].as_str() == Some(&n.id)
                                    && c.props["model"].as_str() == Some(model_ref)
                            }) && candidate
                                .component("PrefabInstance")
                                .and_then(|c| c.props["rootId"].as_u64())
                                == owner_root
                        });
                        let mat = if let Some(proxy) = proxy {
                            crate::modelrt::entity_world(scene, proxy)?
                        } else {
                            crate::viewport::m4_mul(entity, worlds[ni])
                        };
                        positions
                            .extend(p.positions.iter().map(|&v| crate::modelrt::point(mat, v)));
                        triangles.extend(
                            p.indices
                                .chunks_exact(3)
                                .map(|t| [base + t[0], base + t[1], base + t[2]]),
                        );
                    }
                }
                transform = PhysicsTransform::IDENTITY;
                ShapeDesc::StaticMesh {
                    vertices: positions,
                    triangles,
                }
            }
            "capsule" => capsule(&c.props)?.0,
            "box" => {
                let h: [f32; 3] = serde_json::from_value(
                    c.props
                        .get("halfExtents")
                        .cloned()
                        .unwrap_or(json!([0.5, 0.5, 0.5])),
                )
                .map_err(|e| e.to_string())?;
                ShapeDesc::Box {
                    half_extents: std::array::from_fn(|i| h[i] * e.transform.scale[i].abs()),
                }
            }
            _ => return Err("COLLIDER_INVALID: unsupported shape".into()),
        };
        (kind, shape, 0)
    };
    Ok(Some(BodyDesc {
        kind,
        shape,
        layer,
        mass_props: MassProps::default(),
        ccd: false,
        transform,
    }))
}
impl Controllers {
    pub fn clear(&mut self) {
        self.keys.clear();
        self.vertical.clear();
    }
    pub fn advance(
        &mut self,
        scene: &mut Scene,
        world: &mut PhysicsWorld,
        bodies: &HashMap<u64, BodyId>,
        inputs: &[(String, f64)],
        dt: f32,
    ) -> Result<(), String> {
        for (action, value) in inputs {
            if action == "input_reset" {
                self.keys.clear();
            } else {
                self.keys.insert(action.clone(), *value);
            }
        }
        let x = (self.keys.get("right").copied().unwrap_or(0.)
            + self.keys.get("left").copied().unwrap_or(0.)) as f32;
        let z = -(self.keys.get("up").copied().unwrap_or(0.)
            + self.keys.get("down").copied().unwrap_or(0.)) as f32;
        let len = (x * x + z * z).sqrt().max(1.);
        let gravity = scene.gravity[1];
        for e in &mut scene.entities {
            let Some(c) = e.component("CharacterController").filter(|c| c.enabled) else {
                continue;
            };
            let props = c.props.clone();
            let (shape, h) = capsule(&props)?;
            let controlled = props["controlled"].as_bool().unwrap_or(true);
            let speed = num(&props, "speed", 3.);
            if !speed.is_finite() || speed < 0. {
                return Err("CHARACTER_INVALID: speed must be finite and nonnegative".into());
            }
            let vy = self.vertical.entry(e.id).or_default();
            *vy += gravity * dt;
            let old = e.transform.translation;
            let mut center = [old[0], old[1] + h * 0.5, old[2]];
            let mut delta = [
                if controlled { x / len * speed * dt } else { 0. },
                *vy * dt,
                if controlled { z / len * speed * dt } else { 0. },
            ];
            let mut grounded = false;
            let mut budget = SyncBudget::new(0, 0, 32);
            for _ in 0..4 {
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if distance < 1e-7 {
                    break;
                }
                let dir = delta.map(|v| v / distance);
                let hits = world
                    .cast_shape(
                        &QueryShape {
                            shape: shape.clone(),
                            start: PhysicsTransform {
                                translation: center,
                                rotation: [0., 0., 0., 1.],
                            },
                            dir,
                            t_max: distance,
                            layer_mask: 1,
                        },
                        &mut budget,
                    )
                    .map_err(|e| e.to_string())?;
                let Some(hit) = hits.first() else {
                    for i in 0..3 {
                        center[i] += delta[i];
                    }
                    break;
                };
                let travel = (hit.t - 0.001).clamp(0., distance);
                for i in 0..3 {
                    center[i] += dir[i] * travel;
                    delta[i] -= dir[i] * travel;
                }
                let n = hit.normal;
                let dot = delta.iter().zip(n).map(|(a, b)| a * b).sum::<f32>();
                if dot < 0. {
                    for i in 0..3 {
                        delta[i] -= n[i] * dot;
                    }
                }
                if n[1] > 0.5 && *vy < 0. {
                    *vy = 0.;
                    grounded = true;
                }
                if travel <= 1e-7 && dot >= -1e-7 {
                    break;
                }
            }
            e.transform.translation = [center[0], center[1] - h * 0.5, center[2]];
            let dx = e.transform.translation[0] - old[0];
            let dz = e.transform.translation[2] - old[2];
            let moving = (dx * dx + dz * dz) > 1e-8;
            if moving {
                let yaw = dx.atan2(dz);
                e.transform.rotation = [0., (yaw * 0.5).sin(), 0., (yaw * 0.5).cos()];
            }
            if let Some(&body) = bodies.get(&e.id) {
                world
                    .set_kinematic_target(
                        body,
                        PhysicsTransform {
                            translation: center,
                            rotation: [0., 0., 0., 1.],
                        },
                    )
                    .map_err(|e| e.to_string())?;
            }
            if let Some(a) = e.component_mut("Animator").filter(|a|a.props["manualControl"].as_bool()!=Some(true)) {
                let key = if moving { "walkClip" } else { "idleClip" };
                let clip = a.props[key]
                    .as_str()
                    .unwrap_or(if moving { "walk" } else { "idle" })
                    .to_string();
                if a.props["clip"].as_str() != Some(&clip) {
                    a.props["clip"] = json!(clip);
                    a.props["time"] = json!(0.);
                }
                a.props["playing"] = json!(true);
            }
            if let Some(c) = e.component_mut("CharacterController") {
                c.props["grounded"] = json!(grounded);
                c.props["moving"] = json!(moving);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capsule_rejects_invalid_dimensions() {
        assert!(capsule(&json!({"radius":1,"height":1})).is_err());
    }
    #[test]
    fn swept_character_stops_at_wall_and_ground() {
        let mut world = PhysicsWorld::new(rurix_physics::WorldDesc::default()).unwrap();
        let base = |translation, shape| BodyDesc {
            kind: BodyKind::Static,
            shape,
            layer: 0,
            mass_props: MassProps::default(),
            ccd: false,
            transform: PhysicsTransform {
                translation,
                rotation: [0., 0., 0., 1.],
            },
        };
        world
            .add_bodies_batch(&[
                base(
                    [0., -0.5, 0.],
                    ShapeDesc::Box {
                        half_extents: [10., 0.5, 10.],
                    },
                ),
                base(
                    [2., 1., 0.],
                    ShapeDesc::Box {
                        half_extents: [0.1, 1., 10.],
                    },
                ),
            ])
            .unwrap();
        let mut scene = Scene::new("controller");
        scene.entities.push(Entity { entity_guid: None,
            id: 1,
            name: "hero".into(),
            transform: forge_scene::Transform::default(),
            components: vec![forge_scene::Component::new(
                "CharacterController",
                json!({"speed":3,"radius":0.35,"height":1.8}),
            )],
        });
        let mut c = Controllers::default();
        for _ in 0..120 {
            c.advance(
                &mut scene,
                &mut world,
                &HashMap::new(),
                &[("right".into(), 1.)],
                1. / 60.,
            )
            .unwrap();
            world.step(1. / 60.).unwrap();
        }
        let p = scene.entities[0].transform.translation;
        assert!(p[0] > 1. && p[0] < 1.6, "wall: {p:?}");
        assert!(p[1].abs() < 0.02, "ground: {p:?}");
    }
}
