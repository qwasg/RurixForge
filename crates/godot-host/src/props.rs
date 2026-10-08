//! Stage 5 组件 props 的访问器。engine-host 的 `render_core::env::Props` 经 RenderList / LightItem 的公开字段可达、
//! 但没有导出类型名(lib.rs 不在 Stage 5 的改动范围里),这里把它包成按字段名取值的 `Fields`,
//! 其余代码只和 `Fields` 打交道;`fields!(&props)` 在调用点构造。

type Getter<'a, T> = Box<dyn Fn(&str) -> T + 'a>;

/// 按字段名取值(字段已按注册表缺省补齐、类型合法,见 engine-host render_core/env.rs)。
pub struct Fields<'a> {
    pub num: Getter<'a, f32>,
    pub flag: Getter<'a, bool>,
    pub text: Getter<'a, &'a str>,
    pub rgba: Getter<'a, [f32; 4]>,
    pub vec3: Getter<'a, [f32; 3]>,
}

impl Fields<'_> {
    pub fn num(&self, k: &str) -> f32 {
        (self.num)(k)
    }
    pub fn flag(&self, k: &str) -> bool {
        (self.flag)(k)
    }
    pub fn text(&self, k: &str) -> &str {
        (self.text)(k)
    }
    pub fn rgba(&self, k: &str) -> [f32; 4] {
        (self.rgba)(k)
    }
    pub fn vec3(&self, k: &str) -> [f32; 3] {
        (self.vec3)(k)
    }
    /// 枚举字段 → 在 `values` 里的下标(注册表的枚举顺序与 Godot 枚举一致);不认识的值取 `fallback`。
    pub fn index(&self, k: &str, values: &[&str], fallback: i64) -> i64 {
        let t = self.text(k);
        values.iter().position(|v| *v == t).map_or(fallback, |i| i as i64)
    }
}

/// `fields!(p)`,p = `&Props`(任意有 num / flag / text / rgba / vec3 方法的引用)。
macro_rules! fields {
    ($p:expr) => {{
        let p = $p;
        $crate::props::Fields {
            num: Box::new(move |k| p.num(k)),
            flag: Box::new(move |k| p.flag(k)),
            text: Box::new(move |k| p.text(k)),
            rgba: Box::new(move |k| p.rgba(k)),
            vec3: Box::new(move |k| p.vec3(k)),
        }
    }};
}
pub(crate) use fields;
