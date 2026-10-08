/** Shared Blender authoring, publishing and template contracts. Paths are project relative. */
export type BlenderAssetKind = 'prop' | 'map' | 'character';
export type BlenderJobState =
  | 'awaiting_codex' | 'claimed' | 'authoring' | 'exporting' | 'validating'
  | 'importing' | 'ready' | 'failed' | 'cancelled';

export interface BlenderPublishedModel {
  modelGuid: string;
  modelPath: string;
  prefabGuid: string;
  prefabPath: string;
  revision: number;
  clips?: string[];
}

export interface BlenderDiagnostic { code: string; message: string }

export interface BlenderJob {
  id: string;
  sourceId: string;
  workspaceId: string;
  name: string;
  prompt: string;
  kind: BlenderAssetKind;
  state: BlenderJobState;
  stage?: string;
  message?: string;
  sourcePath: string;
  sourceAbsolutePath?: string;
  sourceBound: boolean;
  autoSync: boolean;
  revision: number;
  executorId?: string | null;
  leaseExpiresAt?: number | null;
  createdAt: number;
  updatedAt: number;
  codexUrl: string;
  handoffPrompt: string;
  published?: BlenderPublishedModel | null;
  error?: BlenderDiagnostic | null;
  reload?: unknown;
}

export interface BlenderStatus {
  blender: { found: boolean; executablePath?: string | null; version?: string };
  computerUse: { status: 'unknown' | 'verified'; evidence?: string };
  setup?: { mcpConfigured: boolean; skillPath?: string; addonPath?: string; addonEnabled?: string };
}

export interface BlenderCreateJob {
  workspaceId: string;
  name: string;
  prompt: string;
  kind: BlenderAssetKind;
}

export interface BlenderClaimJob {
  workspaceId: string;
  executorId: string;
  capabilities: { computerUse: true };
}

export interface BlenderSourceBinding {
  workspaceId: string;
  leaseToken: string;
  sourcePath: string;
  autoSync?: boolean;
}

export interface BlenderExportManifest {
  version: number;
  sourceId: string;
  name: string;
  kind: BlenderAssetKind;
  sourceBlend?: string;
  revision: number;
  objectIds: Record<string, string>;
  idleClip?: string;
  walkClip?: string;
}

export interface BlenderAssetUpdated {
  jobId: string;
  workspaceId: string;
  revision: number;
  published: BlenderPublishedModel;
}
