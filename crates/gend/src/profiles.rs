//! User-named connections share protocol adapters, but never their configuration or secrets.
use std::{io, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    backends::{EditRequest, GenBackend, GenCandidate, GenRequest},
    config::GenConfig,
    keystore::Keystore,
    media::{MediaArtifact, MediaBackend, MediaRequest},
    Result,
};

#[derive(Clone, Serialize, Deserialize)]
pub struct BackendProfile {
    pub id: String,
    pub label: String,
    pub adapter: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Profiles {
    pub profiles: Vec<BackendProfile>,
}

impl Profiles {
    pub fn path() -> PathBuf {
        crate::config::data_dir().join("gen-profiles.json")
    }

    pub fn load() -> io::Result<Self> {
        match std::fs::read(Self::path()) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    pub fn entry(&self, id: &str) -> Option<&BackendProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn upsert(&mut self, profile: BackendProfile) {
        if let Some(old) = self.profiles.iter_mut().find(|p| p.id == profile.id) {
            *old = profile;
        } else {
            self.profiles.push(profile);
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, bytes)
    }
}

pub fn valid_profile_id(id: &str) -> bool {
    id.starts_with("custom-") && id.len() > 7 && id.len() <= 80
        && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn scoped_config(profile: &BackendProfile, cfg: &GenConfig) -> GenConfig {
    GenConfig {
        backends: cfg.entry(&profile.id).cloned().map(|mut e| {
            e.id = profile.adapter.clone();
            e
        }).into_iter().collect(),
    }
}

pub fn image_backend(profile: &BackendProfile) -> Option<Box<dyn GenBackend>> {
    let inner = crate::backends::builtin_registry().into_iter().find(|b| b.id() == profile.adapter)?;
    Some(Box::new(ImageConnection { profile: profile.clone(), inner }))
}

pub fn media_backend(profile: &BackendProfile) -> Option<Box<dyn MediaBackend>> {
    let inner = crate::media::builtin_media_registry().into_iter().find(|b| b.id() == profile.adapter)?;
    Some(Box::new(MediaConnection { profile: profile.clone(), inner }))
}

struct ImageConnection {
    profile: BackendProfile,
    inner: Box<dyn GenBackend>,
}

impl GenBackend for ImageConnection {
    fn id(&self) -> &str { &self.profile.id }
    fn kind(&self) -> &str { self.inner.kind() }
    fn capabilities(&self) -> Value { self.inner.capabilities() }
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        self.inner.configured(&scoped_config(&self.profile, cfg), &keys.for_backend_alias(self.id(), &self.profile.adapter))
    }
    fn generate(&self, req: &GenRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<GenCandidate>> {
        if !self.configured(cfg, keys) { return Err(crate::GenError::new(crate::GEN_BACKEND_NOT_CONFIGURED, "连接尚未配置或已停用")); }
        self.inner.generate(req, &scoped_config(&self.profile, cfg), &keys.for_backend_alias(self.id(), &self.profile.adapter))
    }
    fn edit(&self, req: &EditRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<GenCandidate>> {
        if !self.configured(cfg, keys) { return Err(crate::GenError::new(crate::GEN_BACKEND_NOT_CONFIGURED, "连接尚未配置或已停用")); }
        self.inner.edit(req, &scoped_config(&self.profile, cfg), &keys.for_backend_alias(self.id(), &self.profile.adapter))
    }
}

struct MediaConnection {
    profile: BackendProfile,
    inner: Box<dyn MediaBackend>,
}

impl MediaBackend for MediaConnection {
    fn id(&self) -> &str { &self.profile.id }
    fn kind(&self) -> &str { self.inner.kind() }
    fn capabilities(&self) -> Value { self.inner.capabilities() }
    fn configured(&self, cfg: &GenConfig, keys: &Keystore) -> bool {
        self.inner.configured(&scoped_config(&self.profile, cfg), &keys.for_backend_alias(self.id(), &self.profile.adapter))
    }
    fn generate(&self, req: &MediaRequest, cfg: &GenConfig, keys: &Keystore) -> Result<Vec<MediaArtifact>> {
        if !self.configured(cfg, keys) { return Err(crate::GenError::new(crate::GEN_BACKEND_NOT_CONFIGURED, "连接尚未配置或已停用")); }
        self.inner.generate(req, &scoped_config(&self.profile, cfg), &keys.for_backend_alias(self.id(), &self.profile.adapter))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::BackendEntry, keystore::set_key_at};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new() -> Self {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("gend-profiles-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &std::path::Path { &self.0 }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join("keys.json"));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn connections_do_not_inherit_template_keys_or_configuration() {
        let _guard = crate::TEST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TestDir::new();
        let path = dir.path().join("keys.json");
        let profile = BackendProfile { id: "custom-images".into(), label: "Images".into(), adapter: "remote-openai-compatible".into() };
        let entry = |id: &str| BackendEntry { id: id.into(), kind: "remote".into(), enabled: true, endpoint: Some("https://images.example.test".into()), model: Some("image-model".into()) };
        let cfg = GenConfig { backends: vec![entry(&profile.adapter), entry(&profile.id)] };
        set_key_at(&path, &profile.adapter, "template-secret").unwrap();
        let keys = Keystore::load_from(&path);
        assert!(keys.for_backend_alias(&profile.id, &profile.adapter).secret_for(&profile.adapter).is_none());
        let backend = image_backend(&profile).unwrap();
        assert!(!backend.configured(&cfg, &keys));
        set_key_at(&path, &profile.id, "connection-secret").unwrap();
        let keys = Keystore::load_from(&path);
        assert!(backend.configured(&cfg, &keys));
        assert_eq!(keys.for_backend_alias(&profile.id, &profile.adapter).secret_for(&profile.adapter).as_deref(), Some("connection-secret"));
        assert!(!backend.configured(&GenConfig { backends: vec![entry(&profile.adapter)] }, &keys));
    }

    #[test]
    fn named_local_connection_generates_using_its_own_enabled_state() {
        let dir = TestDir::new();
        let keys = Keystore::load_from(&dir.path().join("missing.json"));
        let profile = BackendProfile { id: "custom-local".into(), label: "Local".into(), adapter: "local-mock".into() };
        let mut cfg = GenConfig { backends: vec![BackendEntry { id: profile.id.clone(), kind: "local".into(), enabled: true, endpoint: None, model: None }] };
        let b = image_backend(&profile).unwrap();
        assert_eq!(b.id(), profile.id);
        let result = b.generate(&GenRequest::square("square", None, 256, 7, 1), &cfg, &keys).unwrap();
        assert_eq!(result.len(), 1);
        assert!(result[0].png_bytes.starts_with(b"\x89PNG"));
        cfg.backends[0].enabled = false;
        assert!(!b.configured(&cfg, &keys));
        assert!(b.generate(&GenRequest::square("square", None, 256, 7, 1), &cfg, &keys).is_err());
    }

    #[test]
    fn media_connections_keep_their_identity_and_protocol_capabilities() {
        let p = BackendProfile { id: "custom-video".into(), label: "Video".into(), adapter: "remote-video-compatible".into() };
        let b = media_backend(&p).unwrap();
        assert_eq!(b.id(), p.id);
        assert_eq!(b.kind(), "remote");
        assert!(b.capabilities()["kinds"].as_array().unwrap().iter().any(|v| v == "text2video"));
        assert!(valid_profile_id(&p.id));
        assert!(!valid_profile_id("custom-../escape"));
    }
}
