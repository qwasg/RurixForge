/**
 * @forge/protocol — WorldBuild wire contracts & state machine definitions.
 * 5-stage lifecycle: survey -> preview_review -> construction -> coherence_verify -> done.
 */

export const WORLD_BUILD_STAGES = [
  'survey',
  'preview_review',
  'construction',
  'coherence_verify',
  'done',
] as const;

export type WorldBuildStage = (typeof WORLD_BUILD_STAGES)[number];

export type WorldBuildPhase = 'waiting' | 'running' | 'failed';

export const WORLD_BUILD_ACTIONS = [
  'survey',
  'rebuild_building',
  'tune_building',
  'adjust_style',
  'approve_preview',
  'resume_construction',
  'fix_coherence',
  'approve_override',
] as const;

export type WorldBuildAction = (typeof WORLD_BUILD_ACTIONS)[number];

export interface WorldBuildError {
  code: string;
  message: string;
}

export interface WorldBuildBuildingTransform {
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
}

export interface WorldBuildBuildingBounds {
  min: [number, number, number];
  max: [number, number, number];
  dimensions?: [number, number, number];
}

export interface WorldBuildBuilding {
  id: string;
  name: string;
  type: string;
  transform: WorldBuildBuildingTransform;
  bounds: WorldBuildBuildingBounds;
  materials: string[];
  interiorPropsCount: number;
}

export interface WorldBuildSceneState {
  slug: string;
  rev: number;
  style: string;
  buildings: WorldBuildBuilding[];
  environment: {
    skybox: string;
    fog: boolean;
    ambientIntensity: number;
  };
}

export interface WorldBuildFacts {
  renderBackend: 'rurix' | 'godot';
  is3d: boolean;
  scenePath: string | null;
  lighting: {
    lightCount: number;
    types: string[];
    ambientColor?: string;
    directionalColor?: string;
  };
  environment: {
    skybox?: string;
    fog?: boolean;
  };
  camera: {
    exists: boolean;
    position?: [number, number, number];
    fov?: number;
  };
  materialsCount: number;
  modelsCount: number;
  artStyle: string;
}

export interface WorldBuildState {
  id: string;
  token: string;
  slug: string;
  dir: string;
  title: string;
  workspaceId: string | null;
  stage: WorldBuildStage;
  phase: WorldBuildPhase;
  running: string | null;
  lastError: WorldBuildError | null;
  previewRev: number;
  selectedBuildingId: string | null;
  approvedAt: string | null;
  renderBackend: 'rurix' | 'godot';
  factsPath: string | null;
  manifestPath: string | null;
  templatePath: string | null;
  scenePath: string | null;
  auditPath: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface WorldBuildActionRequest {
  id: string;
  rev?: number;
  action: WorldBuildAction;
  buildingId?: string;
  instruction?: string;
  stylePreset?: string;
}

/** Verifiable contract exposed by Three.js WebGL preview on demo_host loopback */
export interface BuildPreviewApi {
  getSceneState(): Promise<WorldBuildSceneState> | WorldBuildSceneState;
  selectBuilding(id: string | null): boolean;
  getSelectedBuilding(): WorldBuildBuilding | string | null;
  applyStylePreset(preset: string): boolean;
  getCameraViews(): Array<{
    name: string;
    position: [number, number, number];
    target: [number, number, number];
  }>;
}

export type WindowBuildPreview = BuildPreviewApi;

export function isWorldBuildStage(x: unknown): x is WorldBuildStage {
  return typeof x === 'string' && (WORLD_BUILD_STAGES as readonly string[]).includes(x as WorldBuildStage);
}

export function isWorldBuildAction(x: unknown): x is WorldBuildAction {
  return typeof x === 'string' && (WORLD_BUILD_ACTIONS as readonly string[]).includes(x as WorldBuildAction);
}
