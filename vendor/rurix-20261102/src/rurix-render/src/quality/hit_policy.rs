//! RFC-0053 private native-t closest surface contract and independent host oracle.
//! No language candidate API or default runtime policy changes.
//! Assisted-by: Codex:gpt-6-astra.
use std::collections::BTreeSet;

pub const CONTROL_WORDS: usize = 16;
pub const CONTROL_MAGIC: u32 = 0x31535448;
pub const POLICY: &str = "native-t-instance-geometry-primitive-v1";
pub const AS_POLICY: &str = "unique-triangle-candidates-v1";
pub const ERROR_TYPE: u32 = 1;
pub const ERROR_NUMERIC: u32 = 2;
pub const ERROR_INSTANCE: u32 = 4;
pub const ERROR_GEOMETRY: u32 = 8;
pub const ERROR_PRIMITIVE: u32 = 16;
pub const ERROR_COUNT: u32 = 32;
pub const ERROR_SELECTED_DUPLICATE: u32 = 64;
pub const ERROR_ABI: u32 = 128;

#[derive(Clone, Copy, Debug)]
pub struct InstanceBound {
    pub primitives: u32,
    pub custom_index: u32,
    pub mask: u32,
    pub facing_flags: u32,
}

pub fn control_words(instances: &[InstanceBound]) -> Result<[u32; CONTROL_WORDS], String> {
    if instances.is_empty() || instances.len()>2 { return Err("private hit policy requires one or two actual instances".into()); }
    let mut words=[0;CONTROL_WORDS];words[1]=CONTROL_MAGIC;words[2]=1;
    words[3]=instances.len() as u32;words[5]=3;words[6]=255;
    for (index,instance) in instances.iter().enumerate() {
        if instance.primitives==0 || instance.custom_index>0xffffff || instance.mask!=255 || instance.facing_flags>1 {
            return Err("unsupported private AS primitive/custom/mask/facing domain".into());
        }
        words[4]=words[4].checked_add(instance.primitives).ok_or("candidate upper-bound sum exceeds u32")?;
        let offset=8+index*4;
        words[offset..offset+4].copy_from_slice(&[instance.primitives,instance.custom_index,instance.mask,instance.facing_flags]);
    }
    Ok(words)
}

pub fn control_bytes(words: &[u32; CONTROL_WORDS]) -> [u8; CONTROL_WORDS*4] {
    let mut result=[0;CONTROL_WORDS*4];
    for (slot,value) in result.chunks_exact_mut(4).zip(words) { slot.copy_from_slice(&value.to_le_bytes()); }
    result
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    pub native_t: f32,
    pub instance: u32,
    pub geometry: u32,
    pub primitive: u32,
    pub custom_index: u32,
    pub barycentric: [f32; 2],
    pub front_face: bool,
}

