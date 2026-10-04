import {ref} from "vue";
import {platformIds, type VideoPlatform} from "./videoPlatforms.ts";
import type {VideoFormat} from "./useVideoParser.ts";
import {persistenceError, type PersistenceBridge, type PersistenceError} from "./useUiPreferences.ts";

export interface DownloadPageState {
    platform: VideoPlatform;
    inputLink: string;
    videoId: string | null;
    title: string | null;
    thumbnailUrl: string | null;
    thumbnailCachePath: string | null;
    durationSeconds: number | null;
    extension: string | null;
    formats: VideoFormat[];
    selectedFormatId: string | null;
    selectedHeight: number | null;
    selectedFps: number | null;
    cookieFallback: boolean;
    downloadDirectory: string;
    directoryCustomized: boolean;
    parserFingerprint: string | null;
    parsedAt: string | null;
    updatedAt: string;
}

export function createDownloadPageState(bridge: PersistenceBridge) {
    const ready = ref(false), loading = ref(false), states = ref<DownloadPageState[]>([]);
    const loadError = ref<PersistenceError | null>(null);
    const saveErrors = ref<Partial<Record<VideoPlatform, PersistenceError | null>>>({});
    const queues = Object.fromEntries(platformIds.map(id => [id, Promise.resolve(true)])) as Record<VideoPlatform, Promise<boolean>>;
    const latest: Partial<Record<VideoPlatform, DownloadPageState>> = {},
        revisions = {douyin: 0, bilibili: 0, youtube: 0};
    let loadTask: Promise<boolean> | undefined;

    function load(): Promise<boolean> {
        if (ready.value) return Promise.resolve(true);
        if (loadTask) return loadTask;
        loading.value = true;
        loadTask = (async () => {
            try {
                states.value = bridge.desktop ? await bridge.invoke<DownloadPageState[]>("get_download_page_states") : [];
                ready.value = true;
                loadError.value = null;
                return true;
            } catch (error) {
                loadError.value = persistenceError(error);
                return false;
            } finally {
                loading.value = false;
                loadTask = undefined;
            }
        })();
        return loadTask;
    }

    function save(value: DownloadPageState): Promise<boolean> {
        if (!ready.value) return Promise.resolve(false);
        const snapshot = JSON.parse(JSON.stringify(value)) as DownloadPageState, id = snapshot.platform,
            current = ++revisions[id];
        latest[id] = snapshot;
        queues[id] = queues[id].then(async () => {
            try {
                if (bridge.desktop) await bridge.invoke("save_download_page_state", {snapshot});
                if (current === revisions[id]) saveErrors.value[id] = null;
                return true;
            } catch (error) {
                if (current === revisions[id]) saveErrors.value[id] = persistenceError(error);
                return false;
            }
        });
        return queues[id];
    }

    async function whenIdle(id: VideoPlatform): Promise<boolean> {
        if (!ready.value) return false;
        let pending: Promise<boolean>;
        do {
            pending = queues[id];
            await pending;
        } while (pending !== queues[id]);
        return saveErrors.value[id] && latest[id] ? save(latest[id]) : true;
    }

    return {ready, loading, states, loadError, saveErrors, load, save, whenIdle};
}
