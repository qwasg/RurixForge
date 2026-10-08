export type ShaderDomain = 'sprite2d' | 'pbr3d' | 'unlit3d';
export type ShaderValueType = 'float' | 'vec2' | 'vec3' | 'vec4' | 'color' | 'texture2d';
export type ShaderValueSource = { node: string; pin: string } | { const: unknown } | { param: string };
export interface ShaderParameter { id: string; name: string; type: ShaderValueType; default: unknown }
export interface ShaderNode { id: string; type: string; pos: [number, number]; inputs: Record<string, ShaderValueSource>; options: Record<string, unknown> }
export interface ShaderGraphDoc { version: 1; id: string; name: string; domain: ShaderDomain; parameters: ShaderParameter[]; nodes: ShaderNode[]; outputs: Record<string, ShaderValueSource> }
export interface ShaderDiagnostic { message: string; nodeId?: string; pin?: string; code?: string; severity?: string; backend?: string; stage?: string; line?: number }
export interface ShaderSourceSpan { nodeId: string; line: number }
export interface ShaderCompilation { ok?: boolean; hash?: string; domain?: ShaderDomain; wgsl?: Record<string, string>; godot?: Record<string, string>; diagnostics?: ShaderDiagnostic[]; sourceMap?: Record<string, ShaderSourceSpan[]>; parameters?: unknown; textureSlots?: unknown; compiled?: Omit<ShaderCompilation, 'compiled'>; validation?: Record<string,string> }
