import {reactive, ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {platformIds, type VideoPlatform} from "./videoPlatforms.ts";
import {persistenceError, type PersistenceBridge, type PersistenceError} from "./useUiPreferences.ts";
import type {ProxySettings} from "./useProxySettings.ts";

export interface PlatformSettings {
    proxyEnabled: boolean
}

function records<T>(create: () => T): Record<VideoPlatform, T> {
    return Object.fromEntries(platformIds.map(id => [id, create()])) as Record<VideoPlatform, T>;
}

export function createPlatformSettings(bridge: PersistenceBridge) {
    const settings = reactive(records<PlatformSettings>(() => ({proxyEnabled: false})));
    const ready = reactive(records(() => false)), loading = reactive(records(() => false)),
        saving = reactive(records(() => false));
    const loadError = reactive(records<PersistenceError | null>(() => null)),
        saveError = reactive(records<PersistenceError | null>(() => null));
    const revision = reactive(records(() => 0));
    const proxy = ref<ProxySettings | null>(null), proxyReady = ref(false),
        proxyLoadError = ref<PersistenceError | null>(null);
    const loads: Partial<Record<VideoPlatform, Promise<boolean>>> = {};
    const saves = records(() => Promise.resolve(true));
    let proxyRequest = 0;

    async function loadProxy() {
        const current = ++proxyRequest;
        proxyReady.value = false;
        try {
            const value = bridge.desktop ? await bridge.invoke<ProxySettings | null>("get_proxy_settings") : null;
            if (current !== proxyRequest) return false;
            proxy.value = value;
            proxyLoadError.value = null;
            proxyReady.value = true;
            return true;
        } catch (error) {
            if (current === proxyRequest) proxyLoadError.value = persistenceError(error);
            return false;
        }
    }

    function load(platform: VideoPlatform): Promise<boolean> {
        if (ready[platform]) return Promise.resolve(true);
        if (loads[platform]) return loads[platform];
        loading[platform] = true;
        const task = (async () => {
            try {
                settings[platform] = bridge.desktop ? await bridge.invoke<PlatformSettings>("get_platform_settings", {platform}) : {proxyEnabled: false};
                ready[platform] = true;
                loadError[platform] = null;
                return true;
            } catch (error) {
                loadError[platform] = persistenceError(error);
                return false;
            } finally {
                loading[platform] = false;
                delete loads[platform];
            }
        })();
        loads[platform] = task;
        return task;
    }

    function updateProxy(platform: VideoPlatform, enabled: boolean): Promise<boolean> {
        if (!bridge.desktop || !ready[platform] || saving[platform]) return Promise.resolve(false);
        if (settings[platform].proxyEnabled === enabled) return Promise.resolve(true);
        const previous = {...settings[platform]}, snapshot = {proxyEnabled: enabled};
        settings[platform] = snapshot;
        saving[platform] = true;
        saveError[platform] = null;
        const task = (async () => {
            try {
                settings[platform] = await bridge.invoke<PlatformSettings>("save_platform_settings", {
                    platform,
                    settings: snapshot
                });
                revision[platform]++;
                return true;
            } catch (error) {
                settings[platform] = previous;
                saveError[platform] = persistenceError(error);
                return false;
            } finally {
                saving[platform] = false;
            }
        })();
        saves[platform] = task;
        return task;
    }

    async function whenIdle(platform: VideoPlatform) {
        if (!await load(platform)) return false;
        if (saving[platform]) return saves[platform];
        return true; // Failed switches are rolled back to the persisted configuration.
    }

    return {
        desktop: bridge.desktop, settings, ready, loading, saving, loadError, saveError, revision,
        proxy, proxyReady, proxyLoadError, loadProxy, load, updateProxy, whenIdle
    };
}

let controller: ReturnType<typeof createPlatformSettings> | undefined;

export function usePlatformSettings() {
    return controller ??= createPlatformSettings({desktop: isTauri(), invoke});
}
