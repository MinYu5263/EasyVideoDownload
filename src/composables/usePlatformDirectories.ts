import {reactive, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {platformIds, type VideoPlatform} from "./videoPlatforms.ts";
import {persistenceError, type PersistenceBridge, type PersistenceError} from "./useUiPreferences.ts";
import type {DownloadPageState} from "./useDownloadPageState.ts";

export function createPlatformDirectories(bridge: PersistenceBridge) {
    const settings = reactive(Object.fromEntries(platformIds.map(id => [id, {
        directory: "",
        customized: false
    }]))) as Record<VideoPlatform, { directory: string; customized: boolean }>;
    const ready = reactive({douyin: false, bilibili: false, youtube: false});
    const invalidStoredDirectory = reactive({douyin: false, bilibili: false, youtube: false});
    const error = reactive<Partial<Record<VideoPlatform, PersistenceError | null>>>({});
    const working = ref<VideoPlatform | null>(null);
    const loads: Partial<Record<VideoPlatform, Promise<boolean>>> = {};
    let operation = Promise.resolve(true);

    function load(platform: VideoPlatform): Promise<boolean> {
        if (ready[platform]) return Promise.resolve(true);
        if (loads[platform]) return loads[platform];
        loads[platform] = (async () => {
            try {
                if (!bridge.desktop) return false;
                const saved = await bridge.invoke<string | null>("get_platform_download_directory", {platform});
                const directory = saved ?? (await bridge.invoke<Record<VideoPlatform, string>>("get_default_download_directories"))[platform];
                settings[platform] = {directory, customized: saved !== null};
                ready[platform] = true;
                invalidStoredDirectory[platform] = false;
                error[platform] = null;
                return true;
            } catch (failure) {
                error[platform] = persistenceError(failure);
                invalidStoredDirectory[platform] = error[platform]?.code === 'invalidDownloadDirectory';
                return false;
            } finally {
                delete loads[platform];
            }
        })();
        return loads[platform]!;
    }

    function canChange(platform: VideoPlatform) {
        return bridge.desktop && (ready[platform] || invalidStoredDirectory[platform]) && !working.value;
    }

    function change(platform: VideoPlatform, reset: boolean): Promise<boolean> {
        if (!canChange(platform)) return Promise.resolve(false);
        working.value = platform;
        if (ready[platform]) error[platform] = null;
        operation = (async () => {
            try {
                const directory = reset ? null : await bridge.invoke<string | null>("select_download_directory");
                if (!reset && directory === null) return true;
                const resolved = directory ?? (await bridge.invoke<Record<VideoPlatform, string>>("get_default_download_directories"))[platform];
                await bridge.invoke("save_platform_download_directory", {platform, directory});
                settings[platform] = {directory: resolved, customized: directory !== null};
                ready[platform] = true;
                invalidStoredDirectory[platform] = false;
                error[platform] = null;
                return true;
            } catch (failure) {
                error[platform] = persistenceError(failure);
                return false;
            } finally {
                working.value = null;
            }
        })();
        return operation;
    }

    async function whenIdle(platform: VideoPlatform) {
        if (!await load(platform)) return false;
        while (working.value === platform) await operation;
        return true; // Failed writes leave the previous persisted directory available.
    }

    function applyToSnapshot(snapshot: DownloadPageState): DownloadPageState {
        const value = settings[snapshot.platform];
        return {...snapshot, downloadDirectory: value.directory, directoryCustomized: value.customized};
    }

    return {
        desktop: bridge.desktop,
        settings,
        ready,
        error,
        working,
        load,
        whenIdle,
        applyToSnapshot,
        canChange,
        choose: (platform: VideoPlatform) => change(platform, false),
        reset: (platform: VideoPlatform) => change(platform, true)
    };
}

let controller: ReturnType<typeof createPlatformDirectories> | undefined;

export function usePlatformDirectories() {
    return controller ??= createPlatformDirectories({desktop: isTauri(), invoke});
}
