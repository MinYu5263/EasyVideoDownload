import {reactive, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";

export type AppLocale = "zh-CN" | "en";
export type AppTheme = "system" | "light" | "dark";
export type CloseAction = "ask" | "tray" | "exit";

export interface AppSettings {
    locale: AppLocale;
    theme: AppTheme;
    notifyOnCompletion: boolean;
    notifyOnFailure: boolean;
    closeAction: CloseAction;
}

interface SettingsError {
    code: "loadFailed" | "saveFailed" | "invalidSettings" | "bridgeFailed";
    detail: string;
}

interface SettingsBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

function bridgeError(error: unknown): SettingsError {
    if (typeof error === "object" && error !== null && "code" in error) return error as SettingsError;
    return {code: "bridgeFailed", detail: String(error)};
}

export function createAppSettings(bridge: SettingsBridge) {
    const settings = reactive<AppSettings>({
        locale: "en", theme: "system", notifyOnCompletion: true,
        notifyOnFailure: true, closeAction: "ask",
    });
    const draft = reactive<AppSettings>({...settings});
    const ready = ref(false);
    const loading = ref(false);
    const saving = ref(false);
    const hasSaved = ref(false);
    const loadError = ref<SettingsError | null>(null);
    const saveError = ref<SettingsError | null>(null);
    let queue: Promise<unknown> = Promise.resolve();
    const pendingPatches: Partial<AppSettings>[] = [];

    function refreshDraft() {
        const next = {...settings};
        for (const patch of pendingPatches) Object.assign(next, patch);
        Object.assign(draft, next);
    }

    async function load(initialLocale: AppLocale) {
        if (loading.value || saving.value) return;
        loading.value = true;
        ready.value = false;
        settings.locale = initialLocale;
        refreshDraft();
        try {
            if (bridge.desktop) {
                const saved = await bridge.invoke<AppSettings>("get_app_settings", {initialLocale});
                Object.assign(settings, saved);
            }
            loadError.value = null;
            ready.value = true;
        } catch (error) {
            loadError.value = bridgeError(error);
        } finally {
            refreshDraft();
            loading.value = false;
        }
    }

    function update(patch: Partial<AppSettings>): Promise<boolean> {
        if (!ready.value) return Promise.resolve(false);
        const queuedPatch = {...patch};
        pendingPatches.push(queuedPatch);
        refreshDraft();
        saving.value = true;
        const task = queue.then(async () => {
            try {
                // Build from the last committed settings, including preceding queued changes.
                const next = {...settings, ...queuedPatch};
                const saved = bridge.desktop
                    ? await bridge.invoke<AppSettings>("save_app_settings", {settings: next})
                    : next;
                Object.assign(settings, saved);
                saveError.value = null;
                hasSaved.value = bridge.desktop;
                return true;
            } catch (error) {
                saveError.value = bridgeError(error);
                hasSaved.value = false;
                return false;
            } finally {
                pendingPatches.shift();
                // Keep newer edits visible when an earlier write completes or fails.
                refreshDraft();
                saving.value = pendingPatches.length > 0;
            }
        });
        queue = task;
        return task;
    }

    return {
        desktop: bridge.desktop,
        settings,
        draft,
        ready,
        loading,
        saving,
        hasSaved,
        loadError,
        saveError,
        load,
        update
    };
}

let controller: ReturnType<typeof createAppSettings> | undefined;

export function useAppSettings() {
    controller ??= createAppSettings({desktop: isTauri(), invoke});
    return controller;
}
