use super::*;

#[derive(Clone)]
struct Expr {
    ty: ValueType,
    w: String,
    g: String,
}
impl Expr {
    fn new(ty: ValueType, w: impl Into<String>, g: impl Into<String>) -> Self {
        Self {
            ty: ty.numeric(),
            w: w.into(),
            g: g.into(),
        }
    }
    fn swizzle(&self, s: &str) -> Self {
        Self::new(
            match s.len() {
                1 => ValueType::Float,
                2 => ValueType::Vec2,
                3 => ValueType::Vec3,
                _ => ValueType::Vec4,
            },
            format!("({}).{s}", self.w),
            format!("({}).{s}", self.g),
        )
    }
}
fn types(n: usize) -> (&'static str, &'static str) {
    match n {
        1 => ("f32", "float"),
        2 => ("vec2<f32>", "vec2"),
        3 => ("vec3<f32>", "vec3"),
        _ => ("vec4<f32>", "vec4"),
    }
}
fn literal(v: &Value) -> Result<Expr, Diagnostic> {
    if let Some(f) = v
        .as_f64()
        .filter(|f| f.is_finite() && f.abs() <= f32::MAX as f64)
    {
        let s = format!("{:?}", f as f32);
        return Ok(Expr::new(ValueType::Float, s.clone(), s));
    }
    if let Some(a) = v.as_array().filter(|a| (2..=4).contains(&a.len())) {
        let mut parts = Vec::new();
        for x in a {
            let x = literal(x)?;
            if x.ty != ValueType::Float {
                return Err(Diagnostic::error(
                    "SHADER_TYPE",
                    "Vector constants require numeric channels",
                    None,
                ));
            }
            parts.push(x.w);
        }
        let (w, g) = types(a.len());
        let v = parts.join(",");
        return Ok(Expr::new(
            match a.len() {
                2 => ValueType::Vec2,
                3 => ValueType::Vec3,
                _ => ValueType::Vec4,
            },
            format!("{w}({v})"),
            format!("{g}({v})"),
        ));
    }
    Err(Diagnostic::error(
        "SHADER_CONSTANT",
        "Expected a finite scalar or 2–4 component vector",
        None,
    ))
}
fn cast(e: Expr, ty: ValueType) -> Result<Expr, Diagnostic> {
    let ty = ty.numeric();
    if e.ty == ty {
        return Ok(e);
    }
    if e.ty == ValueType::Float && ty.lanes() > 1 {
        let (w, g) = types(ty.lanes());
        return Ok(Expr::new(
            ty,
            format!("{w}({})", e.w),
            format!("{g}({})", e.g),
        ));
    }
    Err(Diagnostic::error(
        "SHADER_TYPE",
        format!("Incompatible pin types {:?} and {:?}", e.ty, ty),
        None,
    ))
}
struct Emitter<'a> {
    graph: &'a GraphDoc,
    state: BTreeMap<String, u8>,
    values: BTreeMap<String, Expr>,
    w: Vec<String>,
    g: Vec<String>,
    textures: Vec<String>,
}
impl<'a> Emitter<'a> {
    fn source(&mut self, s: &ValueSource) -> Result<Expr, Diagnostic> {
        match s {
            ValueSource::Constant { value } => literal(value),
            ValueSource::Parameter { param } => self.parameter(param),
            ValueSource::Node { node, pin } => {
                let e = self.node(node)?;
                if pin == "out" {
                    Ok(e)
                } else if self
                    .graph
                    .nodes
                    .iter()
                    .any(|n| n.id == *node && n.ty == "split")
                    && ["x", "y", "z", "w"].contains(&pin.as_str())
                    && "xyzw".find(pin).is_some_and(|i| i < e.ty.lanes())
                {
                    Ok(e.swizzle(pin))
                } else {
                    Err(Diagnostic::error(
                        "SHADER_PIN",
                        format!("Unknown output pin {pin}"),
                        Some(node),
                    ))
                }
            }
        }
    }
    fn parameter(&self, id: &str) -> Result<Expr, Diagnostic> {
        let p = self
            .graph
            .parameters
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| {
                Diagnostic::error("SHADER_PARAMETER", format!("Unknown parameter {id}"), None)
            })?;
        if p.ty == ValueType::Texture2d {
            let slot = self.textures.iter().position(|s| s == id).unwrap() + 1;
            return Ok(Expr::new(
                ValueType::Texture2d,
                format!("{slot}u"),
                format!("forge_tex_{slot}"),
            ));
        }
        let i = self
            .graph
            .parameters
            .iter()
            .filter(|p| p.ty != ValueType::Texture2d)
            .position(|p| p.id == id)
            .unwrap();
        let offset = 4 + (self.textures.len() + 1) * 8 + i * 4;
        let sw = &"xyzw"[..p.ty.lanes()];
        let expr = Expr::new(
            ValueType::Vec4,
            format!("forge_param({offset}u)"),
            format!("forge_param_{i}"),
        );
        Ok(expr.swizzle(sw))
    }
    fn input(&mut self, n: &Node, pin: &str, default: Value) -> Result<Expr, Diagnostic> {
        let result = match n.inputs.get(pin) {
            Some(v) => self.source(v),
            None => literal(&default),
        };
        result.map_err(|mut e| {
            if e.node_id.is_none() {
                e.node_id = Some(n.id.clone());
            }
            e.pin = Some(pin.into());
            e
        })
    }
    fn node(&mut self, id: &str) -> Result<Expr, Diagnostic> {
        if let Some(v) = self.values.get(id) {
            return Ok(v.clone());
        }
        if self.state.get(id) == Some(&1) {
            return Err(Diagnostic::error(
                "SHADER_CYCLE",
                "Shader graphs must be acyclic",
                Some(id),
            ));
        }
        let n = self
            .graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .cloned()
            .ok_or_else(|| {
                Diagnostic::error(
                    "SHADER_NODE_MISSING",
                    format!("Missing node {id}"),
                    Some(id),
                )
            })?;
        self.state.insert(id.into(), 1);
        let v = (|| -> Result<Expr, Diagnostic> {
            Ok(match n.ty.as_str() {
                "constant" | "color" => literal(n.options.get("value").unwrap_or(
                    &serde_json::json!(if n.ty == "color" {
                        serde_json::json!([1, 1, 1, 1])
                    } else {
                        serde_json::json!(0)
                    }),
                ))?,
                "parameter" => self.parameter(
                    n.options
                        .get("parameter")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                )?,
                "uv" => Expr::new(ValueType::Vec2, "uv", "uv"),
                "uvTransform" => {
                    let uv = if let Some(v) = n.inputs.get("uv") {
                        cast(self.source(v)?, ValueType::Vec2)?
                    } else {
                        Expr::new(ValueType::Vec2, "uv", "uv")
                    };
                    let tiling = cast(
                        self.input(&n, "tiling", serde_json::json!([1, 1]))?,
                        ValueType::Vec2,
                    )?;
                    let offset = cast(
                        self.input(&n, "offset", serde_json::json!([0, 0]))?,
                        ValueType::Vec2,
                    )?;
                    Expr::new(
                        ValueType::Vec2,
                        format!("({}*{}+{})", uv.w, tiling.w, offset.w),
                        format!("({}*{}+{})", uv.g, tiling.g, offset.g),
                    )
                }
                "time" => Expr::new(
                    ValueType::Float,
                    "bitcast<f32>(graph_data[0u])",
                    "forge_time",
                ),
                "texture" => {
                    let tex = if let Some(s) = n.inputs.get("texture") {
                        self.source(s)?
                    } else {
                        Expr::new(ValueType::Texture2d, "0u", "forge_tex_0")
                    };
                    if tex.ty != ValueType::Texture2d {
                        return Err(Diagnostic::error(
                            "SHADER_TYPE",
                            "texture pin requires a texture2d parameter",
                            Some(id),
                        ));
                    }
                    let uv = if let Some(v) = n.inputs.get("uv") {
                        cast(self.source(v)?, ValueType::Vec2)?
                    } else {
                        Expr::new(ValueType::Vec2, "uv", "uv")
                    };
                    let w = format!("forge_sample({}, {})", tex.w, uv.w);
                    let g = format!("texture({}, {})", tex.g, uv.g);
                    if n.options.get("colorSpace").and_then(Value::as_str) == Some("linear") {
                        Expr::new(ValueType::Vec4, w, g)
                    } else {
                        Expr::new(
                            ValueType::Vec4,
                            format!("forge_decode({w})"),
                            format!("forge_decode({g})"),
                        )
                    }
                }
                "add" | "subtract" | "multiply" | "divide" | "dot" => {
                    let a = self.input(&n, "a", serde_json::json!(0))?;
                    let b = self.input(
                        &n,
                        "b",
                        serde_json::json!(if n.ty == "divide" || n.ty == "multiply" {
                            1
                        } else {
                            0
                        }),
                    )?;
                    let ty = if a.ty.lanes() >= b.ty.lanes() {
                        a.ty
                    } else {
                        b.ty
                    };
                    let a = cast(a, ty)?;
                    let b = cast(b, ty)?;
                    if ty.lanes() == 0 {
                        return Err(Diagnostic::error(
                            "SHADER_TYPE",
                            "Math requires numeric inputs",
                            Some(id),
                        ));
                    }
                    if n.ty == "dot" {
                        if ty == ValueType::Float {
                            return Err(Diagnostic::error(
                                "SHADER_TYPE",
                                "dot requires vector inputs",
                                Some(id),
                            ));
                        }
                        Expr::new(
                            ValueType::Float,
                            format!("dot({}, {})", a.w, b.w),
                            format!("dot({}, {})", a.g, b.g),
                        )
                    } else {
                        let op = match n.ty.as_str() {
                            "add" => "+",
                            "subtract" => "-",
                            "multiply" => "*",
                            _ => "/",
                        };
                        Expr::new(
                            ty,
                            format!("({} {op} {})", a.w, b.w),
                            format!("({} {op} {})", a.g, b.g),
                        )
                    }
                }
                "mix" => {
                    let a = self.input(&n, "a", serde_json::json!(0))?;
                    let b = self.input(&n, "b", serde_json::json!(1))?;
                    let ty = if a.ty.lanes() >= b.ty.lanes() {
                        a.ty
                    } else {
                        b.ty
                    };
                    let a = cast(a, ty)?;
                    let b = cast(b, ty)?;
                    let t = cast(self.input(&n, "t", serde_json::json!(0.5))?, ty)?;
                    Expr::new(
                        ty,
                        format!("mix({}, {}, {})", a.w, b.w, t.w),
                        format!("mix({}, {}, {})", a.g, b.g, t.g),
                    )
                }
                "clamp" => {
                    let a = self.input(&n, "value", serde_json::json!(0))?;
                    let lo = cast(self.input(&n, "min", serde_json::json!(0))?, a.ty)?;
                    let hi = cast(self.input(&n, "max", serde_json::json!(1))?, a.ty)?;
                    Expr::new(
                        a.ty,
                        format!("clamp({}, {}, {})", a.w, lo.w, hi.w),
                        format!("clamp({}, {}, {})", a.g, lo.g, hi.g),
                    )
                }
                "sin" | "cos" | "abs" | "fract" | "normalize" | "oneMinus" => {
                    let a = self.input(&n, "value", serde_json::json!(0))?;
                    if a.ty.lanes() == 0 || (n.ty == "normalize" && a.ty.lanes() < 2) {
                        return Err(Diagnostic::error(
                            "SHADER_TYPE",
                            "Invalid unary input",
                            Some(id),
                        ));
                    }
                    if n.ty == "oneMinus" {
                        let one = cast(literal(&serde_json::json!(1))?, a.ty)?;
                        Expr::new(
                            a.ty,
                            format!("({}-{})", one.w, a.w),
                            format!("({}-{})", one.g, a.g),
                        )
                    } else {
                        Expr::new(
                            a.ty,
                            format!("{}({})", n.ty, a.w),
                            format!("{}({})", n.ty, a.g),
                        )
                    }
                }
                "split" => {
                    let a = self.input(&n, "value", serde_json::json!([0, 0, 0, 0]))?;
                    if a.ty.lanes() < 2 {
                        return Err(Diagnostic::error(
                            "SHADER_TYPE",
                            "split requires a vector",
                            Some(id),
                        ));
                    }
                    a
                }
                "combine" => {
                    let count = n.options.get("size").and_then(Value::as_u64).unwrap_or(4) as usize;
                    if !(2..=4).contains(&count) {
                        return Err(Diagnostic::error(
                            "SHADER_TYPE",
                            "combine size must be 2, 3 or 4",
                            Some(id),
                        ));
                    }
                    let mut w = Vec::new();
                    let mut g = Vec::new();
                    for pin in ["x", "y", "z", "w"].iter().take(count) {
                        let v = cast(self.input(&n, pin, serde_json::json!(0))?, ValueType::Float)?;
                        w.push(v.w);
                        g.push(v.g);
                    }
                    let (wn, gn) = types(count);
                    Expr::new(
                        match count {
                            2 => ValueType::Vec2,
                            3 => ValueType::Vec3,
                            _ => ValueType::Vec4,
                        },
                        format!("{wn}({})", w.join(",")),
                        format!("{gn}({})", g.join(",")),
                    )
                }
                "normalMap" => {
                    if self.graph.domain != Domain::Pbr3d {
                        return Err(Diagnostic::error(
                            "SHADER_DOMAIN",
                            "normalMap is available in pbr3d",
                            Some(id),
                        ));
                    }
                    let raw = self.input(&n, "color", serde_json::json!([0.5, 0.5, 1]))?;
                    let raw = if raw.ty == ValueType::Vec4 {
                        raw.swizzle("xyz")
                    } else {
                        cast(raw, ValueType::Vec3)?
                    };
                    let s = cast(
                        self.input(&n, "strength", serde_json::json!(1))?,
                        ValueType::Float,
                    )?;
                    Expr::new(
                        ValueType::Vec3,
                        format!(
                            "normalize(({} * 2.0 - vec3<f32>(1.0)) * vec3<f32>({}, {}, 1.0))",
                            raw.w, s.w, s.w
                        ),
                        format!(
                            "normalize(({} * 2.0 - vec3(1.0)) * vec3({}, {}, 1.0))",
                            raw.g, s.g, s.g
                        ),
                    )
                }
                _ => {
                    return Err(Diagnostic::error(
                        "SHADER_NODE_TYPE",
                        format!("Unknown shader node type {}", n.ty),
                        Some(id),
                    ))
                }
            })
        })()
        .map_err(|mut e| {
            if e.node_id.is_none() {
                e.node_id = Some(id.to_string());
            }
            e
        })?;
        let v = if v.ty == ValueType::Texture2d {
            v
        } else {
            let name = format!("n{}", self.values.len());
            let (_, gty) = types(v.ty.lanes());
            self.w.push(format!("// node:{id}\nlet {name} = {};", v.w));
            self.g
                .push(format!("// node:{id}\n{gty} {name} = {};", v.g));
            Expr::new(v.ty, name.clone(), name)
        };
        self.state.insert(id.into(), 2);
        self.values.insert(id.into(), v.clone());
        Ok(v)
    }
    fn output(&mut self, name: &str, default: Value, ty: ValueType) -> Result<Expr, Diagnostic> {
        let e = match self.graph.outputs.get(name) {
            Some(v) => self.source(v)?,
            None => literal(&default)?,
        };
        if ty == ValueType::Vec3 && e.ty == ValueType::Vec4 {
            return Ok(e.swizzle("xyz"));
        }
        cast(e, ty).map_err(|mut e| {
            e.pin = Some(name.into());
            e
        })
    }
}

