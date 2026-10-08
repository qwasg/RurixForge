//! End-to-end graph compilation, offscreen rendering and hot publication through the host RPC.
//! Requires freshly built engine-host and godot-host binaries and a real GPU.
mod common;
mod g4util;
mod g5util;
use base64::Engine;
use common::{serial, Rpc};
use serde_json::{json, Value};

fn evidence_image(name: &str, width: u32, height: u32, pixels: &[u8]) {
    let directory = common::build_target().join("shader-acceptance");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("{name}.png")),
        g5util::png(width, height, pixels),
    )
    .unwrap();
}

fn graph(domain: &str) -> Value {
    let key = if domain == "sprite2d" {
        "color"
    } else {
        "baseColor"
    };
    json!({"version":1,"id":"gpu-test","name":"GPU fixture","domain":domain,"nodes":[{"id":"red","type":"color","options":{"value":[1,0.01,0.01,1]}}],"outputs":{key:{"node":"red","pin":"out"}}})
}
fn exercise(r: &mut Rpc, backend: &str) {
    let before = r.call("scene.summary", json!({}));
    for domain in ["sprite2d", "pbr3d", "unlit3d"] {
        for shape in ["sphere", "plane"] {
            let preview = r.call(
                "shader.preview",
                json!({"graph":graph(domain),"shape":shape,"width":96,"height":96}),
            );
            assert_eq!(
                preview["ok"], true,
                "{backend}/{domain}/{shape}: validation={} diagnostics={}",
                preview["validation"], preview["diagnostics"]
            );
            assert_eq!(preview["validation"][backend], "pipeline-validated");
            let pixels = base64::engine::general_purpose::STANDARD
                .decode(preview["pixelsB64"].as_str().unwrap())
                .unwrap();
            assert_eq!(pixels.len(), 96 * 96 * 4);
            let center = g4util::mean_box(&pixels, 96, 48, 48, 3);
            assert!(
                center[0] > center[1] + 20.,
                "{backend}/{domain}/{shape} has no red material: {center:?}"
            );
        }
    }
    let mut bad = graph("sprite2d");
    bad["nodes"][0]["type"] = json!("unknown");
    let failure = r.raw("shader.preview", json!({"graph":bad}));
    assert!(failure.get("error").is_some());
    assert_eq!(
        r.call("scene.summary", json!({})),
        before,
        "preview changed editor scene"
    );
}
#[test]
fn actual_shader_graphs_render_in_rurix_and_godot() {
    let _guard = serial();
    let root = g4util::temp_project("shader-graph", None);
    {
        let host = g4util::RurixAt::start(&root);
        exercise(&mut Rpc::connect(host.port), "rurix");
    }
    for (method, driver) in [("forward_plus", "vulkan"), ("gl_compatibility", "opengl3")] {
        let host = g4util::godot_at(method, driver, &root, &[]);
        exercise(&mut host.rpc(), "godot");
        assert!(
            !host
                .log()
                .iter()
                .any(|l| l.contains("Rust function panicked")),
            "{:?}",
            host.log()
        );
    }
}

