import {ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {persistenceError, type PersistenceBridge, type PersistenceError} from "./useUiPreferences.ts";
import type {VideoPlatform} from "./videoPlatforms.ts";

export interface HistoryCursor {
    startedAt: string;
    id: number
}

export type HistoryStatus = "queued" | "running" | "completed" | "failed" | "cancelled" | "interrupted";

export interface HistoryFilters {
    query: string;
    status: HistoryStatus | null
}

export interface DownloadRecord {
    id: number;
    requestId: string;
    platform: VideoPlatform;
    videoId: string;
    sourceLink: string;
    title: string;
    thumbnailUrl: string | null;
    thumbnailCachePath: string | null;
    durationSeconds: number | null;
    formatId: string;
    formatExtension: string | null;
    height: number | null;
    fps: number | null;
    selectedSizeBytes: number | null;
    sizeApproximate: boolean;
    cookieFallback: boolean;
    downloadDirectory: string;
    outputPath: string | null;
    outputExtension: string | null;
    fileSizeBytes: number | null;
    fileAvailability?: "unknown" | "present" | "missing";
    status: HistoryStatus;
    errorCode: string | null;
    errorDetail: string | null;
    errorStage?: string | null;
    failureKind?: string | null;
    startedAt: string;
    finishedAt: string | null;
    updatedAt: string;
    deletedAt?: string | null;
    fileDeletedAt?: string | null;
}

export interface HistoryResponse {
    records: DownloadRecord[];
    nextCursor: HistoryCursor | null;
    totalCount: number;
    matchedCount?: number;
    trashCount?: number;
    fileRecyclingSupported?: boolean;
    fileDeletionSupported?: boolean;
    statusCounts?: Record<HistoryStatus, number>
}

interface HistoryBridge extends PersistenceBridge {
    listen: (event: string, handler: () => void) => Promise<() => void>
}

export function createDownloadHistory(bridge: HistoryBridge) {
    const records = ref<DownloadRecord[]>([]), totalCount = ref<number | null>(null),
        nextCursor = ref<HistoryCursor | null>(null);
    const loading = ref(false), error = ref<PersistenceError | null>(null);
    const connectionError = ref<PersistenceError | null>(null);
    const matchedCount = ref<number | null>(null), statusCounts = ref<Record<HistoryStatus, number>>({
        queued: 0,
        running: 0,
        completed: 0,
        failed: 0,
        cancelled: 0,
        interrupted: 0
    });
    const filters = ref<HistoryFilters>({query: "", status: null});
    const trashed = ref(false), trashCount = ref(0);
    const fileRecyclingSupported = ref(false), fileDeletionSupported = ref(false);
    let disposed = false, revision = 0, task: Promise<boolean> | undefined, unlisten: (() => void) | undefined,
        connecting: Promise<void> | undefined;
    let active: { revision: number; refreshPending: boolean } | undefined;
    let historyStatus: HistoryStatus | null = null;

    async function query(more: boolean, current: number) {
        try {
            const response: HistoryResponse = bridge.desktop ? await bridge.invoke<HistoryResponse>("list_download_records", {
                cursor: more ? nextCursor.value : null,
                limit: 50, ...filters.value, ...(trashed.value ? {trashed: true} : {})
            }) : {records: [], nextCursor: null, totalCount: 0, matchedCount: 0, trashCount: 0};
            if (disposed || current !== revision) return false;
            records.value = more ? [...records.value, ...response.records.filter(row => !records.value.some(old => old.id === row.id))] : response.records;
            totalCount.value = response.totalCount;
            matchedCount.value = response.matchedCount ?? response.totalCount;
            trashCount.value = response.trashCount ?? 0;
            fileRecyclingSupported.value = response.fileRecyclingSupported === true;
            fileDeletionSupported.value = response.fileDeletionSupported === true;
            if (response.statusCounts) statusCounts.value = response.statusCounts;
            nextCursor.value = response.nextCursor;
            error.value = null;
            return true;
        } catch (e) {
            if (!disposed && current === revision) error.value = persistenceError(e);
            return false;
        }
    }

    function load(more = false): Promise<boolean> {
        if (disposed) return Promise.resolve(false);
        if (task && active?.revision === revision) {
            if (!more) active.refreshPending = true;
            return task;
        }
        if (more && !nextCursor.value) return Promise.resolve(true);
        const state = {revision, refreshPending: false};
        active = state;
        loading.value = true;
        task = (async () => {
            let result = await query(more, state.revision);
            while (state.refreshPending && !disposed && state.revision === revision) {
                state.refreshPending = false;
                result = await query(false, state.revision);
            }
            return result;
        })().finally(() => {
            if (active === state) {
                task = undefined;
                active = undefined;
                loading.value = false;
            }
        });
        return task;
    }

    function setFilters(patch: Partial<HistoryFilters>) {
        const next = {...filters.value, ...patch};
        next.query = next.query.trim();
        if (next.query === filters.value.query && next.status === filters.value.status) return load();
        filters.value = next;
        revision++;
        nextCursor.value = null;
        return load();
    }

    function setTrashed(value: boolean) {
        if (value === trashed.value) return load();
        if (value) historyStatus = filters.value.status;
        filters.value = {...filters.value, status: value ? null : historyStatus};
        trashed.value = value;
        revision++;
        nextCursor.value = null;
        records.value = [];
        totalCount.value = null;
        matchedCount.value = null;
        error.value = null;
        statusCounts.value = {queued: 0, running: 0, completed: 0, failed: 0, cancelled: 0, interrupted: 0};
        return load();
    }

    function connect(): Promise<void> {
        if (disposed || unlisten || !bridge.desktop) return Promise.resolve();
        return connecting ??= (async () => {
            try {
                const remove = await bridge.listen("download-records-changed", () => {
                    void load();
                });
                if (disposed) remove(); else {
                    unlisten = remove;
                    connectionError.value = null;
                }
            } catch (e) {
                if (!disposed) connectionError.value = persistenceError(e);
            } finally {
                connecting = undefined;
            }
        })();
    }

    function dispose() {
        disposed = true;
        revision++;
        if (active) active.refreshPending = false;
        loading.value = false;
        unlisten?.();
        unlisten = undefined;
    }

    async function whenIdle() {
        await task;
    }

    async function refresh() {
        await connect();
        return load();
    }

    return {
        records,
        totalCount,
        matchedCount,
        statusCounts,
        filters,
        trashed,
        trashCount,
        fileRecyclingSupported,
        fileDeletionSupported,
        nextCursor,
        loading,
        error,
        connectionError,
        setFilters,
        setTrashed,
        load: () => load(),
        loadMore: () => load(true),
        connect,
        refresh,
        dispose,
        whenIdle
    };
}

let controller: ReturnType<typeof createDownloadHistory> | undefined;

export function useDownloadHistory() {
    return controller ??= createDownloadHistory({desktop: isTauri(), invoke, listen});
}