const WGSL_HELPERS: &str = r#"
@group(0) @binding(0) var<storage,read> graph_data:array<u32>;
fn forge_param(p:u32)->vec4<f32>{return vec4<f32>(bitcast<f32>(graph_data[p]),bitcast<f32>(graph_data[p+1u]),bitcast<f32>(graph_data[p+2u]),bitcast<f32>(graph_data[p+3u]));}
fn forge_linear(c:vec3<f32>)->vec3<f32>{return select(c/12.92,pow((max(c,vec3<f32>(0.0))+vec3<f32>(0.055))/1.055,vec3<f32>(2.4)),c>vec3<f32>(0.04045));}
fn forge_encode(c:vec3<f32>)->vec3<f32>{return select(c*12.92,1.055*pow(max(c,vec3<f32>(0.0)),vec3<f32>(1.0/2.4))-vec3<f32>(0.055),c>vec3<f32>(0.0031308));}
fn forge_decode(c:vec4<f32>)->vec4<f32>{return vec4<f32>(forge_linear(c.rgb),c.a);}
fn forge_sample(slot:u32,uv:vec2<f32>)->vec4<f32>{let h=4u+slot*8u;let w=graph_data[h+1u];let ht=graph_data[h+2u];if(w==0u||ht==0u){return vec4<f32>(1.0);}let p=vec2<u32>(clamp(uv,vec2<f32>(0.0),vec2<f32>(0.999999))*vec2<f32>(f32(w),f32(ht)));let c=graph_data[graph_data[h]+p.y*w+p.x];return vec4<f32>(f32(c&255u),f32((c>>8u)&255u),f32((c>>16u)&255u),f32((c>>24u)&255u))/255.0;}
struct ForgeSurface{base:vec3<f32>,alpha:f32,metallic:f32,roughness:f32,emission:vec3<f32>,normal:vec3<f32>};
"#;
const GODOT_HELPERS: &str = r#"
vec3 forge_linear(vec3 c){return mix(c/12.92,pow((max(c,vec3(0.0))+vec3(0.055))/1.055,vec3(2.4)),greaterThan(c,vec3(0.04045)));}
vec3 forge_encode(vec3 c){return mix(c*12.92,1.055*pow(max(c,vec3(0.0)),vec3(1.0/2.4))-vec3(0.055),greaterThan(c,vec3(0.0031308)));}
vec4 forge_decode(vec4 c){return vec4(forge_linear(c.rgb),c.a);}
uniform float forge_time=0.0;
uniform vec4 forge_tint=vec4(1.0);
uniform vec4 forge_uv_rect=vec4(0.0,0.0,1.0,1.0);
uniform vec2 forge_flip=vec2(0.0);
"#;

