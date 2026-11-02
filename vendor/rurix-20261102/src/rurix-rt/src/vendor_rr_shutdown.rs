//! U63/U32 final RR teardown proof. No Vulkan calls are hardcoded here.
//! Actual Drop supplies its real function table; tests supply CPU fake FFI.
use std::mem::ManuallyDrop;

/// U63 staged frame-end amendment: caller admission combines actual WSI fence,
/// native device completion and successful SDK shutdown. A rejected admission
/// never calls release or Drop, including when native device result was LOST.
pub(super) unsafe fn finish_owner_admitted<T>(
    owner:&mut ManuallyDrop<T>,already_quarantined:bool,
    admit:impl FnOnce(&mut T)->bool,release:impl FnOnce(&mut T),
)->bool {
    if already_quarantined||!admit(owner){return false;}
    release(owner);
    // SAFETY: U63/U32, sole owner; caller admission established every required native/SDK proof.
    unsafe{ManuallyDrop::drop(owner)};
    true
}

/// The caller owns this live ManuallyDrop exactly once. `release` destroys only
/// GPU/SDK objects whose use was covered by `wait` and leaves ordinary Rust
/// fields valid for one Drop. On retention, the caller must also retain any
/// additional imported-resource metadata outside `owner`. No later access or
/// second drop of a released owner is permitted.
///
/// SAFETY: U63/U32. SUCCESS and DEVICE_LOST are the only completion-equivalent
/// results for pending/in-use teardown. Other values cannot justify release.
pub(super) unsafe fn finish_owner<T>(
    owner:&mut ManuallyDrop<T>,
    already_quarantined:bool,
    wait:impl FnOnce(&mut T)->i32,
    release:impl FnOnce(&mut T),
    quarantine:impl FnOnce(i32),
)->bool {
    if already_quarantined {return false;}
    let result=wait(owner);
    if result!=0 && result!=-4 {
        quarantine(result);
        return false;
    }
    release(owner);
    // SAFETY: U63/U32, the sole owner passed the actual final wait gate; release
    // preserved the Rust fields and this helper performs their one explicit Drop.
    unsafe {ManuallyDrop::drop(owner)};
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc,atomic::{AtomicUsize,Ordering}};
    #[test]
    fn rejected_present_or_sdk_admission_never_releases_or_drops_owner(){
        struct Native {events:Arc<std::sync::Mutex<Vec<&'static str>>>,sdk_result:i32}
        impl Drop for Native {fn drop(&mut self){self.events.lock().unwrap().push("drop");}}
        unsafe extern "system" fn shutdown(raw:*mut Native)->i32{
            // SAFETY: U63 CPU fake FFI, pointer refers to the live exclusive stack test owner.
            let owner=unsafe{&mut *raw};owner.events.lock().unwrap().push("slShutdown");owner.sdk_result
        }
        for (present_retired,sdk_result) in [(false,0),(true,-1),(true,0)] {
            let events=Arc::new(std::sync::Mutex::new(Vec::new()));
            let mut owner=ManuallyDrop::new(Native{events:events.clone(),sdk_result});
            let admit=|native:&mut Native|{
                if !present_retired{return false;}
                native.events.lock().unwrap().push("present_proof");
                // SAFETY: U63 CPU fake FFI, unique Native owner remains live for the synchronous SDK stub.
                let result=unsafe{shutdown(native)};result==0
            };
            let release=|native:&mut Native|{native.events.lock().unwrap().push("destroy");};
            // SAFETY: U63 CPU fixture, rejected admission intentionally retains owner; accepted admission drops exactly once.
            let released=unsafe{finish_owner_admitted(&mut owner,false,admit,release)};
            let actual=events.lock().unwrap().clone();
            if !present_retired{assert!(!released);assert!(actual.is_empty());}
            else if sdk_result!=0{assert!(!released);assert_eq!(actual,vec!["present_proof","slShutdown"]);}
            else{assert!(released);assert_eq!(actual,vec!["present_proof","slShutdown","destroy","drop"]);}
        }
    }
    struct Audit {waits:AtomicUsize,releases:AtomicUsize,drops:AtomicUsize,quarantines:AtomicUsize}
    struct FakeCore {result:i32,audit:Arc<Audit>,idle_proof:bool}
    impl Drop for FakeCore {fn drop(&mut self){
        assert!(self.idle_proof,"inner destructor ran without admitted wait proof");
        self.audit.drops.fetch_add(1,Ordering::SeqCst);
    }}
    unsafe extern "system" fn wait(device:*mut FakeCore)->i32 {
        // SAFETY: U63 test fixture, device is a live exclusive FakeCore pointer.
        let core=unsafe{&mut *device};
        core.audit.waits.fetch_add(1,Ordering::SeqCst);core.result
    }
    unsafe extern "system" fn destroy(device:*mut FakeCore) {
        // SAFETY: U63 test fixture, device is a live exclusive FakeCore pointer.
        let core=unsafe{&mut *device};
        core.audit.releases.fetch_add(1,Ordering::SeqCst);core.idle_proof=true;
    }
    fn run(result:i32,already:bool)->(bool,Arc<Audit>){
        let audit=Arc::new(Audit{waits:AtomicUsize::new(0),releases:AtomicUsize::new(0),
            drops:AtomicUsize::new(0),quarantines:AtomicUsize::new(0)});
        let mut owner=ManuallyDrop::new(FakeCore{result,audit:audit.clone(),idle_proof:false});
        let do_wait=|core:&mut FakeCore|{
            // SAFETY: U63 CPU fixture only; no driver function or device.
            unsafe{wait(core)}
        };
        let do_release=|core:&mut FakeCore|{
            // SAFETY: U63 CPU fixture, invoked only under final-wait proof.
            unsafe{destroy(core)}
        };
        // SAFETY: U63 fixture owns this ManuallyDrop once and never reuses it.
        let released=unsafe{finish_owner(&mut owner,already,do_wait,do_release,|r|{
            assert_eq!(r,result);audit.quarantines.fetch_add(1,Ordering::SeqCst);
        })};
        (released,audit)
    }
    #[test]fn actual_fake_ffi_success_and_device_lost_release_once(){
        for result in [0,-4] {
            let(released,a)=run(result,false);assert!(released);
            assert_eq!(a.waits.load(Ordering::SeqCst),1);
            assert_eq!(a.releases.load(Ordering::SeqCst),1);
            assert_eq!(a.drops.load(Ordering::SeqCst),1);
            assert_eq!(a.quarantines.load(Ordering::SeqCst),0);
        }
    }
    #[test]fn actual_fake_ffi_other_errors_retain_inner_owner(){
        for result in [-1,-2,-13,2,i32::MIN] {
            let(released,a)=run(result,false);assert!(!released);
            assert_eq!(a.waits.load(Ordering::SeqCst),1);
            assert_eq!(a.releases.load(Ordering::SeqCst),0);
            assert_eq!(a.drops.load(Ordering::SeqCst),0);
            assert_eq!(a.quarantines.load(Ordering::SeqCst),1);
        }
    }
    #[test]fn already_quarantined_performs_no_driver_or_sdk_call(){
        let(released,a)=run(0,true);assert!(!released);
        assert_eq!(a.waits.load(Ordering::SeqCst),0);
        assert_eq!(a.releases.load(Ordering::SeqCst),0);
        assert_eq!(a.drops.load(Ordering::SeqCst),0);
        assert_eq!(a.quarantines.load(Ordering::SeqCst),0);
    }
}