fn write_graph_material(
    root: &std::path::Path,
    domain: &str,
    tex: &str,
) -> (Value, String, String) {
    let id = format!("graph-live-{domain}");
    let material = format!("material-live-{domain}");
    let color = if domain == "sprite2d" {
        "color"
    } else {
        "baseColor"
    };
    let mut g = json!({"version":1,"id":id,"name":"Live texture/UV/normal","domain":domain,"parameters":[{"id":"map","name":"Map","type":"texture2d","default":tex},{"id":"tint","name":"Tint","type":"color","default":[1,1,1,1]}],"nodes":[{"id":"uv","type":"uvTransform"},{"id":"tex","type":"texture","inputs":{"texture":{"param":"map"},"uv":{"node":"uv","pin":"out"}}},{"id":"tinted","type":"multiply","inputs":{"a":{"node":"tex","pin":"out"},"b":{"param":"tint"}}}],"outputs":{color:{"node":"tinted","pin":"out"}}});
    if domain == "pbr3d" {
        g["nodes"].as_array_mut().unwrap().push(
            json!({"id":"normal","type":"normalMap","inputs":{"color":{"const":[0.5,0.5,1]}}}),
        );
        g["outputs"]["normal"] = json!({"node":"normal","pin":"out"});
    }
    let content = root.join("Content");
    std::fs::write(content.join(format!("{id}.rxshadergraph")), g.to_string()).unwrap();
    std::fs::write(
        content.join(format!("{id}.rxshadergraph.meta")),
        format!("guid: {id}\ntype: shadergraph\nimporter: shader-graph\n"),
    )
    .unwrap();
    std::fs::write(
        content.join(format!("{material}.rxmat")),
        json!({"version":2,"shaderGraph":id,"params":{},"textures":{}}).to_string(),
    )
    .unwrap();
    std::fs::write(
        content.join(format!("{material}.rxmat.meta")),
        format!("guid: {material}\ntype: material\nimporter: material\n"),
    )
    .unwrap();
    (g, id, material)
}

