import { adminApi } from './api/admin';
import type { AdminModel, Group, Plan } from './api/types';
import { useAsync } from './hooks';

/** 下拉/多选用的引用数据（分组、套餐、模型都不分页）。 */
export function useGroups() {
  return useAsync<Group[]>(() => adminApi.groups.list().then((r) => r.items ?? []), []);
}

export function usePlans() {
  return useAsync<Plan[]>(() => adminApi.plans.list().then((r) => r.items ?? []), []);
}

export function useModels() {
  return useAsync<AdminModel[]>(() => adminApi.models.list().then((r) => r.items ?? []), []);
}

export function groupNameMap(groups: Group[] | undefined): Map<number, string> {
  return new Map((groups ?? []).map((g) => [g.id, g.name]));
}
