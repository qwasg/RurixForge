//! 安装包载荷。最终的 `RurixForge-Setup-<版本>.exe` 由四段首尾相接:
//!
//! ```text
//! forge-setup.exe ‖ 内层 NSIS 安装包 ‖ 元数据 JSON ‖ 尾标(32 字节)
//! 尾标 = 内层安装包长度(u64 LE)‖ 元数据长度(u64 LE)‖ MAGIC(16 字节)
//! ```
//!
//! 运行时从自身 exe 末尾读尾标定位另外两段;Windows 加载 PE 时不看文件尾部追加的数据。

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const MAGIC: &[u8; 16] = b"RurixForgeSetup\x01";
const TRAILER_LEN: u64 = 32;

/// 出包时写入的产品信息;安装界面和安装流程只认这里,不写死产品名。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    /// 产品名:安装文件夹名、Electron 的用户数据目录名(%APPDATA%\<产品名>)、卸载信息的显示名前缀。
    pub product_name: String,
    pub version: String,
    /// 安装目录里的主程序文件名。
    pub exe_name: String,
    /// 装好后安装目录的大致字节数,用于空间检查与进度估算。
    pub installed_bytes: u64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub homepage: String,
    /// 许可证全文,在安装界面里查看。
    #[serde(default)]
    pub license: String,
}

#[derive(Clone, Debug)]
pub struct Payload {
    pub meta: Meta,
    /// 内层安装包在文件中的起始偏移与长度。
    pub offset: u64,
    pub len: u64,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// 读取 `path`(通常是正在运行的安装程序自身)末尾的载荷信息。
pub fn read(path: &Path) -> io::Result<Payload> {
    let mut file = File::open(path)?;
    let total = file.seek(SeekFrom::End(0))?;
    if total < TRAILER_LEN {
        return Err(invalid("安装包不完整"));
    }
    file.seek(SeekFrom::Start(total - TRAILER_LEN))?;
    let mut trailer = [0u8; TRAILER_LEN as usize];
    file.read_exact(&mut trailer)?;
    if &trailer[16..] != MAGIC {
        return Err(invalid("安装包里没有安装内容"));
    }
    let inner_len = u64::from_le_bytes(trailer[0..8].try_into().unwrap());
    let meta_len = u64::from_le_bytes(trailer[8..16].try_into().unwrap());
    let meta_start = (total - TRAILER_LEN)
        .checked_sub(meta_len)
        .ok_or_else(|| invalid("安装包元数据损坏"))?;
    let offset = meta_start
        .checked_sub(inner_len)
        .ok_or_else(|| invalid("安装包内容损坏"))?;
    if meta_len > 16 * 1024 * 1024 {
        return Err(invalid("安装包元数据损坏"));
    }
    file.seek(SeekFrom::Start(meta_start))?;
    let mut raw = vec![0u8; meta_len as usize];
    file.read_exact(&mut raw)?;
    let meta: Meta = serde_json::from_slice(&raw).map_err(|_| invalid("安装包元数据损坏"))?;
    Ok(Payload { meta, offset, len: inner_len })
}

/// 把内层安装包复制到 `dest`;`progress` 收到已复制的字节数。
pub fn extract(
    source: &Path,
    payload: &Payload,
    dest: &Path,
    mut progress: impl FnMut(u64),
) -> io::Result<()> {
    let mut file = File::open(source)?;
    file.seek(SeekFrom::Start(payload.offset))?;
    let mut reader = file.take(payload.len);
    let mut out = File::create(dest)?;
    let mut buf = vec![0u8; 1 << 20];
    let mut copied = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        copied += n as u64;
        progress(copied);
    }
    if copied != payload.len {
        return Err(invalid("安装包内容不完整"));
    }
    out.flush()
}

/// 大块复制(io::copy 的 8 KB 缓冲拷几百 MB 太慢)。
fn copy_all(from: &Path, to: &mut File) -> io::Result<u64> {
    let mut src = File::open(from)?;
    let mut buf = vec![0u8; 4 << 20];
    let mut total = 0u64;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            return Ok(total);
        }
        to.write_all(&buf[..n])?;
        total += n as u64;
    }
}

/// 出包:`stub`(forge-setup.exe)+ `inner`(NSIS 安装包)+ 元数据 → `out`。返回写出的总字节数。
pub fn pack(stub: &Path, inner: &Path, meta: &Meta, out: &Path) -> io::Result<u64> {
    let mut dest = File::create(out)?;
    let stub_len = copy_all(stub, &mut dest)?;
    let inner_len = copy_all(inner, &mut dest)?;
    let raw = serde_json::to_vec(meta).map_err(io::Error::other)?;
    dest.write_all(&raw)?;
    dest.write_all(&inner_len.to_le_bytes())?;
    dest.write_all(&(raw.len() as u64).to_le_bytes())?;
    dest.write_all(MAGIC)?;
    dest.flush()?;
    Ok(stub_len + inner_len + raw.len() as u64 + TRAILER_LEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("forge-setup-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pack_then_read_and_extract_round_trips() {
        let dir = scratch("roundtrip");
        let stub = dir.join("stub.exe");
        let inner = dir.join("inner.exe");
        fs::write(&stub, b"MZ stub bytes").unwrap();
        let inner_bytes: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&inner, &inner_bytes).unwrap();
        let meta = Meta {
            product_name: "RurixForge".into(),
            version: "0.2.0".into(),
            exe_name: "RurixForge.exe".into(),
            installed_bytes: 1_234,
            description: "说明".into(),
            homepage: "https://example.com".into(),
            license: "Apache".into(),
        };
        let out = dir.join("setup.exe");
        let total = pack(&stub, &inner, &meta, &out).unwrap();
        assert_eq!(total, fs::metadata(&out).unwrap().len());

        let payload = read(&out).unwrap();
        assert_eq!(payload.meta, meta);
        assert_eq!(payload.offset, 13);
        assert_eq!(payload.len, inner_bytes.len() as u64);

        let extracted = dir.join("extracted.exe");
        let mut last = 0;
        extract(&out, &payload, &extracted, |n| last = n).unwrap();
        assert_eq!(last, inner_bytes.len() as u64);
        assert_eq!(fs::read(&extracted).unwrap(), inner_bytes);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_rejects_files_without_payload() {
        let dir = scratch("nopayload");
        let plain = dir.join("plain.exe");
        fs::write(&plain, vec![7u8; 4096]).unwrap();
        assert!(read(&plain).is_err());
        let tiny = dir.join("tiny.exe");
        fs::write(&tiny, b"MZ").unwrap();
        assert!(read(&tiny).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
