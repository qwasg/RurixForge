//! 长度前缀帧 IO:4 字节小端长度 + JSON 负载。

use std::io::{self, Read, Write};

/// 单帧上限(防异常长度前缀打爆内存)。
const MAX_FRAME: usize = 8 * 1024 * 1024;

/// 写一帧(JSON 序列化 + 长度前缀)。
pub fn write_frame<W: Write>(w: &mut W, v: &serde_json::Value) -> io::Result<()> {
    let payload = serde_json::to_vec(v)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "帧超长"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&payload)?;
    w.flush()
}

/// 读一帧原始字节;对端有序关闭 → `Err(UnexpectedEof)`。
pub fn read_frame_raw<R: Read>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "帧长超上限"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}
