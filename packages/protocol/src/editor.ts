/** References are identities, never executable instructions. Payloads are resolved by the workspace host. */
export type EditorReferenceKind = 'scene' | 'entity' | 'component' | 'property' | 'asset' | 'blueprint' | 'studio' | 'logicGraph' | 'shaderGraph' | 'source' | 'viewport';
export interface EditorSelection {
  /** Quoted source data, never an instruction; supplied for unsaved editor selections. */
  excerpt?: string;
  dirty?: boolean;
  nodeIds?: string[];
  edgeIds?: string[];
  featureIds?: string[];
  entityIds?: number[];
  component?: string;
  property?: string;
  pin?: string;
  versionId?: string;
  /** Source offsets are UTF-16, line numbers are one-based. */
  range?: { from: number; to: number; startLine: number; endLine: number };
  /** Normalized image coordinates, captured against an immutable observation. */
  region?: { x: number; y: number; width: number; height: number };
}
export interface EditorReference {
  workspaceId: string;
  kind: EditorReferenceKind;
  sceneGuid?: string;
  entityGuid?: string;
  identityPersisted?: boolean;
  entityIdentityPersisted?: boolean;
  entityId?: number;
  resourceId?: string;
  path?: string;
  selection?: EditorSelection;
  revision?: number | string;
  hostEpoch?: string;
  targetMode?: 'edit' | 'runtime';
}
export interface EditorAnnotation {
  id: string;
  reference: EditorReference;
  note?: string;
  label?: string;
  observationId?: string;
}
export interface EditorDocument<T = unknown> { document: T; revision: number }
export type EditorDocumentKind = 'blueprint' | 'studio' | 'shaderGraph' | 'logicGraph';
export interface EditorResolution {
  reference: EditorReference;
  status?: 'resolved' | 'stale' | 'missing';
  label?: string;
  message?: string;
  [key: string]: unknown;
}
export interface EditorObservation {
  staleForEditing?: boolean;
  observationId: string;
  reference?: EditorReference;
  imageUrl?: string;
  width?: number;
  height?: number;
}
export const EDITOR_REFERENCE_MIME = 'application/x-forge-editor-annotations';
