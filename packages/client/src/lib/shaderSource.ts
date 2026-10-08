import type { ShaderCompilation, ShaderDomain, ShaderSourceSpan } from '@forge/protocol';

/** Keep emitted line numbers intact so diagnostics and node markers address the same source. */
export function shaderGeneratedSource(compilation: ShaderCompilation | null, domain: ShaderDomain, backend: 'wgsl' | 'godot') {
  const compiled = compilation?.compiled ?? compilation;
  const sprite = (compiled?.domain ?? domain) === 'sprite2d';
  const stage = backend === 'wgsl' ? (sprite ? 'sprite' : 'model') : (sprite ? 'canvas' : 'spatial');
  return { code: compiled?.[backend]?.[stage] ?? '', spans: compiled?.sourceMap?.[`${backend}.${stage}`] ?? [] };
}

export function shaderNodeAtLine(spans: ShaderSourceSpan[], line: number): string | undefined {
  // Each marker describes the following expression line. Helper/output boilerplate
  // is deliberately not attributed to the last graph node.
  return spans.find((span) => line === span.line || line === span.line + 1)?.nodeId;
}