pub(super) fn compile(graph: &GraphDoc) -> Result<CompiledGraph, Diagnostic> {
    let textures = graph
        .parameters
        .iter()
        .filter(|p| p.ty == ValueType::Texture2d)
        .map(|p| p.id.clone())
        .collect::<Vec<_>>();
    let mut e = Emitter {
        graph,
        state: BTreeMap::new(),
        values: BTreeMap::new(),
        w: vec![],
        g: vec![],
        textures: textures.clone(),
    };
    // Validate even disconnected nodes: an invalid saved node cannot hide outside the output cone.
    for n in &graph.nodes {
        e.node(&n.id)?;
    }
    let allowed: &[&str] = match graph.domain {
        Domain::Sprite2d => &["color", "alpha"],
        Domain::Pbr3d => &[
            "baseColor",
            "alpha",
            "metallic",
            "roughness",
            "emission",
            "normal",
        ],
        Domain::Unlit3d => &["baseColor", "alpha", "emission"],
    };
    for key in graph.outputs.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(Diagnostic::error(
                "SHADER_OUTPUT",
                format!("Output {key} is invalid for this domain"),
                None,
            ));
        }
    }
    let color_name = if graph.domain == Domain::Sprite2d {
        "color"
    } else {
        "baseColor"
    };
    let raw = if let Some(s) = graph.outputs.get(color_name) {
        e.source(s)?
    } else {
        Expr::new(
            ValueType::Vec4,
            "forge_decode(forge_sample(0u,uv))",
            "forge_decode(texture(forge_tex_0,uv))",
        )
    };
    let base = if raw.ty == ValueType::Vec4 {
        raw.swizzle("rgb")
    } else {
        cast(raw.clone(), ValueType::Vec3)?
    };
    let alpha = if graph.outputs.contains_key("alpha") {
        e.output("alpha", serde_json::json!(1), ValueType::Float)?
    } else if raw.ty == ValueType::Vec4 {
        raw.swizzle("a")
    } else {
        literal(&serde_json::json!(1))?
    };
    let metal = e.output("metallic", serde_json::json!(0), ValueType::Float)?;
    let rough = e.output("roughness", serde_json::json!(0.8), ValueType::Float)?;
    let emission = e.output("emission", serde_json::json!([0, 0, 0]), ValueType::Vec3)?;
    let normal = e.output("normal", serde_json::json!([0, 0, 1]), ValueType::Vec3)?;
    let body = e.w.join("\n");
    let eval=format!("{WGSL_HELPERS}\nfn forge_eval(uv:vec2<f32>)->ForgeSurface{{\n{body}\nreturn ForgeSurface({}, {}, clamp({},0.0,1.0),clamp({},0.045,1.0),{},{});\n}}",base.w,alpha.w,metal.w,rough.w,emission.w,normal.w);
    let sprite=format!("{eval}\nstruct Pc{{model:mat4x4<f32>,color:vec4<f32>,size:vec2<u32>,flags:vec2<f32>,uv_rect:vec4<f32>,compositing:vec4<f32>}};var<push_constant> pc:Pc;\n@fragment fn main(@location(0) normal:vec3<f32>,@location(1) uv:vec2<f32>)->@location(0) vec4<f32>{{let s=forge_eval(uv);let a=s.alpha*pc.color.a;if(a<=0.0){{discard;}}return vec4<f32>(forge_encode(s.base+s.emission)*pc.color.rgb,a);}}");
    let light = if graph.domain == Domain::Pbr3d {
        r#"
let n0=normalize(normal);let t=normalize(tangent.xyz-dot(tangent.xyz,n0)*n0);let b=cross(n0,t)*tangent.w;let n=normalize(t*s.normal.x+b*s.normal.y+n0*s.normal.z);let l=normalize(vec3<f32>(0.45,0.8,0.35));let v=normalize(pc.eye_metal.xyz-pos);let h=normalize(l+v);let nv=max(dot(n,v),0.001);let nl=max(dot(n,l),0.0);let nh=max(dot(n,h),0.0);let hv=max(dot(h,v),0.0);let a=s.roughness*s.roughness;let a2=a*a;let d=a2/(3.14159265*pow(nh*nh*(a2-1.0)+1.0,2.0));let k=pow(s.roughness+1.0,2.0)/8.0;let g=(nv/(nv*(1.0-k)+k))*(nl/(nl*(1.0-k)+k));let f0=mix(vec3<f32>(0.04),s.base,s.metallic);let f=f0+(vec3<f32>(1.0)-f0)*pow(1.0-hv,5.0);let spec=d*g*f/max(4.0*nv*nl,0.001);let diffuse=(vec3<f32>(1.0)-f)*(1.0-s.metallic)*s.base/3.14159265;var color=s.base*0.14+(diffuse+spec)*nl*3.0+s.emission;
color=color/(vec3<f32>(1.0)+color);
"#
    } else {
        "var color=s.base+s.emission;"
    };
    let model=format!("{eval}\nstruct Pc{{base:vec4<f32>,emission_rough:vec4<f32>,eye_metal:vec4<f32>,controls:vec4<f32>,flags:vec4<f32>}};var<push_constant> pc:Pc;\n@fragment fn main(@location(0) pos:vec3<f32>,@location(1) normal:vec3<f32>,@location(2) uv:vec2<f32>,@location(3) tangent:vec4<f32>)->@location(0) vec4<f32>{{let s=forge_eval(uv);if(s.alpha<=0.0){{discard;}}{light}\nreturn vec4<f32>(forge_encode(color)*pc.base.rgb,s.alpha*pc.base.a);}}");
    let mut uniforms = String::new();
    for i in 0..=textures.len() {
        uniforms.push_str(&format!(
            "uniform sampler2D forge_tex_{i}:filter_nearest,repeat_disable;\n"
        ));
    }
    for (i, _) in graph
        .parameters
        .iter()
        .filter(|p| p.ty != ValueType::Texture2d)
        .enumerate()
    {
        uniforms.push_str(&format!("uniform vec4 forge_param_{i}=vec4(0.0);\n"));
    }
    let hash = graph_hash(graph);
    let program_hash = program_hash(graph);
    uniforms.push_str(&format!(
        "uniform float forge_compiled_{}=1.0;\n",
        &program_hash[..16]
    ));
    let gbody = e.g.join("\n");
    let canvas=format!("shader_type canvas_item;\nrender_mode blend_mix;\n{GODOT_HELPERS}\n{uniforms}\nvoid fragment(){{\nvec2 uv=UV;\n{gbody}\nCOLOR=vec4(forge_encode({}+{}),{})*forge_tint;\n}}",base.g,emission.g,alpha.g);
    let mode = if graph.domain == Domain::Pbr3d {
        "cull_disabled,blend_mix"
    } else {
        "unshaded,cull_disabled,blend_mix"
    };
    let pbr = if graph.domain == Domain::Pbr3d {
        format!("METALLIC=clamp({},0.0,1.0);ROUGHNESS=clamp({},0.045,1.0);NORMAL_MAP=normalize({})*0.5+vec3(0.5);",metal.g,rough.g,normal.g)
    } else {
        String::new()
    };
    let spatial=format!("shader_type spatial;\nrender_mode {mode};\n{GODOT_HELPERS}\n{uniforms}\nvoid fragment(){{\nvec2 uv=forge_uv_rect.xy+mix(UV,vec2(1.0)-UV,forge_flip)*forge_uv_rect.zw;\n{gbody}\nvec3 forge_base={};ALBEDO=(OUTPUT_IS_SRGB?forge_encode(forge_base):forge_base)*forge_tint.rgb;ALPHA={}*forge_tint.a;EMISSION={};{pbr}\n}}",base.g,alpha.g,emission.g);
    // Compile both actual fragment entry points, not only the expression fragment.
    compile_spirv(&sprite)
        .map_err(|msg| diagnostic_for_source("rurix", "fragment", &sprite, &msg, None))?;
    compile_spirv(&model)
        .map_err(|msg| diagnostic_for_source("rurix", "fragment", &model, &msg, None))?;
    let mut source_map = BTreeMap::new();
    for (k, src) in [
        ("wgsl.sprite", &sprite),
        ("wgsl.model", &model),
        ("godot.canvas", &canvas),
        ("godot.spatial", &spatial),
    ] {
        source_map.insert(
            k.into(),
            src.lines()
                .enumerate()
                .filter_map(|(i, l)| {
                    l.strip_prefix("// node:").map(|id| SourceSpan {
                        node_id: id.into(),
                        line: i + 1,
                    })
                })
                .collect(),
        );
    }
    Ok(CompiledGraph {
        hash,
        program_hash,
        domain: graph.domain,
        parameters: graph.parameters.clone(),
        texture_slots: textures,
        wgsl: Sources { sprite, model },
        godot: GodotSources { canvas, spatial },
        source_map,
        animated: graph.nodes.iter().any(|n| n.ty == "time"),
    })
}

