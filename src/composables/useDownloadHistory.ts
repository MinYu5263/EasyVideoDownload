import {computed, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {listen} from "@tauri-apps/api/event";
import {persistenceError, type PersistenceBridge, type PersistenceError} from "./useUiPreferences.ts";
import type {VideoPlatform} from "./videoPlatforms.ts";
import type {VideoFormat} from "./useVideoParser.ts";

export interface HistoryCursor {
    startedAt: string;
    id: number
}

export type HistoryStatus = "queued" | "running" | "paused" | "completed" | "failed" | "cancelled" | "interrupted";

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
    formatSnapshot?: VideoFormat | null;
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
    successfulOutput?: {
        formatSnapshot?: VideoFormat | null;
        formatId: string;
        formatExtension: string | null;
        height: number | null;
        fps: number | null;
        directory: string;
        finishedAt: string
    } | null;
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
    const viewLoaded = ref(false);
    const blockingLoading = computed(() => loading.value && !viewLoaded.value);
    const connectionError = ref<PersistenceError | null>(null);
    const matchedCount = ref<number | null>(null), statusCounts = ref<Record<HistoryStatus, number>>({
        queued: 0,
        running: 0,
        paused: 0,
        completed: 0,
        failed: 0,
        cancelled: 0,
        interrupted: 0
    });
    const filters = ref<HistoryFilters>({query: "", status: null});
    const trashed = ref(false), trashCount = ref(0);
    const switching = ref(false);
    let requestedFilters: HistoryFilters = {...filters.value};
    let requestedTrashed = false;
    const fileDeletionSupported = ref(false);
    let disposed = false, revision = 0, task: Promise<boolean> | undefined, unlisten: (() => void) | undefined,
        connecting: Promise<void> | undefined;
    let active: { revision: number; refreshPending: boolean } | undefined;
    let historyStatus: HistoryStatus | null = null;
    let loadedThrough: HistoryCursor | null = null;

    async function query(more: boolean, current: number) {
        try {
            const queryFilters = {...requestedFilters}, queryTrashed = requestedTrashed;
            const fetchPage = (cursor: HistoryCursor | null): Promise<HistoryResponse> => bridge.desktop ? bridge.invoke<HistoryResponse>("list_download_records", {
                cursor,
                limit: 50, ...queryFilters, ...(queryTrashed ? {trashed: true} : {})
            }) : Promise.resolve({records: [], nextCursor: null, totalCount: 0, matchedCount: 0, trashCount: 0});
            const boundary = more ? null : loadedThrough;
            let response = await fetchPage(more ? nextCursor.value : null);
            if (disposed || current !== revision) return false;
            const refreshed = new Map(response.records.map(row => [row.id, row]));
            const visited = new Set<string>();
            // Keep the visible range intact when native task updates refresh a paginated list.
            // Publish only after the entire range is loaded, so the focused card stays mounted.
            while (boundary && response.nextCursor && (response.nextCursor.startedAt > boundary.startedAt ||
                (response.nextCursor.startedAt === boundary.startedAt && response.nextCursor.id > boundary.id))) {
                const cursor = JSON.stringify(response.nextCursor);
                if (visited.has(cursor)) throw {code: 'loadFailed', detail: 'History pagination did not advance'};
                visited.add(cursor);
                response = await fetchPage(response.nextCursor);
                if (disposed || current !== revision) return false;
                for (const row of response.records) refreshed.set(row.id, row);
            }
            if (disposed || current !== revision) return false;
            // Commit the destination and its data together; never clear the current view first.
            if (filters.value.query !== queryFilters.query || filters.value.status !== queryFilters.status) filters.value = queryFilters;
            trashed.value = queryTrashed;
            records.value = more ? [...records.value, ...response.records.filter(row => !records.value.some(old => old.id === row.id))] : [...refreshed.values()];
            const last = records.value[records.value.length - 1];
            loadedThrough = last ? {id: last.id, startedAt: last.startedAt} : null;
            totalCount.value = response.totalCount;
            matchedCount.value = response.matchedCount ?? response.totalCount;
            trashCount.value = response.trashCount ?? 0;
            fileDeletionSupported.value = response.fileDeletionSupported === true;
            if (response.statusCounts) statusCounts.value = response.statusCounts;
            nextCursor.value = response.nextCursor;
            error.value = null;
            viewLoaded.value = true;
            return true;
        } catch (e) {
            if (!disposed && current === revision) error.value = persistenceError(e);
            return false;
        }
    }

    function load(more = false): Promise<boolean> {
        if (disposed) return Promise.resolve(false);
        const changingView = requestedTrashed !== trashed.value || requestedFilters.query !== filters.value.query || requestedFilters.status !== filters.value.status;
        if (more && changingView) return Promise.resolve(false);
        if (task && active?.revision === revision) {
            if (!more) active.refreshPending = true;
            return task;
        }
        if (more && !nextCursor.value) return Promise.resolve(true);
        const state = {revision, refreshPending: false};
        active = state;
        switching.value = changingView;
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
                switching.value = false;
            }
        });
        return task;
    }

    function setFilters(patch: Partial<HistoryFilters>) {
        // New filters belong to the visible view after a failed scope switch.
        // refresh() still retries the failed destination until a new action intervenes.
        if (!loading.value && requestedTrashed !== trashed.value) {
            requestedTrashed = trashed.value;
            requestedFilters = {...filters.value};
            revision++;
            loadedThrough = null;
        }
        const next = {...requestedFilters, ...patch};
        next.query = next.query.trim();
        if (next.query === requestedFilters.query && next.status === requestedFilters.status) return load();
        requestedFilters = next;
        revision++;
        loadedThrough = null;
        return load();
    }

    function setTrashed(value: boolean) {
        if (value === requestedTrashed) return load();
        if (value) historyStatus = requestedFilters.status;
        requestedFilters = {...requestedFilters, status: value ? null : historyStatus};
        requestedTrashed = value;
        revision++;
        loadedThrough = null;
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
        switching.value = false;
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
        fileDeletionSupported,
        nextCursor,
        loading,
        blockingLoading,
        switching,
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
