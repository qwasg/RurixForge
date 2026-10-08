//! Observe telemetry already returned by Rurix; no new device queries or fences.
#[cfg(feature = "backend-rurix")]
use rurix_rt::render_exec::DeviceFrameTelemetry;
use serde_json::{json, Value};
#[cfg_attr(not(feature = "backend-rurix"), allow(unused_imports))]
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Default)]
struct Layer {
    host_provenance_ns: VecDeque<f64>,
    host_execute_call_ns: VecDeque<f64>,
    cpu_record_ns: VecDeque<f64>,
    cpu_submit_ns: VecDeque<f64>,
    cpu_fence_wait_ns: VecDeque<f64>,
    passes: BTreeMap<(u64, String), VecDeque<f64>>,
    timestamp_period_ns: Option<f32>,
    spikes: Vec<(f64, Value)>,
}
static DATA: OnceLock<Mutex<BTreeMap<i32, Layer>>> = OnceLock::new();
#[cfg_attr(not(feature = "backend-rurix"), allow(dead_code))]
fn push(rows: &mut VecDeque<f64>, value: f64) {
    if rows.len() >= 8192 {
        rows.pop_front();
    }
    rows.push_back(value);
}
pub fn clear() {
    if let Some(data) = DATA.get() {
        data.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
#[cfg(feature = "backend-rurix")]
pub fn record(layer: i32, provenance_ns: f64, execute_ns: f64, telemetry: &DeviceFrameTelemetry) {
    let mut data = DATA
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let row = data.entry(layer).or_default();
    push(&mut row.host_provenance_ns, provenance_ns);
    push(&mut row.host_execute_call_ns, execute_ns);
    push(&mut row.cpu_record_ns, telemetry.cpu_record_ns as f64);
    push(&mut row.cpu_submit_ns, telemetry.cpu_submit_ns as f64);
    push(
        &mut row.cpu_fence_wait_ns,
        telemetry.cpu_fence_wait_ns as f64,
    );
    row.timestamp_period_ns = Some(telemetry.timestamp_period_ns);
    for pass in &telemetry.passes {
        push(
            row.passes
                .entry((pass.pass_id, pass.name.clone()))
                .or_default(),
            pass.gpu_ns,
        );
    }
    if row.spikes.len() < 16
        || row
            .spikes
            .last()
            .is_some_and(|(duration, _)| execute_ns > *duration)
    {
        let sample = json!({"observed_at_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d|d.as_millis() as u64),"host_provenance_ns":provenance_ns,"host_execute_call_ns":execute_ns,"timestamp_period_ns":telemetry.timestamp_period_ns,"cpu_record_ns":telemetry.cpu_record_ns,"cpu_submit_ns":telemetry.cpu_submit_ns,"cpu_fence_wait_ns":telemetry.cpu_fence_wait_ns,"passes":telemetry.passes.iter().map(|p|json!({"pass_id":p.pass_id,"name":p.name,"gpu_ns":p.gpu_ns})).collect::<Vec<_>>()});
        row.spikes.push((execute_ns, sample));
        row.spikes.sort_by(|a, b| b.0.total_cmp(&a.0));
        row.spikes.truncate(16);
    }
}
fn describe(row: &Layer) -> Value {
    let q = crate::sentinels_v6::metric;
    json!({"units":"nanoseconds, except timestamp_period_ns (ns per GPU tick) and observed_at_unix_ms","scope":"Existing Rurix telemetry only. cpu_fence_wait_ns starts before slot reuse and ends after completion wait; it includes recording/submission and is not exclusive sleep time. GPU pass start/end timestamps are not exported by this backend API, so overlap is unknown and no sum is claimed as total GPU frame latency. Separate CPU readback duration is unavailable; execute call includes queries/readback/return processing.","spike_selection":"Top16 host_execute_call_ns per layer since reset; distributions use the latest8192 recorded frames, not only spikes.","timestamp_period_ns":row.timestamp_period_ns,"gpu_pass_overlap":"unknown","cpu_readback_ns":null,"cpu_readback_observed":false,"host_provenance_ns":q(&row.host_provenance_ns),"host_execute_call_ns":q(&row.host_execute_call_ns),"cpu_record_ns":q(&row.cpu_record_ns),"cpu_submit_ns":q(&row.cpu_submit_ns),"cpu_fence_wait_ns":q(&row.cpu_fence_wait_ns),"passes":row.passes.iter().map(|((id,name),rows)|json!({"pass_id":id,"name":name,"gpu_ns":q(rows)})).collect::<Vec<_>>(),"top_execute_call_spikes":row.spikes.iter().map(|(_,sample)|sample).collect::<Vec<_>>()})
}
pub fn metrics() -> Value {
    let data = DATA
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    json!(data
        .iter()
        .map(|(layer, row)| (*layer, describe(row)))
        .collect::<BTreeMap<_, _>>())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_units_and_unknown_gpu_overlap_are_preserved() {
        let mut layer = Layer::default();
        push(&mut layer.cpu_record_ns, 500.);
        push(&mut layer.cpu_submit_ns, 100.);
        push(&mut layer.cpu_fence_wait_ns, 1500.);
        push(layer.passes.entry((1, "opaque".into())).or_default(), 900.);
        push(layer.passes.entry((2, "effects".into())).or_default(), 800.);
        let value = describe(&layer);
        assert_eq!(value["cpu_fence_wait_ns"]["p50"], 1500.);
        assert_eq!(value["cpu_record_ns"]["p50"], 500.);
        assert_eq!(value["passes"][0]["gpu_ns"]["p50"], 900.);
        assert_eq!(value["gpu_pass_overlap"], "unknown");
        assert!(value.get("gpu_total_ns").is_none());
        assert!(value["cpu_readback_ns"].is_null());
        assert_eq!(value["cpu_readback_observed"], false);
    }
}
