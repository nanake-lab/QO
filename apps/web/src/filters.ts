import type { TaskFilters } from "./state.js";

export const TASK_FILTERS_STORAGE_KEY = "qo.web.taskFilters.v1";

export const DEFAULT_TASK_FILTERS: TaskFilters = {
  hideClosed: false,
  onlyMine: false,
};

type StorageReader = Pick<Storage, "getItem">;
type StorageWriter = Pick<Storage, "setItem">;

function defaultStorage(): Storage | null {
  try {
    return typeof localStorage !== "undefined" ? localStorage : null;
  } catch {
    return null;
  }
}

export function loadTaskFilters(storage?: StorageReader | null): TaskFilters {
  try {
    const target = storage === undefined ? defaultStorage() : storage;
    if (!target) return DEFAULT_TASK_FILTERS;
    const raw = target.getItem(TASK_FILTERS_STORAGE_KEY);
    if (!raw) return DEFAULT_TASK_FILTERS;
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") return DEFAULT_TASK_FILTERS;
    const record = parsed as Record<string, unknown>;
    return {
      hideClosed: record.hideClosed === true,
      onlyMine: record.onlyMine === true,
    };
  } catch {
    return DEFAULT_TASK_FILTERS;
  }
}

export function saveTaskFilters(
  filters: TaskFilters,
  storage?: StorageWriter | null,
): void {
  try {
    const target = storage === undefined ? defaultStorage() : storage;
    if (!target) return;
    target.setItem(TASK_FILTERS_STORAGE_KEY, JSON.stringify(filters));
  } catch {
    // 隐身模式或禁用存储时忽略持久化失败，保留本次会话内的内存筛选。
  }
}