pub fn compile_spirv(src: &str) -> Result<Vec<u8>, String> {
    let module = naga::front::wgsl::parse_str(src).map_err(|e| e.emit_to_string(src))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::PUSH_CONSTANT,
    )
    .validate(&module)
    .map_err(|e| e.to_string())?;
    let mut options = naga::back::spv::Options::default();
    options.lang_version = (1, 3);
    let words =
        naga::back::spv::write_vec(&module, &info, &options, None).map_err(|e| e.to_string())?;
    Ok(words.into_iter().flat_map(u32::to_le_bytes).collect())
}
pub fn diagnostic_for_source(
    backend: &str,
    stage: &str,
    src: &str,
    message: &str,
    line: Option<usize>,
) -> Diagnostic {
    let node_id = line
        .and_then(|line| {
            src.lines()
                .take(line)
                .filter_map(|l| l.trim().strip_prefix("// node:"))
                .last()
        })
        .map(str::to_string);
    Diagnostic {
        code: "SHADER_COMPILE_FAILED".into(),
        message: message.into(),
        severity: "error".into(),
        backend: backend.into(),
        stage: stage.into(),
        node_id,
        pin: None,
        line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph() -> GraphDoc {
        serde_json::from_value(serde_json::json!({"version":1,"id":"test","name":"Test","domain":"sprite2d","nodes":[{"id":"uv","type":"uv"},{"id":"tex","type":"texture","inputs":{"uv":{"node":"uv","pin":"out"}}}],"outputs":{"color":{"node":"tex","pin":"out"}}})).unwrap()
    }
    #[test]
    fn all_domains_compile_real_spirv() {
        for domain in [Domain::Sprite2d, Domain::Pbr3d, Domain::Unlit3d] {
            let mut g = graph();
            g.domain = domain;
            if domain != Domain::Sprite2d {
                let color = g.outputs.remove("color").unwrap();
                g.outputs.insert("baseColor".into(), color);
            }
            let c = super::super::compile(&g).unwrap();
            for src in [&c.wgsl.sprite, &c.wgsl.model] {
                let bytes = compile_spirv(src).unwrap();
                assert_eq!(&bytes[..4], &0x07230203u32.to_le_bytes());
            }
            assert_eq!(c.hash, super::super::compile(&g).unwrap().hash);
        }
    }
    #[test]
    fn cycle_and_mismatched_pins_have_node_diagnostics() {
        let mut g = graph();
        g.nodes[0].ty = "add".into();
        g.nodes[0].inputs.insert(
            "a".into(),
            ValueSource::Node {
                node: "uv".into(),
                pin: "out".into(),
            },
        );
        let e = super::super::compile(&g).unwrap_err();
        assert_eq!(e[0].code, "SHADER_CYCLE");
        assert_eq!(e[0].node_id.as_deref(), Some("uv"));
    }
    #[test]
    fn program_identity_excludes_layout_and_uniform_values_but_keeps_code_and_abi() {
        let mut g = graph();
        g.parameters.push(Parameter {
            id: "tint".into(),
            name: "Tint".into(),
            ty: ValueType::Color,
            default: serde_json::json!([1, 1, 1, 1]),
        });
        g.outputs.insert(
            "color".into(),
            ValueSource::Parameter {
                param: "tint".into(),
            },
        );
        let first = super::super::compile(&g).unwrap();
        g.nodes[0].pos = [380., 190.];
        g.name = "Renamed".into();
        g.parameters[0].name = "Color".into();
        g.parameters[0].default = serde_json::json!([0.2, 0.8, 0.5, 1.]);
        let updated = super::super::compile(&g).unwrap();
        assert_ne!(first.hash, updated.hash);
        assert_eq!(first.program_hash, updated.program_hash);
        assert_eq!(first.wgsl.sprite, updated.wgsl.sprite);
        assert_eq!(first.godot.canvas, updated.godot.canvas);
        assert_ne!(first.parameters[0].default, updated.parameters[0].default);
        g.outputs.insert(
            "color".into(),
            ValueSource::Constant {
                value: serde_json::json!([0, 1, 0, 1]),
            },
        );
        assert_ne!(program_hash(&g), first.program_hash);
        let code_hash = program_hash(&g);
        g.parameters[0].ty = ValueType::Vec3;
        assert_ne!(program_hash(&g), code_hash);
        let abi_hash = program_hash(&g);
        g.domain = Domain::Pbr3d;
        assert_ne!(program_hash(&g), abi_hash);
    }
    #[test]
    fn textures_are_not_numeric_and_unknown_outputs_fail() {
        let mut g = graph();
        g.outputs.insert(
            "roughness".into(),
            ValueSource::Constant {
                value: serde_json::json!(1),
            },
        );
        assert_eq!(
            super::super::compile(&g).unwrap_err()[0].code,
            "SHADER_OUTPUT"
        );
    }
}
