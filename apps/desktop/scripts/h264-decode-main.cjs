'use strict';

// F1 wave.4 G-F1-13 证据腿:Annex B H.264 码流经 Chromium WebCodecs VideoDecoder 解码,
// 校验解码帧尺寸与编码请求尺寸一致(纯 web 客户端可解码的直接证据)。
// 用法: electron h264-decode-main.cjs --nal <annexb文件> --out <结果json> --expect-w <宽> --expect-h <高>
// 退出码: 0 = 解码成功且尺寸一致; 2 = 解码断言失败; 1 = harness 错误。
// 结果权威载体 = --out JSON 文件(GUI 子进程 stdout 不可达,见 RD 经验)。

const { app, BrowserWindow } = require('electron');
const fs = require('fs');
const http = require('http');

function argValue(name) {
  const i = process.argv.indexOf(name);
  if (i < 0 || i + 1 >= process.argv.length) return null;
  return process.argv[i + 1];
}

const nalPath = argValue('--nal');
const outPath = argValue('--out');
const expectW = Number(argValue('--expect-w') || '0');
const expectH = Number(argValue('--expect-h') || '0');

function writeOut(obj, code) {
  try {
    fs.writeFileSync(outPath, JSON.stringify(obj, null, 2));
  } catch (e) {
    console.error(`[h264-decode] 结果落盘失败: ${e.message}`);
  }
  app.exit(code);
}

if (!nalPath || !outPath || !expectW || !expectH) {
  console.error('[h264-decode] 缺参: --nal/--out/--expect-w/--expect-h 均必填');
  app.exit(1);
}

const nalB64 = fs.readFileSync(nalPath).toString('base64');

const DECODE_JS = `(async () => {
  if (typeof VideoDecoder !== 'function') throw new Error('WebCodecs VideoDecoder 不可用');
  const raw = Uint8Array.from(atob(${JSON.stringify(nalB64)}), (c) => c.charCodeAt(0));
  // Annex B NAL 扫描(3/4 字节起始码均容忍)。
  function nextStart(buf, pos) {
    for (let k = pos; k + 3 < buf.length; k++) {
      if (buf[k] === 0 && buf[k + 1] === 0) {
        if (buf[k + 2] === 1) return { off: k, len: 3 };
        if (k + 4 < buf.length && buf[k + 2] === 0 && buf[k + 3] === 1) return { off: k, len: 4 };
      }
    }
    return null;
  }
  const nals = [];
  let cur = nextStart(raw, 0);
  while (cur) {
    const nxt = nextStart(raw, cur.off + cur.len);
    nals.push({ start: cur.off + cur.len, end: nxt ? nxt.off : raw.length });
    cur = nxt;
  }
  if (nals.length === 0) throw new Error('缺 Annex B 起始码');
  const nalTypes = nals.map(({ start }) => raw[start] & 0x1f);
  const spsIdx = nalTypes.indexOf(7);
  if (spsIdx < 0) throw new Error('缺 SPS(NAL type 7)');
  const sps = raw.subarray(nals[spsIdx].start, nals[spsIdx].end);
  // codec 串 = avc1.profile_idc constraint_flags level_idc(直读 SPS 前三字节,不猜 profile)。
  const hex = (b) => b.toString(16).padStart(2, '0');
  const codec = 'avc1.' + hex(sps[1]) + hex(sps[2]) + hex(sps[3]);
  const frames = [];
  let decErr = null;
  const decoder = new VideoDecoder({
    // 权威尺寸 = visibleRect(SPS 声明 320x240 无裁剪);codedHeight 是 Chromium 内部分配对齐值,不作判据。
    output: (f) => { frames.push({ w: f.visibleRect.width, h: f.visibleRect.height, codedW: f.codedWidth, codedH: f.codedHeight }); f.close(); },
    error: (e) => { decErr = e && e.message ? e.message : String(e); },
  });
  decoder.configure({ codec, avc: { format: 'annexb' }, hardwareAcceleration: 'prefer-software' });
  decoder.decode(new EncodedVideoChunk({ type: 'key', timestamp: 0, data: raw }));
  await decoder.flush().catch((e) => { throw new Error('flush 失败: ' + (e && e.message ? e.message : e)); });
  decoder.close();
  if (decErr) throw new Error('解码错误: ' + decErr);
  return { codec, nalCount: nals.length, nalTypes, decoded: frames.length, frames };
})()`;

app.whenReady().then(async () => {
  // WebCodecs 仅在安全上下文暴露:data: URL 非安全上下文 → 起环回 http 服务供页。
  const server = http.createServer((req, res) => {
    res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
    res.end('<html><body>h264-decode</body></html>');
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const port = server.address().port;
  const win = new BrowserWindow({
    show: false,
    width: 64,
    height: 64,
    webPreferences: { offscreen: true },
  });
  const guard = setTimeout(() => {
    writeOut({ ok: false, error: '解码 30s 超时' }, 1);
  }, 30_000);
  try {
    await win.loadURL(`http://127.0.0.1:${port}/`);
    const r = await win.webContents.executeJavaScript(DECODE_JS);
    clearTimeout(guard);
    const first = r.frames[0] || { w: 0, h: 0 };
    const sizeMatch = r.decoded >= 1 && first.w === expectW && first.h === expectH;
    writeOut(
      {
        ok: sizeMatch,
        codec: r.codec,
        nalCount: r.nalCount,
        nalTypes: r.nalTypes,
        decoded: r.decoded,
        frames: r.frames,
        expectW,
        expectH,
        sizeMatch,
      },
      sizeMatch ? 0 : 2
    );
  } catch (e) {
    clearTimeout(guard);
    writeOut({ ok: false, error: e && e.message ? e.message : String(e) }, 1);
  }
});
