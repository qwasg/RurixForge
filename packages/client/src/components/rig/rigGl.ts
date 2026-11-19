import type { PartBlend, Vec2 } from '@/lib/rigTypes';

/**
 * 绑骨编辑器 WebGL2 渲染器(F-GAME-5 W5):把每部件的显示顶点(画布像素)展开成纹理三角形,
 * 一个动态 VBO(x, y, u, v)整帧重填,按 drawOrder 逐部件 drawArrays(逐部件 opacity / 混合模式)。
 * UV 为图集像素,CPU 端除以图集宽高归一化。WebGL2 不可用(jsdom / 旧驱动)时 create 返回 null,
 * 调用方降级为 2D 线框。
 */

export interface GlPart {
  positions: Vec2[];
  uvs: Vec2[];
  triangles: [number, number, number][];
  opacity: number;
  blend: PartBlend;
}

export interface GlView {
  scale: number;
  ox: number;
  oy: number;
}

const VS = `#version 300 es
in vec2 a_pos;
in vec2 a_uv;
uniform float u_scale;
uniform vec2 u_offset;
uniform vec2 u_size;
out vec2 v_uv;
void main() {
  vec2 s = a_pos * u_scale + u_offset;
  vec2 c = s / u_size * 2.0 - 1.0;
  gl_Position = vec4(c.x, -c.y, 0.0, 1.0);
  v_uv = a_uv;
}`;

const FS = `#version 300 es
precision mediump float;
in vec2 v_uv;
uniform sampler2D u_tex;
uniform int u_hasTex;
uniform float u_alpha;
uniform vec4 u_color;
out vec4 o;
void main() {
  if (u_hasTex == 1) {
    vec4 c = texture(u_tex, v_uv);
    o = vec4(c.rgb, c.a * u_alpha);
  } else {
    o = vec4(u_color.rgb, u_color.a * u_alpha);
  }
}`;

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader {
  const sh = gl.createShader(type);
  if (!sh) throw new Error('createShader 失败');
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(sh);
    gl.deleteShader(sh);
    throw new Error(`着色器编译失败:${log ?? ''}`);
  }
  return sh;
}

export class RigGlRenderer {
  private gl: WebGL2RenderingContext;
  private prog: WebGLProgram;
  private vao: WebGLVertexArrayObject;
  private vbo: WebGLBuffer;
  private tex: WebGLTexture | null = null;
  private texW = 0;
  private texH = 0;
  private cap = 0;
  private u: Record<string, WebGLUniformLocation | null>;