/// Complete-list reference, intentionally stronger than a constant-state shader:
/// checks every duplicate identity, including candidates that never win.
pub fn select_reference(candidates: &[Candidate], instances: &[InstanceBound], interval: [f32;2])
    -> Result<Option<Candidate>,u32>
{
    let words=control_words(instances).map_err(|_|ERROR_ABI)?;
    if interval.iter().any(|v|!v.is_finite()) || interval[0]<0.0 || interval[1]<interval[0] { return Err(ERROR_NUMERIC); }
    if candidates.len() as u64>u64::from(words[4]) { return Err(ERROR_COUNT); }
    let mut seen=BTreeSet::new();let mut best: Option<Candidate>=None;
    for &candidate in candidates {
        if !candidate.native_t.is_finite() || candidate.barycentric.iter().any(|v|!v.is_finite())
            || candidate.native_t<interval[0] || candidate.native_t>interval[1] {return Err(ERROR_NUMERIC);}
        let instance=instances.get(candidate.instance as usize).ok_or(ERROR_INSTANCE)?;
        if candidate.custom_index!=instance.custom_index {return Err(ERROR_INSTANCE);}
        if candidate.geometry!=0 {return Err(ERROR_GEOMETRY);}
        if candidate.primitive>=instance.primitives {return Err(ERROR_PRIMITIVE);}
        let id=(candidate.instance,candidate.geometry,candidate.primitive);
        if !seen.insert(id) {return Err(ERROR_SELECTED_DUPLICATE);}
        let mut canonical=candidate;
        if canonical.native_t==0.0 {canonical.native_t=0.0;}
        if best.is_none_or(|b| canonical.native_t<b.native_t ||
            (canonical.native_t==b.native_t && id<(b.instance,b.geometry,b.primitive))) {
            best=Some(canonical);
        }
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(n:u32)->InstanceBound {InstanceBound{primitives:n,custom_index:7,mask:255,facing_flags:1}}
    fn c(instance:u32,primitive:u32,t:f32)->Candidate {Candidate{native_t:t,instance,geometry:0,primitive,custom_index:7,barycentric:[0.2,0.3],front_face:true}}
    #[test]
    fn bounds_sum_instances_even_when_they_reference_one_blas() {
        let words=control_words(&[bounds(8),bounds(8)]).unwrap();assert_eq!(words[4],16);
        assert_eq!(words[8],8);assert_eq!(words[12],8);assert_eq!(control_bytes(&words).len(),64);
        assert_eq!(&control_bytes(&words)[4..8],&CONTROL_MAGIC.to_le_bytes());
        assert!(control_words(&[bounds(u32::MAX),bounds(1)]).is_err());
        assert!(control_words(&[]).is_err());assert!(control_words(&[bounds(1);3]).is_err());
    }
    #[test]
    fn minimum_is_permutation_invariant_and_custom_id_is_not_the_key() {
        let candidates=[c(1,0,1.0),c(0,3,1.0),c(0,1,1.0),c(0,0,1.0000001)];
        for shift in 0..4 {
            let mut rows=candidates;rows.rotate_left(shift);
            for reverse in [false,true] {
                if reverse {rows.reverse();}
                assert_eq!(select_reference(&rows,&[bounds(8);2],[0.,10.]).unwrap(),Some(c(0,1,1.0)));
            }
        }
    }
    #[test]
    fn no_epsilon_tie_and_signed_zero_canonicalization() {
        let near=f32::from_bits(1.0f32.to_bits()+1);
        assert_eq!(select_reference(&[c(0,0,near),c(0,1,1.)],&[bounds(2)],[0.,10.]).unwrap(),Some(c(0,1,1.)));
        let chosen=select_reference(&[c(0,1,-0.),c(0,0,0.)],&[bounds(2)],[0.,10.]).unwrap().unwrap();
        assert_eq!(chosen.native_t.to_bits(),0);assert_eq!(chosen.primitive,0);
        assert_eq!(select_reference(&[],&[bounds(2)],[0.,10.]),Ok(None));
        let subnormal=f32::from_bits(1);
        assert_eq!(select_reference(&[c(0,0,subnormal),c(0,1,0.)],&[bounds(2)],[0.,1.]).unwrap(),Some(c(0,1,0.)));
    }
    #[test]
    fn full_capture_oracle_rejects_discarded_duplicates_and_bad_domains() {
        assert_eq!(select_reference(&[c(0,0,1.),c(0,2,9.),c(0,2,9.)],&[bounds(4)],[0.,10.]),Err(ERROR_SELECTED_DUPLICATE));
        for (mut candidate,expected) in [(c(2,0,1.),ERROR_INSTANCE),(c(0,4,1.),ERROR_PRIMITIVE),(c(0,0,f32::NAN),ERROR_NUMERIC)] {
            assert_eq!(select_reference(&[candidate],&[bounds(4)],[0.,10.]),Err(expected));
            candidate=c(0,0,1.);candidate.geometry=1;
            assert_eq!(select_reference(&[candidate],&[bounds(4)],[0.,10.]),Err(ERROR_GEOMETRY));
        }
        let mut candidate=c(0,0,1.);candidate.barycentric=[-f32::EPSILON,0.5];
        assert_eq!(select_reference(&[candidate],&[bounds(4)],[0.,10.]),Ok(Some(candidate)));
    }
}
