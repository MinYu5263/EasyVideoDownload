import {reactive} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {platformIds, type VideoPlatform} from "./videoPlatforms.ts";

interface CookieError {
    code: string;
    detail?: string;
}

interface CookieBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
}

function platformRecord<T>(value: T): Record<VideoPlatform, T> {
    return Object.fromEntries(platformIds.map(platform => [platform, value])) as Record<VideoPlatform, T>;
}

function cookieError(error: unknown, code: string): CookieError {
    if (typeof error === "object" && error !== null && "code" in error) return error as CookieError;
    return {code, detail: String(error)};
}

export function createCookieSettings(bridge: CookieBridge) {
    const contents = reactive(platformRecord(""));
    const ready = reactive(platformRecord(!bridge.desktop));
    const loading = reactive(platformRecord(false));
    const saving = reactive(platformRecord(false));
    const loadError = reactive(platformRecord<CookieError | null>(null));
    const saveError = reactive(platformRecord<CookieError | null>(null));
    const queues = platformRecord<Promise<boolean>>(Promise.resolve(true));
    const pending = platformRecord(0);
    const revisions = platformRecord(0);
    const loads: Partial<Record<VideoPlatform, Promise<boolean>>> = {};

    function load(platform: VideoPlatform): Promise<boolean> {
        if (ready[platform]) {
            // Reopening preserves an unsaved draft and retries its latest contents.
            return queues[platform].then(() => saveError[platform]
                ? update(platform, contents[platform])
                : true);
        }
        if (loads[platform]) return loads[platform];
        loading[platform] = true;
        const task = (async () => {
            try {
                contents[platform] = await bridge.invoke<string>("get_cookie_contents", {platform});
                ready[platform] = true;
                loadError[platform] = null;
                return true;
            } catch (error) {
                loadError[platform] = cookieError(error, "loadFailed");
                return false;
            } finally {
                loading[platform] = false;
                delete loads[platform];
            }
        })();
        loads[platform] = task;
        return task;
    }

    function update(platform: VideoPlatform, value: string): Promise<boolean> {
        // Do not replace an unread file with the editor's initial empty value.
        if (!ready[platform]) return Promise.resolve(false);
        contents[platform] = value;
        const revision = ++revisions[platform];
        pending[platform] += 1;
        saving[platform] = true;
        const task = queues[platform].then(async () => {
            try {
                if (bridge.desktop) await bridge.invoke<void>("save_cookie_contents", {platform, contents: value});
                if (revision === revisions[platform]) saveError[platform] = null;
                return true;
            } catch (error) {
                if (revision === revisions[platform]) saveError[platform] = cookieError(error, "saveFailed");
                return false;
            } finally {
                pending[platform] -= 1;
                saving[platform] = pending[platform] > 0;
            }
        });
        queues[platform] = task;
        return task;
    }

    function whenIdle(platform: VideoPlatform) {
        return queues[platform];
    }

    return {desktop: bridge.desktop, contents, ready, loading, saving, loadError, saveError, load, update, whenIdle};
}

let controller: ReturnType<typeof createCookieSettings> | undefined;

export function useCookieSettings() {
    controller ??= createCookieSettings({desktop: isTauri(), invoke});
    return controller;
}