  private constructor(gl: WebGL2RenderingContext) {
    this.gl = gl;
    const prog = gl.createProgram();
    if (!prog) throw new Error('createProgram 失败');
    gl.attachShader(prog, compile(gl, gl.VERTEX_SHADER, VS));
    gl.attachShader(prog, compile(gl, gl.FRAGMENT_SHADER, FS));
    gl.bindAttribLocation(prog, 0, 'a_pos');
    gl.bindAttribLocation(prog, 1, 'a_uv');
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      throw new Error(`着色器链接失败:${gl.getProgramInfoLog(prog) ?? ''}`);
    }
    this.prog = prog;
    const vao = gl.createVertexArray();
    const vbo = gl.createBuffer();
    if (!vao || !vbo) throw new Error('缓冲创建失败');
    this.vao = vao;
    this.vbo = vbo;
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 16, 8);
    gl.bindVertexArray(null);
    this.u = Object.fromEntries(
      ['u_scale', 'u_offset', 'u_size', 'u_tex', 'u_hasTex', 'u_alpha', 'u_color'].map((n) => [
        n,
        gl.getUniformLocation(prog, n),
      ]),
    );
  }

  /** WebGL2 不可用或初始化失败 → null(调用方降级)。 */
  static create(canvas: HTMLCanvasElement): RigGlRenderer | null {
    try {
      const gl = canvas.getContext('webgl2', { premultipliedAlpha: false, antialias: true });
      if (!gl || typeof (gl as WebGL2RenderingContext).createVertexArray !== 'function') return null;
      return new RigGlRenderer(gl as WebGL2RenderingContext);
    } catch {
      return null;
    }
  }

  setTexture(img: HTMLImageElement | null): void {
    const gl = this.gl;
    if (this.tex) {
      gl.deleteTexture(this.tex);
      this.tex = null;
    }
    if (!img) return;
    const tex = gl.createTexture();
    if (!tex) return;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_PREMULTIPLY_ALPHA_WEBGL, false);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    this.tex = tex;
    this.texW = img.naturalWidth;
    this.texH = img.naturalHeight;
  }

  private setBlend(b: PartBlend): void {
    const gl = this.gl;
    if (b === 'additive') gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE, gl.ONE, gl.ONE);
    else if (b === 'multiply') gl.blendFuncSeparate(gl.DST_COLOR, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    else gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  }

  /**
   * 整帧绘制:清屏 → 画布底板(canvasSize 矩形)→ 各部件(调用方已按 drawOrder 升序排好、剔除隐藏)。
   * size 为 CSS 像素,dpr 由 canvas 尺寸体现。
   */
  draw(parts: GlPart[], canvasSize: Vec2, view: GlView, size: { w: number; h: number; dpr: number }): void {
    const gl = this.gl;
    const cw = Math.round(size.w * size.dpr);
    const ch = Math.round(size.h * size.dpr);
    if (gl.canvas.width !== cw) gl.canvas.width = cw;
    if (gl.canvas.height !== ch) gl.canvas.height = ch;
    gl.viewport(0, 0, cw, ch);
    gl.clearColor(0.09, 0.09, 0.11, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    // 顶点数据:底板 6 顶点 + 各部件展开三角形。
    let total = 6;
    for (const p of parts) total += p.triangles.length * 3;
    const data = new Float32Array(total * 4);
    const [W, H] = canvasSize;
    const bg = [0, 0, W, 0, W, H, 0, 0, W, H, 0, H];
    for (let i = 0; i < 6; i += 1) {
      data[i * 4] = bg[i * 2];
      data[i * 4 + 1] = bg[i * 2 + 1];
    }
    const tw = this.texW || 1;
    const th = this.texH || 1;
    let o = 24;
    const ranges: Array<{ first: number; count: number; part: GlPart }> = [];
    let first = 6;
    for (const p of parts) {
      for (const tri of p.triangles) {
        for (const vi of tri) {
          const pos = p.positions[vi] ?? [0, 0];
          const uv = p.uvs[vi] ?? [0, 0];
          data[o] = pos[0];
          data[o + 1] = pos[1];
          data[o + 2] = uv[0] / tw;
          data[o + 3] = uv[1] / th;
          o += 4;
        }
      }
      const count = p.triangles.length * 3;
      ranges.push({ first, count, part: p });
      first += count;
    }

    gl.useProgram(this.prog);
    gl.bindVertexArray(this.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.vbo);
    if (data.byteLength > this.cap) {
      this.cap = Math.max(data.byteLength, this.cap * 2, 4096);
      gl.bufferData(gl.ARRAY_BUFFER, this.cap, gl.DYNAMIC_DRAW);
    }
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, data);
    gl.uniform1f(this.u.u_scale, view.scale);
    gl.uniform2f(this.u.u_offset, view.ox, view.oy);
    gl.uniform2f(this.u.u_size, size.w, size.h);
    gl.uniform1i(this.u.u_tex, 0);
    gl.enable(gl.BLEND);

    // 底板
    this.setBlend('alpha');
    gl.uniform1i(this.u.u_hasTex, 0);
    gl.uniform1f(this.u.u_alpha, 1);
    gl.uniform4f(this.u.u_color, 0.15, 0.15, 0.18, 1);
    gl.drawArrays(gl.TRIANGLES, 0, 6);

    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, this.tex);
    gl.uniform1i(this.u.u_hasTex, this.tex ? 1 : 0);
    gl.uniform4f(this.u.u_color, 0.55, 0.57, 0.62, 0.55);
    for (const r of ranges) {
      if (r.count === 0) continue;
      this.setBlend(r.part.blend);
      gl.uniform1f(this.u.u_alpha, Math.max(0, Math.min(1, r.part.opacity)));
      gl.drawArrays(gl.TRIANGLES, r.first, r.count);
    }
    gl.bindVertexArray(null);
  }

  dispose(): void {
    const gl = this.gl;
    if (this.tex) gl.deleteTexture(this.tex);
    gl.deleteBuffer(this.vbo);
    gl.deleteVertexArray(this.vao);
    gl.deleteProgram(this.prog);
  }
}
