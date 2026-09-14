//! Per-instance PBR parameters. The immutable imported material is never modified.
use assetd::model::ModelMaterial;
use serde_json::Value;

fn scalar(value: &Value, name: &str, max: Option<f32>) -> Result<f32, String> {
    let v = value
        .as_f64()
        .ok_or_else(|| format!("MATERIAL_OVERRIDE_INVALID: {name} must be a number"))?;
    if !v.is_finite() || v < 0. || v > f32::MAX as f64 || max.is_some_and(|m| v > m as f64) {
        return Err(format!(
            "MATERIAL_OVERRIDE_INVALID: {name} is outside its valid range"
        ));
    }
    Ok(v as f32)
}

fn array<const N: usize>(v: &Value, name: &str, max: Option<f32>) -> Result<[f32; N], String> {
    let a = v
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("MATERIAL_OVERRIDE_INVALID: {name} must have {N} channels"))?;
    let mut out = [0.; N];
    for i in 0..N {
        out[i] = scalar(&a[i], name, max)?;
    }
    Ok(out)
}

pub fn apply(
    material: &ModelMaterial,
    overrides: &Value,
    slot: usize,
) -> Result<ModelMaterial, String> {
    let mut result = material.clone();
    if overrides.is_null() {
        return Ok(result);
    }
    let slots = overrides
        .as_object()
        .ok_or("MATERIAL_OVERRIDE_INVALID: materialOverrides must be a dictionary")?;
    let Some(value) = slots.get(&slot.to_string()) else {
        return Ok(result);
    };
    let params = value
        .as_object()
        .ok_or("MATERIAL_OVERRIDE_INVALID: material slot must contain PBR parameters")?;
    for (key, value) in params {
        match key.as_str() {
            "baseColor" => result.base_color = array(value, key, Some(1.))?,
            "metallic" => result.metallic = scalar(value, key, Some(1.))?,
            "roughness" => result.roughness = scalar(value, key, Some(1.))?,
            "emissive" => result.emissive = array(value, key, None)?,
            "normalScale" => result.normal_scale = scalar(value, key, None)?,
            "occlusionStrength" => result.occlusion_strength = scalar(value, key, Some(1.))?,
            "alphaCutoff" => result.alpha_cutoff = scalar(value, key, Some(1.))?,
            "doubleSided" => {
                result.double_sided = value
                    .as_bool()
                    .ok_or("MATERIAL_OVERRIDE_INVALID: doubleSided must be boolean")?
            }
            "alphaMode" => {
                let mode = value
                    .as_str()
                    .ok_or("MATERIAL_OVERRIDE_INVALID: alphaMode must be text")?
                    .to_uppercase();
                if !matches!(mode.as_str(), "OPAQUE" | "MASK" | "BLEND") {
                    return Err(
                        "MATERIAL_OVERRIDE_INVALID: alphaMode must be OPAQUE, MASK or BLEND".into(),
                    );
                }
                result.alpha_mode = mode;
            }
            _ => {
                return Err(format!(
                    "MATERIAL_OVERRIDE_INVALID: unsupported parameter {key}"
                ))
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn material() -> ModelMaterial {
        ModelMaterial {
            guid: "m".into(),
            name: "wood".into(),
            base_color: [1.; 4],
            metallic: 0.,
            roughness: 0.7,
            emissive: [0.; 3],
            base_color_texture: None,
            normal_texture: None,
            metallic_roughness_texture: None,
            occlusion_texture: None,
            emissive_texture: None,
            normal_scale: 1.,
            occlusion_strength: 1.,
            double_sided: false,
            alpha_mode: "OPAQUE".into(),
            alpha_cutoff: 0.5,
            unlit: false,
        }
    }
    #[test]
    fn overrides_are_per_slot_and_do_not_mutate_the_imported_asset() {
        let original = material();
        let values =
            json!({"2":{"baseColor":[0.8,0.2,0.1,0.5],"roughness":0.1,"alphaMode":"BLEND"}});
        let other = apply(&original, &values, 0).unwrap();
        assert_eq!(other.base_color, [1.; 4]);
        let changed = apply(&original, &values, 2).unwrap();
        assert_eq!(changed.base_color, [0.8, 0.2, 0.1, 0.5]);
        assert_eq!(changed.alpha_mode, "BLEND");
        assert_eq!(original.roughness, 0.7);
        assert_eq!(changed.base_color_texture, original.base_color_texture);
    }
    #[test]
    fn malformed_and_out_of_range_parameters_fail_instead_of_silently_changing_materials() {
        for params in [
            json!({"roughness":-0.1}),
            json!({"metallic":2}),
            json!({"baseColor":[1,0,0]}),
            json!({"alphaMode":"unknown"}),
            json!({"futureUnsupportedMap":0}),
        ] {
            assert!(apply(&material(), &json!({"0":params}), 0).is_err());
        }
        assert_eq!(
            apply(&material(), &json!({"0":{"emissive":[2,3,4]}}), 0)
                .unwrap()
                .emissive,
            [2., 3., 4.]
        );
    }
}