/// Import a genuine GLB with UVs, tangents and a non-zero material slot. This
/// catches bindings accidentally keyed by primitive index rather than material index.
fn imported_glb(root: &std::path::Path) -> String {
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    for (values, components, kind) in [
        (
            vec![
                -0.5f32, -0.5, 0., 0.5, -0.5, 0., 0.5, 0.5, 0., -0.5, 0.5, 0.,
            ],
            3,
            "VEC3",
        ),
        (
            vec![0., 0., 1., 0., 0., 1., 0., 0., 1., 0., 0., 1.],
            3,
            "VEC3",
        ),
        (vec![0., 1., 1., 1., 1., 0., 0., 0.], 2, "VEC2"),
        (
            vec![
                1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1., 1., 0., 0., 1.,
            ],
            4,
            "VEC4",
        ),
    ] {
        let index = views.len();
        views.push(json!({"buffer":0,"byteOffset":bin.len(),"byteLength":values.len()*4}));
        let mut accessor = json!({"bufferView":index,"componentType":5126,"count":values.len()/components,"type":kind});
        if index == 0 {
            accessor["min"] = json!([-0.5, -0.5, 0]);
            accessor["max"] = json!([0.5, 0.5, 0]);
        }
        accessors.push(accessor);
        for v in values {
            bin.extend_from_slice(&v.to_le_bytes());
        }
    }
    views.push(json!({"buffer":0,"byteOffset":bin.len(),"byteLength":12}));
    accessors.push(json!({"bufferView":4,"componentType":5123,"count":6,"type":"SCALAR"}));
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let doc = json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2,"TANGENT":3},"indices":4,"material":1}]}],"materials":[{"name":"unused"},{"name":"slot-one"}],"buffers":[{"byteLength":bin.len()}],"bufferViews":views,"accessors":accessors});
    let mut json = serde_json::to_vec(&doc).unwrap();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut glb = Vec::new();
    for word in [
        0x46546c67u32,
        2,
        (28 + json.len() + bin.len()) as u32,
        json.len() as u32,
        0x4e4f534a,
    ] {
        glb.extend_from_slice(&word.to_le_bytes());
    }
    glb.extend(json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e4942u32.to_le_bytes());
    glb.extend(bin);
    let source = root.join("shader-slot.glb");
    std::fs::write(&source, glb).unwrap();
    let manifest:assetd::model::ModelManifest=serde_json::from_value(json!({"version":1,"sourceId":"shader-slot-source","name":"Shader slot","kind":"prop","revision":1})).unwrap();
    let project = assetd::project::ForgeProject::with_defaults(root.to_owned());
    let imported = assetd::model::import_model_bundle(&project, &source, &manifest).unwrap();
    let model = assetd::model::load_model(&project, &imported.guid).unwrap();
    assert_eq!(model.primitives[0].material, Some(1));
    assert_eq!(model.primitives[0].uv0[1], [1., 1.]);
    imported.guid
}
fn hot_bindings(
    r: &mut Rpc,
    root: &std::path::Path,
    backend: &str,
    source: &str,
    map: &str,
    model: &str,
    child: &std::process::Child,
) {
    let mut last_entity = Value::Null;
    for mode in ["2d", "3d", "model"] {
        let domain = if mode == "model" { "pbr3d" } else { "sprite2d" };
        let (mut graph, guid, material) = write_graph_material(root, domain, map);
        r.call("shader.publish", json!({"reference":guid}));
        r.call(
            "scene.new",
            json!({"name":"Shader live","mode":if mode=="2d"{"2d"}else{"3d"}}),
        );
        r.call(
            "viewport.setCamera",
            json!({"target":[0,0,0],"yaw":0,"pitch":0,"dist":6,"ortho":true,"orthoSize":1.25}),
        );
        let component = if mode == "model" {
            json!({"type":"ModelRenderer","props":{"model":model,"materialBindings":{"1":{"material":material,"params":{}}}}})
        } else {
            json!({"type":"Sprite","props":{"texture":source,"pixelsPerUnit":4,"chromaKey":"none","blendMode":"alpha","material":material,"materialParams":{}}})
        };
        let entity = r.call("entity.create",json!({"name":"Bound entity","scale":if mode=="model"{[2,2,2]}else{[1,1,1]},"components":[component]}))["id"].clone();
        last_entity = entity.clone();
        let before = r.frame(96, 96).1;
        let (left, right) = (
            g4util::at(&before, 96, 32, 48),
            g4util::at(&before, 96, 64, 48),
        );
        assert!(
            left[0] > left[1] + 20 && right[1] > right[0] + 20,
            "{backend}/{mode} texture/UV binding left={left:?} right={right:?}"
        );
        evidence_image(&format!("{backend}-{mode}-original"), 96, 96, &before);
        let original_builds = r.call("shader.status", json!({}))["backendBuilds"][backend].clone();
        let parameter_physics = r.call("scene.summary", json!({}))["physics"].clone();
        let mut props = component["props"].clone();
        if mode == "model" {
            props["materialBindings"]["1"]["params"] = json!({"tint":[0.15,0.15,0.15,1]});
        } else {
            props["materialParams"] = json!({"tint":[0.15,0.15,0.15,1]});
        }
        r.call(
            "component.set",
            json!({"id":entity,"type":component["type"],"props":props}),
        );
        let dimmed = r.frame(96, 96).1;
        assert!(
            g4util::at(&dimmed, 96, 32, 48)[0] + 30 < left[0],
            "{backend}/{mode} material parameter did not update pixels"
        );
        assert_eq!(
            r.call("shader.status", json!({}))["backendBuilds"][backend],
            original_builds,
            "{backend}/{mode} material parameter rebuilt executable shader"
        );
        r.call(
            "component.set",
            json!({"id":entity,"type":component["type"],"props":component["props"]}),
        );
        assert_eq!(
            r.frame(96, 96).1,
            before,
            "{backend}/{mode} parameter undo did not restore pixels"
        );
        let path = root.join("Content").join(format!("{guid}.rxshadergraph"));
        graph["parameters"][1]["default"] = json!([0.15, 0.15, 0.15, 1]);
        std::fs::write(&path, graph.to_string()).unwrap();
        assert_eq!(
            r.call("shader.publish", json!({"reference":guid}))["ok"],
            true
        );
        assert_eq!(
            r.frame(96, 96).1,
            dimmed,
            "{backend}/{mode} default uniform value stayed stale"
        );
        assert_eq!(
            r.call("shader.status", json!({}))["backendBuilds"][backend],
            original_builds,
            "{backend}/{mode} parameter default rebuilt executable shader"
        );
        graph["parameters"][1]["default"] = json!([1, 1, 1, 1]);
        graph["nodes"][0]["pos"] = json!([380, 190]);
        graph["name"] = json!("Layout and name changed");
        std::fs::write(&path, graph.to_string()).unwrap();
        r.call("shader.publish", json!({"reference":guid}));
        assert_eq!(r.frame(96, 96).1, before);
        let after_builds = r.call("shader.status", json!({}))["backendBuilds"][backend].clone();
        assert_eq!(
            after_builds, original_builds,
            "{backend}/{mode} layout rebuilt executable shader"
        );
        assert_eq!(
            r.call("scene.summary", json!({}))["physics"],
            parameter_physics
        );
        evidence_image(&format!("{backend}-{mode}-uniform-update"), 96, 96, &dimmed);
        let evidence = json!({"backend":backend,"mode":mode,"passed":true,"backendBuildsBefore":original_builds,"backendBuildsAfter":after_builds,"checks":["material parameter override changes pixels","parameter undo restores pixels","graph default updates uniform","layout and names preserve executable","no backend recompilation","physics unchanged"]});
        std::fs::write(
            common::build_target()
                .join("shader-acceptance")
                .join(format!("{backend}-{mode}-uniform-reuse.json")),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
        // Reimport changes both bytes and dimensions while preserving the texture GUID
        // and graph hash. Publishing must rebuild only material GPU inputs.
        let project = assetd::project::ForgeProject::with_defaults(root.to_owned());
        let imported_map = g5util::write_texture(root, "map", 4, 1, |x, _| {
            if x < 2 {
                [0, 0, 255, 255]
            } else {
                [255, 255, 0, 255]
            }
        });
        assert_eq!(imported_map, map);
        let map_source = root
            .join("Content/Textures/map.png")
            .to_string_lossy()
            .into_owned();
        let imported =
            assetd::import::import_assets(&project, &[map_source.clone()], "Textures", None)
                .unwrap();
        assert!(imported.failed.is_empty());
        assert_eq!(imported.imported[0].guid, map);
        let physics = r.call("scene.summary", json!({}))["physics"].clone();
        assert_eq!(
            r.call("shader.publish", json!({"reference":guid}))["ok"],
            true
        );
        let republished = r.frame(96, 96).1;
        let blue = g4util::at(&republished, 96, 32, 48);
        let yellow = g4util::at(&republished, 96, 64, 48);
        assert!(
            blue[2] > blue[0] + 20
                && blue[2] > blue[1] + 20
                && yellow[0] > yellow[2] + 20
                && yellow[1] > yellow[2] + 20,
            "{backend}/{mode} same-GUID texture reimport remained stale: {blue:?}/{yellow:?}"
        );
        evidence_image(
            &format!("{backend}-{mode}-texture-reimport"),
            96,
            96,
            &republished,
        );
        assert_eq!(r.call("scene.summary", json!({}))["physics"], physics);
        g5util::write_texture(root, "map", 2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        });
        assert!(
            assetd::import::import_assets(&project, &[map_source], "Textures", None)
                .unwrap()
                .failed
                .is_empty()
        );
        r.call("shader.publish", json!({"reference":guid}));
        assert_eq!(
            r.frame(96, 96).1,
            before,
            "restored texture contents did not rebind"
        );
        graph["nodes"][0]["inputs"] = json!({"tiling":{"const":[-1,1]},"offset":{"const":[1,0]}});
        std::fs::write(&path, graph.to_string()).unwrap();
        // Saving a draft alone must not mutate the running material.
        assert_eq!(r.frame(96, 96).1, before, "draft unexpectedly published");
        assert_eq!(
            r.call("shader.publish", json!({"reference":guid}))["ok"],
            true
        );
        let after = r.frame(96, 96).1;
        evidence_image(&format!("{backend}-{mode}-uv-flipped"), 96, 96, &after);
        let left = g4util::at(&after, 96, 32, 48);
        assert!(
            left[1] > left[0] + 20,
            "{backend}/{mode} hot UV flip did not update pixels: {left:?}"
        );
        graph["nodes"][0]["type"] = json!("not-a-node");
        std::fs::write(&path, graph.to_string()).unwrap();
        assert_eq!(
            r.call("shader.publish", json!({"reference":guid}))["ok"],
            false
        );
        assert_eq!(
            r.frame(96, 96).1,
            after,
            "invalid graph replaced last valid material"
        );
    }
    let timed = json!({"version":1,"id":"clock","name":"Time","domain":"unlit3d","nodes":[{"id":"time","type":"time"},{"id":"repeat","type":"fract","inputs":{"value":{"node":"time","pin":"out"}}}],"outputs":{"baseColor":{"node":"repeat","pin":"out"}}});
    let a = r.call(
        "shader.preview",
        json!({"graph":timed,"shape":"plane","time":0.1,"width":64,"height":64}),
    );
    let b = r.call(
        "shader.preview",
        json!({"graph":timed,"shape":"plane","time":0.8,"width":64,"height":64}),
    );
    assert_eq!(a["ok"], true);
    assert_eq!(b["ok"], true);
    assert_ne!(
        a["pixelsB64"], b["pixelsB64"],
        "{backend} Time uniform did not change actual pixels"
    );
    let mut normal = graph("pbr3d");
    normal["outputs"]["baseColor"] = json!({"const":[0.8,0.8,0.8,1]});
    normal["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"normal","type":"normalMap","inputs":{"color":{"const":[0.5,0.5,1]}}}));
    normal["outputs"]["normal"] = json!({"node":"normal","pin":"out"});
    let flat = r.call(
        "shader.preview",
        json!({"graph":normal,"shape":"plane","width":64,"height":64}),
    );
    normal["nodes"][1]["inputs"]["color"] = json!({"const":[1,0.5,0.5]});
    let bent = r.call(
        "shader.preview",
        json!({"graph":normal,"shape":"plane","width":64,"height":64}),
    );
    assert_eq!(flat["ok"], true);
    assert_eq!(bent["ok"], true);
    let decode = |v: &Value| {
        base64::engine::general_purpose::STANDARD
            .decode(v["pixelsB64"].as_str().unwrap())
            .unwrap()
    };
    let a = g4util::mean_box(&decode(&flat), 64, 32, 32, 3);
    let b = g4util::mean_box(&decode(&bent), 64, 32, 32, 3);
    evidence_image(&format!("{backend}-normal-flat"), 64, 64, &decode(&flat));
    evidence_image(&format!("{backend}-normal-tilted"), 64, 64, &decode(&bent));
    assert!(
        (a[0] - b[0]).abs() > 8.,
        "{backend} normalMap did not alter lighting: {a:?} / {b:?}"
    );
    println!("{backend} normalMap flat={a:?}, tangent-tilted={b:?}");
    publication_lifecycle(r, root, backend, map, child, model, last_entity);
}

#[cfg(windows)]
fn private_bytes(child: &std::process::Child) -> usize {
    type Handle = *mut std::ffi::c_void;
    #[repr(C)]
    struct ProcessEntry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        name: [u16; 260],
    }
    #[repr(C)]
    struct Counters {
        size: u32,
        faults: u32,
        values: [usize; 9],
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
        fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    // Godot's console executable is a small launcher; account for its actual
    // rendering descendant too, otherwise an apparent 1 MiB plateau is meaningless.
    let snapshot = unsafe { CreateToolhelp32Snapshot(2, 0) };
    assert_ne!(snapshot as isize, -1);
    let mut entry: ProcessEntry = unsafe { std::mem::zeroed() };
    entry.size = std::mem::size_of::<ProcessEntry>() as u32;
    let mut processes = Vec::new();
    let mut present = unsafe { Process32FirstW(snapshot, &mut entry) };
    while present != 0 {
        processes.push((entry.pid, entry.parent));
        present = unsafe { Process32NextW(snapshot, &mut entry) };
    }
    unsafe { CloseHandle(snapshot) };
    let mut ids = std::collections::BTreeSet::from([child.id()]);
    loop {
        let count = ids.len();
        for &(pid, parent) in &processes {
            if ids.contains(&parent) {
                ids.insert(pid);
            }
        }
        if ids.len() == count {
            break;
        }
    }
    let mut total = 0;
    let mut observed = Vec::new();
    for pid in ids {
        let handle = unsafe { OpenProcess(0x410, 0, pid) };
        if handle.is_null() {
            continue;
        }
        let mut counters = Counters {
            size: std::mem::size_of::<Counters>() as u32,
            faults: 0,
            values: [0; 9],
        };
        let size = counters.size;
        let ok = unsafe { K32GetProcessMemoryInfo(handle, &mut counters, size) };
        unsafe { CloseHandle(handle) };
        assert_ne!(ok, 0);
        total += counters.values[8];
        observed.push((pid, counters.values[8] / (1024 * 1024)));
    }
    println!("host process tree private MiB (PID,MiB)={observed:?}");
    total
}
#[cfg(not(windows))]
fn private_bytes(_: &std::process::Child) -> usize {
    0
}

fn publication_lifecycle(
    r: &mut Rpc,
    root: &std::path::Path,
    backend: &str,
    map: &str,
    child: &std::process::Child,
    model: &str,
    entity: Value,
) {
    let (original, guid, material) = write_graph_material(root, "pbr3d", map);
    let path = root.join("Content").join(format!("{guid}.rxshadergraph"));
    let body=r.call("entity.create",json!({"name":"Physics sentinel","translation":[10,10,0],"components":[{"type":"RigidBody","props":{"kind":"dynamic","mass":1}}]}))["id"].clone();
    r.call("play.enter", json!({}));
    r.call("play.pause", json!({}));
    for _ in 0..12 {
        r.call("play.step", json!({}));
    }
    let physics = r.call("scene.summary", json!({}))["physics"].clone();
    let initial_transform = r.call("transform.get", json!({"id":body}));
    let pose = |v: Value| json!({"translation":v["translation"],"rotation":v["rotation"],"scale":v["scale"]});
    let transform = pose(initial_transform.clone());
    r.call("shader.publish", json!({"reference":guid}));
    let before = r.frame(64, 64).1;
    let builds_before = r.call("shader.status", json!({}))["backendBuilds"][backend].clone();
    for tint in [json!([0.1, 0.1, 0.1, 1]), json!([1, 1, 1, 1])] {
        r.call("component.set",json!({"id":entity,"type":"ModelRenderer","props":{"model":model,"materialBindings":{"1":{"material":material,"params":{"tint":tint}}}}}));
        let frame = r.frame(64, 64).1;
        if tint[0] == 1 {
            assert_eq!(frame, before);
        } else {
            assert_ne!(frame, before);
        }
        assert_eq!(
            r.call("shader.status", json!({}))["backendBuilds"][backend],
            builds_before,
            "{backend} paused slot parameter rebuilt GPU program"
        );
        assert_eq!(
            r.call("scene.summary", json!({}))["physics"],
            physics,
            "{backend} paused slot parameter reset physics"
        );
        let current = r.call("transform.get", json!({"id":body}));
        assert!(
            current["contentRevision"].as_u64().unwrap()
                > initial_transform["contentRevision"].as_u64().unwrap()
        );
        assert_eq!(pose(current), transform);
    }
    let builds_after = r.call("shader.status", json!({}))["backendBuilds"][backend].clone();
    let mut samples = Vec::new();
    for cycle in 0..24 {
        let mut candidate = original.clone();
        candidate["outputs"]["baseColor"] = json!({"const":[0.1+(cycle as f32)/32.,0.2,0.7,1.]});
        for document in [&candidate, &original] {
            std::fs::write(&path, document.to_string()).unwrap();
            assert_eq!(
                r.call("shader.publish", json!({"reference":guid}))["ok"],
                true
            );
            r.frame(64, 64);
            let status = r.call("shader.status", json!({}));
            let programs = status["programs"].as_u64().unwrap();
            assert!(
                status["livePrograms"].as_u64().unwrap() <= programs + 2,
                "{backend} leaked program ownership: {status}"
            );
            assert_eq!(
                status["previousPrograms"], 0,
                "{backend} retained candidate predecessor"
            );
            assert!(status["validationEntries"].as_u64().unwrap() <= programs);
            assert!(status["materials"].as_u64().unwrap() <= 1);
        }
        if cycle % 8 == 7 {
            samples.push(private_bytes(child));
        }
    }
    // Driver/compiler caches may warm up. Over the final 16 new GPU programs +
    // undo draws, require less than 64 MiB additional private memory, alongside
    // exact bounded Arc/program/material ownership above (no monotonic retention).
    assert!(
        samples[2].saturating_sub(samples[1]) < 64 * 1024 * 1024,
        "{backend} private memory did not settle: {samples:?}"
    );
    println!(
        "{backend} after 16/32/48 publications private MiB={:?}",
        samples
            .iter()
            .map(|n| n / (1024 * 1024))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        r.call("scene.summary", json!({}))["physics"],
        physics,
        "publication reset physics steps"
    );
    assert_eq!(
        pose(r.call("transform.get", json!({"id":body}))),
        transform,
        "publication reset simulated body"
    );
    assert_eq!(r.call("play.state", json!({}))["state"], "play_paused");
    r.call("play.step", json!({}));
    assert_ne!(
        pose(r.call("transform.get", json!({"id":body}))),
        transform,
        "physics no longer advances after hot publication"
    );
    r.call("play.exit", json!({}));
    let evidence = json!({"backend":backend,"passed":true,"publicationCount":48,"newGraphSourceCount":24,"privateMemoryBytes":samples,"sampleAtPublication":[16,32,48],"maximumAllowedFinalGrowthBytes":64*1024*1024,"pausedParameterProgramBuildsBefore":builds_before,"pausedParameterProgramBuildsAfter":builds_after,"checks":["real GLB slot 1","2D and 3D sprite","material and default uniforms change pixels without backend recompilation","paused slot uniform changes and undo preserve physics steps and body transform","same-GUID texture reimport byte/dimension changes","UV transformation","normalMap lighting","Time uniform","invalid graph fallback","draft does not publish","bounded live program ownership","physics state preserved"],"screenshotsDirectory":common::build_target().join("shader-acceptance")});
    std::fs::write(
        common::build_target()
            .join("shader-acceptance")
            .join(format!("{backend}-runtime.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
}
#[test]
fn live_sprite_spatial_sprite_and_model_slots_use_graph_textures_and_hot_publication() {
    let _guard = serial();
    let root = g4util::temp_project("shader-live", None);
    let source = g5util::write_texture(&root, "source", 8, 8, |_, _| [255; 4]);
    let map = g5util::write_texture(&root, "map", 2, 1, |x, _| {
        if x == 0 {
            [255, 0, 0, 255]
        } else {
            [0, 255, 0, 255]
        }
    });
    let model = imported_glb(&root);
    if std::env::var("FORGE_SHADER_TEST_BACKEND").as_deref() != Ok("godot") {
        let host = g4util::RurixAt::start(&root);
        hot_bindings(
            &mut Rpc::connect(host.port),
            &root,
            "rurix",
            &source,
            &map,
            &model,
            &host.child,
        );
    }
    if std::env::var("FORGE_SHADER_TEST_BACKEND").as_deref() != Ok("rurix") {
        let host = g4util::godot_at("forward_plus", "vulkan", &root, &[]);
        hot_bindings(
            &mut host.rpc(),
            &root,
            "godot",
            &source,
            &map,
            &model,
            &host.child,
        );
    }
}
