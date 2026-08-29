//! SHA-256 十六进制摘要(一次性 + 流式)。
//!
//! 收编缘由:仓内 SHA-256 已有三处逐字重复的调用样板——`assetd::meta::MetaDoc::cache_key`
//! 内联、`forge_index::sha256_hex`、`gend` 侧缓存键;F11 商店的下载校验若再写一份就是
//! 第四份。故在此收口成唯一入口。算法根仍委托 `rurix_pkg::sha256`(手写 FIPS 180-4,
//! 确定性、可审计、自身零第三方依赖),本模块不重新实现哈希。
//!
//! 取舍留痕:引入 `rurix-pkg` 后 forge-util 不再是「零依赖」crate(Cargo.toml
//! description 已如实更新)。代价可接受——该依赖是仓内 path 依赖且自身零第三方,
//! 换来的是消除第四份重复实现与「同一份字节两处算出不同 hex」的风险。

use std::path::Path;

/// 一次性 SHA-256 → 64 位小写 hex。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = rurix_pkg::sha256::Sha256::new();
    h.update(bytes);
    rurix_pkg::sha256::hex(&h.finalize())
}

/// 流式 SHA-256:分段喂入,大文件不必整读进内存。
pub struct Sha256Stream {
    inner: rurix_pkg::sha256::Sha256,
}

impl Default for Sha256Stream {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256Stream {
    pub fn new() -> Self {
        Sha256Stream { inner: rurix_pkg::sha256::Sha256::new() }
    }

    /// 喂入一段字节(可多次调用;分段边界不影响结果)。
    pub fn update(&mut self, bytes: &[u8]) {
        self.inner.update(bytes);
    }

    /// 收尾 → 64 位小写 hex(消费自身,避免复用已 finalize 的状态)。
    pub fn finish(self) -> String {
        rurix_pkg::sha256::hex(&self.inner.finalize())
    }
}

/// 分块读文件算 SHA-256(64KiB 块;不把整文件读进内存)。
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256Stream::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FIPS 180-4 标准向量(与 forge_index::sha256_hex / assetd 缓存键同源实现,
    /// 此处硬编码标准值而非跨 crate 比对——forge-util 位于依赖树下游,不可反向依赖)。
    #[test]
    fn known_answer_vectors() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(sha256_hex(b"abc").len(), 64);
        assert!(sha256_hex(b"abc").chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()));
    }

    #[test]
    fn stream_matches_oneshot_regardless_of_chunking() {
        let data: Vec<u8> = (0u32..300_000).map(|i| (i % 251) as u8).collect();
        let expect = sha256_hex(&data);
        // 分段边界跨 64 字节块边界与缓冲区边界。
        for chunk in [1usize, 7, 64, 1000, 65_536] {
            let mut h = Sha256Stream::new();
            for part in data.chunks(chunk) {
                h.update(part);
            }
            assert_eq!(h.finish(), expect, "分块 {chunk} 结果须与一次性一致");
        }
        // 空输入流式 = 空输入一次性。
        assert_eq!(Sha256Stream::new().finish(), sha256_hex(b""));
    }

    #[test]
    fn file_hash_matches_bytes_hash() {
        let dir = std::env::temp_dir().join(format!("forge-util-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("sample.bin");
        let data: Vec<u8> = (0u32..200_000).map(|i| (i % 97) as u8).collect();
        std::fs::write(&p, &data).unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_hex(&data));
        // 缺文件如实报 io 错误,不静默返回空 hash。
        assert!(sha256_file(&dir.join("no-such.bin")).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
