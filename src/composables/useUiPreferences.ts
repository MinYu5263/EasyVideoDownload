import {reactive, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import type {VideoPlatform} from "./videoPlatforms.ts";
import type {PersistedAppPageId} from "../navigation.ts";

export interface UiPreferences {
    downloadPlatform: VideoPlatform;
    settingsSection: "application" | "tools" | "proxy" | "about";
    activePage: PersistedAppPageId
}

export interface PersistenceError {
    code: string;
    detail?: string
}

export interface PersistenceBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>
}

export function persistenceError(error: unknown): PersistenceError {
    return typeof error === "object" && error !== null && "code" in error ? error as PersistenceError : {
        code: "bridgeFailed",
        detail: String(error)
    };
}

export function createUiPreferences(bridge: PersistenceBridge) {
    const draft = reactive<UiPreferences>({
        downloadPlatform: "douyin",
        settingsSection: "application",
        activePage: "download"
    });
    const ready = ref(false), loading = ref(false), saving = ref(false);
    const loadError = ref<PersistenceError | null>(null), saveError = ref<PersistenceError | null>(null);
    let loadTask: Promise<boolean> | undefined, queue: Promise<boolean> = Promise.resolve(true), revision = 0,
        pending = 0;

    function load(): Promise<boolean> {
        if (ready.value) return Promise.resolve(true);
        if (loadTask) return loadTask;
        loading.value = true;
        loadTask = (async () => {
            try {
                if (bridge.desktop) Object.assign(draft, await bridge.invoke<UiPreferences>("get_ui_preferences"));
                loadError.value = null;
                ready.value = true;
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

    function update(patch: Partial<UiPreferences>): Promise<boolean> {
        if (!ready.value) return Promise.resolve(false);
        Object.assign(draft, patch);
        const settings = {...draft}, current = ++revision;
        pending++;
        saving.value = true;
        queue = queue.then(async () => {
            try {
                if (bridge.desktop) await bridge.invoke("save_ui_preferences", {settings});
                if (current === revision) saveError.value = null;
                return true;
            } catch (error) {
                if (current === revision) saveError.value = persistenceError(error);
                return false;
            } finally {
                pending--;
                saving.value = pending > 0;
            }
        });
        return queue;
    }

    return {draft, ready, loading, saving, loadError, saveError, load, update};
}

let controller: ReturnType<typeof createUiPreferences> | undefined;

export function useUiPreferences() {
    return controller ??= createUiPreferences({desktop: isTauri(), invoke});
}
