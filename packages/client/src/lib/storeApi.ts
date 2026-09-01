/**
 * F11 wave.4:资产商店 REST 面封装(契约见 11_API_CONTRACTS.md §2.7)。
 *
 * 独立于 forgeApi 的 fetch 封装(只复用 ForgeApiError,保持 .code/.status 提升方式一致):
 * 商店面全部走 plain JSON 的 /api/forge/store/*,与 MCP 信封无关。
 * 后端 wave.3 并行落地中,本文件按冻结契约写,不做任何形态猜测式兜底——
 * 字段缺失时如实呈现为空,错误码原样上抛给 UI。
 */

import { ForgeApiError } from './forgeApi';

// ---------- DTO(与 Rust 侧 serde camelCase 对齐) ----------

export type PackageKind = 'asset-pack' | 'skill';

export interface PackageLicense {
  id: string;
  url?: string;
}

export interface PackagePricing {
  amount: number;
  currency: string;
}

export interface PackagePublisher {
  id: string;
  name: string;
  url?: string;
}

export interface PackageSummary {
  id: string;
  name: string;
  kind: PackageKind;
  description: string;
  latestVersion: string;
  tags: string[];
  license?: PackageLicense | null;
  pricing?: PackagePricing | null;
  publisher?: PackagePublisher | null;
  thumbnail?: string | null;
}

export interface PackageFile {
  path: string;
  sha256: string;
  size: number;
}

export interface PackageDependency {
  id: string;
  version: string;
}

export interface PackagePreview {
  thumbnail?: string | null;
  screenshots: string[];
}

export interface PackageManifest {
  id: string;
  name: string;
  kind: PackageKind;
  description: string;
  version: string;
  tags: string[];
  license?: PackageLicense | null;
  pricing?: PackagePricing | null;
  publisher?: PackagePublisher | null;
  engineVersion?: string | null;
  dependencies: PackageDependency[];
  files: PackageFile[];
  preview?: PackagePreview | null;
  createdAt: string;
  updatedAt: string;
}

/** 源清单条目;token 只以 hasToken 布尔露面,响应面永不含明文(R-5)。 */
export interface StoreSource {
  id: string;
  name: string;
  baseUrl: string;
  enabled: boolean;
  hasToken: boolean;
}

/** 搜索命中(包 + 它来自哪个源)。 */
export interface SearchHit {
  sourceId: string;
  sourceName: string;
  package: PackageSummary;
}

/** 单源失败如实上报(不可达的源不静默跳过)。 */
export interface SourceError {
  sourceId: string;
  code: string;
  message: string;
}

export interface SearchResponse {
  total: number;
  page: number;
  pageSize: number;
  items: SearchHit[];
  errors: SourceError[];
}

export interface PackageDetail {
  sourceId: string;
  summary: PackageSummary;
  versions: string[];
}

export interface VersionDetail {
  sourceId: string;
  manifest: PackageManifest;
}

export interface InstallRecord {
  sourceId: string;
  packageId: string;
  version: string;
  kind: string;
  assetPaths: string[];
  skillNames: string[];
  installedAt: string;
}

export type StoreTaskStatus = 'running' | 'completed' | 'failed';

export interface StoreTask {
  taskId: string;
  status: StoreTaskStatus;
  phase: string;
  done: number;
  total: number;
  error?: { code: string; message: string } | null;
  record?: InstallRecord | null;
}

export interface UpdateInfo {
  sourceId: string;
  packageId: string;
  current: string;
  latest: string;
  hasUpdate: boolean;
}

export interface LibraryItem {
  id: string;
  name: string;
  sha256: string;
  size: number;
  ext: string;
  kind: string;
  tags: string[];
  source: string;
  addedAt: string;
}

/**
 * 409 GOV_PROPOSAL_REQUIRED 专用:除 code/message 外还带回 proposalId,
 * 供 UI 就地展开「在此批准」而无须再查提案列表。
 */
export class StoreProposalRequiredError extends ForgeApiError {
  proposalId: string;
  constructor(message: string, proposalId: string, status = 409) {
    super('GOV_PROPOSAL_REQUIRED', message, status);
    this.name = 'StoreProposalRequiredError';
    this.proposalId = proposalId;
  }
}

// ---------- fetch 底座 ----------

interface ErrorBody {
  error?: { code?: string; message?: string; proposalId?: string };
}

