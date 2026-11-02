//! Artifact-only recorder boundary. No public runtime API is enabled.
//! Assisted-by: Codex:GPT-6. Proposed semantics: prior FULL_RFC_DRAFT.md.
#![forbid(unsafe_code)]
use std::ops::Range;
pub type State = (u32,u64,u64);

/// Original fixed-pass indices; never renumber descriptors or timestamp slots.
#[derive(Debug)]
pub struct Scope {
    pub range: Range<usize>,
    pub final_suffix: bool,
    pub header: usize,
    pub tracker: Vec<State>,
    pub query_epoch: u64,
    pub skip_last_timestamp: bool,
}
impl Scope {
    pub fn validate(&self, passes: usize, resources: usize, query_base: u32) -> Result<Range<u32>,String> {
        if self.range.start>=self.range.end || self.range.end>passes || self.range.len()>256 {
            return Err("region must be a nonempty bounded original-pass range".into());
        }
        if self.header>=resources || self.tracker.len()!=resources || self.query_epoch==0 {
            return Err("invalid carried tracker/header/query epoch".into());
        }
        if self.final_suffix && self.range.end!=passes {return Err("suffix does not end at graph end".into());}
        let start=u32::try_from(self.range.start).map_err(|_|"pass index overflow")?
            .checked_mul(2).and_then(|v|query_base.checked_add(v)).ok_or("query start overflow")?;
        let end=u32::try_from(self.range.end).map_err(|_|"pass index overflow")?
            .checked_mul(2).and_then(|v|query_base.checked_add(v)).ok_or("query end overflow")?;
        Ok(start..end)
    }
}

/// Values are [tick,availability] exactly as queried using Vulkan's
/// QUERY_RESULT_64_BIT | QUERY_RESULT_WITH_AVAILABILITY_BIT, after fence.
/// No WAIT_BIT: missing values fail rather than opening an unbounded wait.
pub fn decode_timestamps(scope:&Scope, raw:&[[u64;2]]) -> Result<Vec<(usize,u64,u64)>,String> {
    if raw.len()!=scope.range.len()*2 || raw.iter().any(|v|v[1]==0) {
        return Err("missing/unavailable region timestamp".into());
    }
    Ok(scope.range.clone().enumerate().map(|(i,pass)|(pass,raw[i*2][0],raw[i*2+1][0])).collect())
}

#[cfg(test)] mod tests {
    use super::*;
    fn s()->Scope{Scope{range:7..10,final_suffix:false,header:2,tracker:vec![(0,4,8);3],query_epoch:9,skip_last_timestamp:false}}
    #[test] fn original_indices_and_query_base_survive(){let s=s();assert_eq!(s.validate(15,3,100).unwrap(),114..120);assert_eq!(decode_timestamps(&s,&[[11,1],[12,1],[13,1],[14,1],[15,1],[16,1]]).unwrap(),vec![(7,11,12),(8,13,14),(9,15,16)]);}
    #[test] fn holes_and_extra_old_queries_fail(){let s=s();for n in [0,4,5,7,30]{assert!(decode_timestamps(&s,&vec![[0,1];n]).is_err());}let mut r=vec![[0,1];6];r[4][1]=0;assert!(decode_timestamps(&s,&r).is_err());}
    #[test] fn every_nonzero_availability_is_valid(){let s=s();let r=[[0,2],[12,3],[13,u64::MAX],[14,1],[15,256],[16,0x1_0000_0000]];assert_eq!(decode_timestamps(&s,&r).unwrap(),vec![(7,0,12),(8,13,14),(9,15,16)]);}
    #[test] fn invalid_scope_fails(){let mut s=s();s.range=10..7;assert!(s.validate(15,3,0).is_err());s.range=0..257;assert!(s.validate(300,3,0).is_err());s.range=7..10;s.final_suffix=true;assert!(s.validate(15,3,0).is_err());s.final_suffix=false;s.tracker.pop();assert!(s.validate(15,3,0).is_err());}
    #[test] fn query_overflow_fails(){assert!(s().validate(15,3,u32::MAX-10).is_err());}
    #[test] fn tracker_is_not_reset_between_scopes(){let mut s=s();s.tracker[0]=(0,0x800,0x60);let before=s.tracker.clone();s.range=7..10;s.query_epoch+=1;assert!(s.validate(15,3,0).is_ok());assert_eq!(s.tracker,before);}
}
