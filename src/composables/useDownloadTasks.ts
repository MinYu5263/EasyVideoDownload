import {computed, ref} from 'vue';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {listen} from '@tauri-apps/api/event';
import type {DownloadRecord} from './useDownloadHistory.ts';
import type {VideoPlatform} from './videoPlatforms.ts';
import {
    taskIsActive,
    type DownloadTaskSnapshot,
    type SubmitDownloadRequest,
    type SubmitDownloadResult
} from './downloadTaskTypes.ts';
import {useCookieSettings} from './useCookieSettings.ts';
import {usePlatformSettings} from './usePlatformSettings.ts';
import {persistenceError} from './useUiPreferences.ts';

export interface DownloadTaskBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    listen: (event: string, handler: (snapshot: DownloadTaskSnapshot) => void) => Promise<() => void>;
    beforeRedownload?: (platform: VideoPlatform) => Promise<void>
}

export function createDownloadTasks(bridge: DownloadTaskBridge) {
    const tasks = ref<Record<number, DownloadTaskSnapshot>>({}),
        error = ref<{ code: string; detail?: string } | null>(null);
    const knownRecords = ref<Record<string, DownloadRecord>>({}), recordsRevision = ref(0);
    const formatRecords = ref<Record<string, DownloadRecord>>({});
    const formatKey = (record: Pick<DownloadRecord, 'platform' | 'videoId' | 'formatId'>) => JSON.stringify([record.platform, record.videoId, record.formatId]);
    const lookups = new Map<string, number>();
    const formatLookups = new Map<string, number>();
    const redownloadErrors = ref<Record<number, { code: string; detail?: string }>>({}),
        submitting = ref<Record<number, boolean>>({});
    const restarts = new Map<number, Promise<SubmitDownloadResult>>();
    const removedRequests = new Set<string>();
    const supersededRequests = new Set<string>();
    const cancellations = new Map<string, Promise<boolean>>();
    const busy = computed(() => Object.values(tasks.value).some(t => taskIsActive(t.phase)));
    let unlisten: (() => void) | undefined, unlistenRecords: (() => void) | undefined,
        connecting: Promise<void> | undefined, disposed = false, generation = 0;

    function apply(update: DownloadTaskSnapshot) {
        if (disposed || !update?.record || !Number.isSafeInteger(update.revision) || !Number.isSafeInteger(update.record.id)) return;
        if (removedRequests.has(update.record.requestId) || supersededRequests.has(update.record.requestId)) return;
        const old = tasks.value[update.record.id];
        if (old && old.revision >= update.revision) return;
        // Defensive rendering guard; native snapshots remain authoritative for phase and values.
        const valid = (value: number | null) => value != null && Number.isFinite(value) && value >= 0 ? value : null;
        const next = {...update, percent: valid(update.percent), speed: valid(update.speed), eta: valid(update.eta)};
        tasks.value = {...tasks.value, [update.record.id]: next};
        if (taskIsActive(next.phase) && old?.record.requestId !== next.record.requestId) {
            const errors = {...redownloadErrors.value};
            delete errors[update.record.id];
            redownloadErrors.value = errors;
        }
        knownRecords.value = {
            ...knownRecords.value,
            [`${update.record.platform}:${update.record.videoId}`]: update.record
        };
        rememberFormat(update.record);
    }

    function rememberFormat(record: DownloadRecord) {
        formatRecords.value = {...formatRecords.value, [formatKey(record)]: record};
    }

    function connect(): Promise<void> {
        if (connecting) return connecting;
        if (!bridge.desktop) return Promise.resolve();
        disposed = false;
        const current = ++generation;
        connecting = (async () => {
            try {
                if (!unlisten) {
                    const stop = await bridge.listen('download-task-changed', apply);
                    if (disposed || current !== generation) {
                        stop();
                        return;
                    }
                    unlisten = stop;
                }
                if (!unlistenRecords) {
                    const stop = await bridge.listen('download-records-changed', () => {
                        if (!disposed) recordsRevision.value++;
                    });
                    if (disposed || current !== generation) {
                        stop();
                        return;
                    }
                    unlistenRecords = stop;
                }
                const snapshots = await bridge.invoke<DownloadTaskSnapshot[]>('list_download_tasks');
                if (disposed || current !== generation) return;
                for (const snapshot of snapshots) apply(snapshot);
                error.value = null;
            } catch (e) {
                if (!disposed) error.value = typeof e === 'object' && e !== null && 'code' in e ? e as {
                    code: string
                } : {code: 'bridgeFailed', detail: String(e)};
            } finally {
                connecting = undefined;
            }
        })();
        return connecting;
    }

    async function submit(request: SubmitDownloadRequest) {
        if (!bridge.desktop) throw {code: 'desktopOnly'};
        await connect();
        const response = await bridge.invoke<SubmitDownloadResult>('enqueue_video_download', {request});
        rememberSubmission(response);
        return response;
    }

    function rememberSubmission(response: SubmitDownloadResult) {
        if (response.task) apply(response.task);
        const cached = taskFor(response.record.id);
        if (!response.task && ['alreadyDownloaded', 'confirmationRequired'].includes(response.kind)) {
            supersededRequests.add(response.record.requestId);
            if (cached?.record.requestId === response.record.requestId) {
                // Keep old snapshots from replacing the verified native record on reconnect.
                const next = {...tasks.value};
                delete next[response.record.id];
                tasks.value = next;
            }
        }
        const live = taskFor(response.record.id);
        const record = response.task || (live && taskIsActive(live.phase)) ? live?.record ?? response.record : response.record;
        knownRecords.value = {...knownRecords.value, [`${record.platform}:${record.videoId}`]: record};
        rememberFormat(record);
    }

    function cancel(requestId: string): Promise<boolean> {
        const pending = cancellations.get(requestId);
        if (pending) return pending;
        const operation = cancelRequest(requestId).finally(() => cancellations.delete(requestId));
        cancellations.set(requestId, operation);
        return operation;
    }

    async function cancelRequest(requestId: string) {
        try {
            if (!bridge.desktop) throw {code: 'desktopOnly'};
            await bridge.invoke('cancel_video_download', {requestId});
            removedRequests.add(requestId);
            tasks.value = Object.fromEntries(Object.entries(tasks.value).filter(([, task]) => task.record.requestId !== requestId));
            knownRecords.value = Object.fromEntries(Object.entries(knownRecords.value).filter(([, record]) => record.requestId !== requestId));
            formatRecords.value = Object.fromEntries(Object.entries(formatRecords.value).filter(([, record]) => record.requestId !== requestId));
            recordsRevision.value++;
            error.value = null;
            return true;
        } catch (e) {
            const failure = persistenceError(e);
            error.value = {code: failure.code === 'cancelCleanupFailed' ? failure.code : 'cancelFailed', detail: failure.detail};
            return false;
        }
    }

    async function setPaused(requestId: string, paused: boolean) {
        const code = paused ? 'pauseFailed' : 'resumeFailed';
        try {
            if (!bridge.desktop) throw {code: 'desktopOnly'};
            const update = await bridge.invoke<DownloadTaskSnapshot>(paused ? 'pause_video_download' : 'resume_video_download', {requestId});
            apply(update);
            error.value = null;
            return true;
        } catch (failure) {
            error.value = {code, detail: persistenceError(failure).detail};
            return false;
        }
    }

    function redownload(record: Pick<DownloadRecord, 'id' | 'requestId' | 'platform'>, deleteExisting = false): Promise<SubmitDownloadResult> {
        const pending = restarts.get(record.id);
        if (pending) return pending;
        submitting.value = {...submitting.value, [record.id]: true};
        const errors = {...redownloadErrors.value};
        delete errors[record.id];
        redownloadErrors.value = errors;
        const restart = Promise.resolve().then(async () => {
            try {
                if (!bridge.desktop) throw {code: 'desktopOnly'};
                await connect();
                await bridge.beforeRedownload?.(record.platform);
                const response = await bridge.invoke<SubmitDownloadResult>('redownload_record', {
                    id: record.id,
                    requestId: record.requestId,
                    ...(deleteExisting ? {deleteExisting: true} : {})
                });
                rememberSubmission(response);
                return response;
            } catch (e) {
                redownloadErrors.value = {
                    ...redownloadErrors.value,
                    [record.id]: typeof e === 'object' && e !== null && 'code' in e ? e as {
                        code: string;
                        detail?: string
                    } : {code: 'bridgeFailed', detail: String(e)}
                };
                throw e;
            } finally {
                const next = {...submitting.value};
                delete next[record.id];
                submitting.value = next;
                restarts.delete(record.id);
            }
        });
        restarts.set(record.id, restart);
        return restart;
    }

    function isSubmitting(id: number) {
        return Boolean(submitting.value[id]);
    }

    async function lookupRecord(platform: VideoPlatform, videoId: string) {
        if (!bridge.desktop) return null;
        const key = `${platform}:${videoId}`, token = (lookups.get(key) ?? 0) + 1,
            recordRevision = recordsRevision.value;
        lookups.set(key, token);
        const before = Object.values(tasks.value).find(t => t.record.platform === platform && t.record.videoId === videoId)?.revision ?? 0;
        const record = await bridge.invoke<DownloadRecord | null>('find_download_record', {platform, videoId});
        if (lookups.get(key) !== token || recordsRevision.value !== recordRevision) return knownRecords.value[key] ?? null;
        const live = record ? taskFor(record.id) : Object.values(tasks.value).find(task => task.record.platform === platform && task.record.videoId === videoId) ?? null;
        const latest = live && (taskIsActive(live.phase) || live.revision > before) ? live.record : record;
        if (latest) knownRecords.value = {...knownRecords.value, [key]: latest}; else {
            const next = {...knownRecords.value};
            delete next[key];
            knownRecords.value = next;
        }
        return latest;
    }

    function taskFor(id: number) {
        return tasks.value[id] ?? null;
    }

    async function lookupFormats(platform: VideoPlatform, videoId: string) {
        if (!bridge.desktop) return;
        const key = JSON.stringify([platform, videoId]), token = (formatLookups.get(key) ?? 0) + 1,
            recordRevision = recordsRevision.value, current = generation;
        formatLookups.set(key, token);
        const before = new Map(Object.values(tasks.value).map(task => [task.record.id, task.revision]));
        const records = await bridge.invoke<DownloadRecord[]>('find_video_download_records', {platform, videoId});
        if (disposed || current !== generation || formatLookups.get(key) !== token || recordsRevision.value !== recordRevision) return;
        const next = {...formatRecords.value};
        for (const [id, record] of Object.entries(next)) {
            if (record.platform === platform && record.videoId === videoId) delete next[id];
        }
        for (const record of records) {
            if (!record.deletedAt && !removedRequests.has(record.requestId)) next[formatKey(record)] = record;
        }
        // Events received after the query began, and active tasks, take precedence over its snapshot.
        for (const task of Object.values(tasks.value)) {
            if (task.record.platform === platform && task.record.videoId === videoId &&
                (taskIsActive(task.phase) || task.revision > (before.get(task.record.id) ?? 0))) {
                next[formatKey(task.record)] = task.record;
            }
        }
        formatRecords.value = next;
    }

    function recordForFormat(platform: VideoPlatform, videoId: string, formatId: string) {
        return formatRecords.value[formatKey({platform, videoId, formatId})] ?? null;
    }

    function mergeRecord(record: DownloadRecord) {
        const task = taskFor(record.id);
        return task && (taskIsActive(task.phase) || (task.record.requestId === record.requestId && (record.status === 'queued' || record.status === 'running'))) ? task.record : record;
    }

    function dispose() {
        disposed = true;
        ++generation;
        unlisten?.();
        unlisten = undefined;
        unlistenRecords?.();
        unlistenRecords = undefined;
    }

    return {
        tasks,
        knownRecords,
        recordsRevision,
        busy,
        error,
        redownloadErrors,
        isSubmitting,
        connect,
        submit,
        redownload,
        cancel,
        pause: (requestId: string) => setPaused(requestId, true),
        resume: (requestId: string) => setPaused(requestId, false),
        lookupRecord,
        lookupFormats,
        recordForFormat,
        taskFor,
        mergeRecord,
        dispose
    };
}

let controller: ReturnType<typeof createDownloadTasks> | undefined;

export function useDownloadTasks() {
    return controller ??= createDownloadTasks({
        desktop: isTauri(), invoke,
        listen: async (name, handler) => listen<DownloadTaskSnapshot>(name, event => handler(event.payload)),
        beforeRedownload: async platform => {
            if (!await usePlatformSettings().whenIdle(platform)) throw {code: 'proxySettingsFailed'};
            if (!await useCookieSettings().whenIdle(platform)) throw {code: 'cookieSaveFailed'};
        },
    });
}
