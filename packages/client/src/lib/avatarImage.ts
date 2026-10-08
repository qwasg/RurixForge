/**
 * 头像上传前的客户端缩放:最长边 ≤256,优先 PNG;dataUrl 超过 512 KiB 时改 JPEG 逐级降质
 * (云端 413 AVATAR_TOO_LARGE 的界限,见 15 §3.2)。
 */

export const AVATAR_MAX_EDGE = 256;
export const AVATAR_MAX_DATA_URL = 512 * 1024;

const JPEG_QUALITIES = [0.92, 0.85, 0.75, 0.6, 0.45];

function loadImage(file: Blob): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const url = URL.createObjectURL(file);
    const img = new Image();
    img.onload = () => {
      URL.revokeObjectURL(url);
      resolve(img);
    };
    img.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error('无法读取该图片'));
    };
    img.src = url;
  });
}

/** 按最长边等比缩放后的尺寸(不放大)。 */
export function fitWithin(width: number, height: number, maxEdge = AVATAR_MAX_EDGE): { width: number; height: number } {
  const w = Math.max(1, width);
  const h = Math.max(1, height);
  const scale = Math.min(1, maxEdge / Math.max(w, h));
  return { width: Math.max(1, Math.round(w * scale)), height: Math.max(1, Math.round(h * scale)) };
}

export async function resizeAvatar(file: Blob): Promise<string> {
  if (!file.type.startsWith('image/')) throw new Error('请选择图片文件');
  const img = await loadImage(file);
  const { width, height } = fitWithin(img.naturalWidth || img.width, img.naturalHeight || img.height);
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('当前环境不支持图片处理');
  ctx.imageSmoothingQuality = 'high';
  ctx.drawImage(img, 0, 0, width, height);
  const png = canvas.toDataURL('image/png');
  if (png.length <= AVATAR_MAX_DATA_URL) return png;
  // JPEG 无透明通道:先在已有像素之下垫白底
  ctx.globalCompositeOperation = 'destination-over';
  ctx.fillStyle = '#FFFFFF';
  ctx.fillRect(0, 0, width, height);
  for (const q of JPEG_QUALITIES) {
    const jpeg = canvas.toDataURL('image/jpeg', q);
    if (jpeg.length <= AVATAR_MAX_DATA_URL) return jpeg;
  }
  throw new Error('图片压缩后仍超过 512 KiB,请换一张');
}
