//! Thread-local, opt-in performance observation. Never serialized or used by game rules.
use serde_json::{json, Value};
use std::{cell::RefCell, collections::BTreeMap, time::Instant};
#[derive(Default)]
struct State {
    enabled: bool,
    last: Option<Instant>,
    tick: u64,
    current: Vec<(&'static str, f64)>,
    samples: BTreeMap<&'static str, Vec<f64>>,
    subsamples: BTreeMap<&'static str, Vec<f64>>,
    current_substages: BTreeMap<&'static str, f64>,
    spikes: Vec<(
        u64,
        f64,
        Vec<(&'static str, f64)>,
        BTreeMap<&'static str, f64>,
    )>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
pub fn enable() {
    STATE.with(|s| {
        *s.borrow_mut() = State {
            enabled: true,
            ..State::default()
        }
    });
}
pub fn disable() {
    STATE.with(|s| s.borrow_mut().enabled = false);
}
pub fn enabled() -> bool {
    STATE.with(|s| s.borrow().enabled)
}
pub struct Observation {
    label: &'static str,
    started: Option<Instant>,
}
pub fn observe(label: &'static str) -> Observation {
    Observation {
        label,
        started: enabled().then(Instant::now),
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        if let Some(start) = self.started {
            record_substage(self.label, start.elapsed().as_secs_f64() * 1000.);
        }
    }
}
/// Nested observations do not move the surrounding step-phase boundary.
pub fn record_substage(name: &'static str, elapsed_ms: f64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.enabled && s.last.is_some() && elapsed_ms.is_finite() && elapsed_ms >= 0. {
            *s.current_substages.entry(name).or_default() += elapsed_ms;
        }
    });
}
pub fn begin(tick: u64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.enabled {
            s.tick = tick;
            s.current.clear();
            s.current_substages.clear();
            s.last = Some(Instant::now());
        }
    });
}
pub fn mark(name: &'static str) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if s.enabled {
            let now = Instant::now();
            let ms = s
                .last
                .map(|t| now.duration_since(t).as_secs_f64() * 1000.)
                .unwrap_or(0.);
            s.current.push((name, ms));
            s.last = Some(now);
        }
    });
}
pub fn end() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.enabled {
            return;
        }
        let phases = s.current.clone();
        let total = phases.iter().map(|p| p.1).sum::<f64>();
        for (name, value) in &phases {
            s.samples.entry(name).or_default().push(*value);
        }
        let tick = s.tick;
        let sub = s.current_substages.clone();
        for (name, value) in &sub {
            s.subsamples.entry(name).or_default().push(*value);
        }
        s.spikes.push((tick, total, phases, sub));
        s.spikes.sort_by(|a, b| b.1.total_cmp(&a.1));
        s.spikes.truncate(12);
        s.last = None;
    });
}
pub fn report() -> Value {
    fn stats(samples: &BTreeMap<&'static str, Vec<f64>>) -> BTreeMap<&'static str, Value> {
        samples.iter().filter(|(_,v)|!v.is_empty()).map(|(name,values)|{let mut v=values.clone();v.sort_by(f64::total_cmp);let at=|q:f64|v[((v.len()-1)as f64*q).ceil()as usize];(*name,json!({"samples":v.len(),"p50":at(0.5),"p95":at(0.95),"p99":at(0.99),"max":v.last(),"total":v.iter().sum::<f64>()}))}).collect()
    }
    STATE.with(|s|{let s=s.borrow();json!({"milliseconds":stats(&s.samples),"substageMilliseconds":stats(&s.subsamples),"substageSamples":"Per tick sums; nested scopes are inclusive and do not add to main total","topSpikes":s.spikes.iter().map(|(tick,total,phases,sub)|json!({"tick":tick,"total":total,"phases":phases.iter().map(|(n,v)|(*n,*v)).collect::<BTreeMap<_,_>>(),"substageMilliseconds":sub})).collect::<Vec<_>>()})})
}