/**
 * 统一请求:非 2xx → ForgeApiError(code 取 {error.code},退 HTTP_{status});
 * 带 proposalId 的 409 → StoreProposalRequiredError;空响应体容忍(DELETE)。
 */
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  try {
    res = await fetch(path, init);
  } catch (err) {
    throw new ForgeApiError('NETWORK', `请求失败:${(err as Error).message}`);
  }

  const text = await res.text();
  let body: unknown = null;
  if (text !== '') {
    try {
      body = JSON.parse(text);
    } catch {
      throw new ForgeApiError('BAD_RESPONSE', `响应非 JSON(HTTP ${res.status})`, res.status);
    }
  }

  if (!res.ok) {
    const err = (body as ErrorBody | null)?.error;
    const code = err?.code ?? `HTTP_${res.status}`;
    const message = err?.message ?? `HTTP ${res.status}`;
    if (code === 'GOV_PROPOSAL_REQUIRED' && err?.proposalId) {
      throw new StoreProposalRequiredError(message, err.proposalId, res.status);
    }
    throw new ForgeApiError(code, message, res.status);
  }
  return body as T;
}

function jsonInit(method: string, payload: unknown): RequestInit {
  return {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  };
}

const seg = (s: string) => encodeURIComponent(s);

// ---------- 源管理 ----------

export function storeListSources(): Promise<{ sources: StoreSource[] }> {
  return request('/api/forge/store/sources');
}

/** token 只单向送出,响应面不回;调用方不得把入参 token 留在任何可渲染状态里。 */
export function storeAddSource(payload: {
  id: string;
  name: string;
  baseUrl: string;
  token?: string;
}): Promise<{ source?: StoreSource }> {
  return request('/api/forge/store/sources', jsonInit('POST', payload));
}

export function storePatchSource(
  id: string,
  patch: { name?: string; enabled?: boolean; token?: string },
): Promise<{ source?: StoreSource }> {
  return request(`/api/forge/store/sources/${seg(id)}`, jsonInit('PATCH', patch));
}

export function storeDeleteSource(id: string): Promise<unknown> {
  return request(`/api/forge/store/sources/${seg(id)}`, { method: 'DELETE' });
}

// ---------- 发现 ----------

export function storeSearch(params: {
  q?: string;
  kind?: PackageKind;
  sourceId?: string;
  page?: number;
  pageSize?: number;
}): Promise<SearchResponse> {
  const qs = new URLSearchParams();
  if (params.q) qs.set('q', params.q);
  if (params.kind) qs.set('kind', params.kind);
  if (params.sourceId) qs.set('sourceId', params.sourceId);
  if (params.page !== undefined) qs.set('page', String(params.page));
  if (params.pageSize !== undefined) qs.set('pageSize', String(params.pageSize));
  const suffix = qs.toString();
  return request(`/api/forge/store/search${suffix === '' ? '' : `?${suffix}`}`);
}

export function storeGetPackage(sourceId: string, pkgId: string): Promise<PackageDetail> {
  return request(`/api/forge/store/packages/${seg(sourceId)}/${seg(pkgId)}`);
}

export function storeGetVersion(
  sourceId: string,
  pkgId: string,
  version: string,
): Promise<VersionDetail> {
  return request(`/api/forge/store/packages/${seg(sourceId)}/${seg(pkgId)}/${seg(version)}`);
}

// ---------- 安装 / 卸载 / 长任务 ----------

export function storeInstall(payload: {
  sourceId: string;
  pkgId: string;
  version: string;
  destFolder?: string;
}): Promise<{ taskId: string }> {
  return request('/api/forge/store/install', jsonInit('POST', payload));
}

export function storeGetTask(taskId: string): Promise<StoreTask> {
  return request(`/api/forge/store/tasks/${seg(taskId)}`);
}

/** 卸载是 destructive:未批准提案时抛 StoreProposalRequiredError(带 proposalId)。 */
export function storeUninstall(payload: {
  sourceId: string;
  pkgId: string;
}): Promise<{ taskId: string }> {
  return request('/api/forge/store/uninstall', jsonInit('POST', payload));
}

export function storeListInstalled(): Promise<{ installed: InstallRecord[] }> {
  return request('/api/forge/store/installed');
}

export function storeCheckUpdates(): Promise<{ updates: UpdateInfo[] }> {
  return request('/api/forge/store/updates');
}

// ---------- 个人资产库 ----------

export function storeListLibrary(): Promise<{ items: LibraryItem[] }> {
  return request('/api/forge/store/library');
}

export function storeAddLibrary(assetPath: string): Promise<{ item: LibraryItem }> {
  return request('/api/forge/store/library', jsonInit('POST', { assetPath }));
}

export function storeRemoveLibrary(id: string): Promise<{ removed: boolean }> {
  return request(`/api/forge/store/library/${seg(id)}`, { method: 'DELETE' });
}

export function storeInstallLibraryItem(
  id: string,
  destFolder?: string,
): Promise<{ assetPath: string }> {
  return request(
    `/api/forge/store/library/${seg(id)}:install`,
    jsonInit('POST', destFolder ? { destFolder } : {}),
  );
}

// ---------- 治理 ----------

/** 卸载提案批准(与 ProposalsTab 同一端点;批准后调用方自行重发卸载)。 */
export function storeApproveProposal(id: string): Promise<unknown> {
  return request(`/api/forge/proposals/${seg(id)}`, jsonInit('PATCH', { action: 'approve' }));
}
