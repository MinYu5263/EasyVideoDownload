import {ref} from "vue";
import {isTauri} from "@tauri-apps/api/core";
import {getCurrentWindow} from "@tauri-apps/api/window";
import type {AppTheme} from "./useAppSettings.ts";

type Theme = "light" | "dark";

interface AppearanceBridge {
    setTheme: (theme: Theme | null) => Promise<void>;
    theme: () => Promise<Theme | null>;
    onThemeChanged: (handler: (event: { payload: Theme }) => void) => Promise<() => void>;
}

export function createAppAppearance(bridge: AppearanceBridge, apply: (theme: Theme) => void) {
    const error = ref<string | null>(null);
    let preference: AppTheme = "system";
    let revision = 0, systemRevision = 0;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    let nativeQueue: Promise<void> = Promise.resolve();

    async function setTheme(theme: AppTheme) {
        if (disposed) return;
        preference = theme;
        const current = ++revision;
        try {
            const task = nativeQueue.then(() => bridge.setTheme(theme === "system" ? null : theme));
            nativeQueue = task.catch(() => {
            });
            await task;
            if (current !== revision || disposed) return;
            const observed = systemRevision;
            const resolved = theme === "system" ? await bridge.theme() : theme;
            if (current !== revision || disposed) return;
            if (observed === systemRevision) apply(resolved ?? "light");
            error.value = null;
        } catch (failure) {
            if (current === revision && !disposed) error.value = String(failure);
        }
    }

    async function start(theme: AppTheme) {
        disposed = false;
        try {
            if (!unlisten) {
                unlisten = await bridge.onThemeChanged(event => {
                    if (!disposed && preference === "system") {
                        ++systemRevision;
                        apply(event.payload);
                    }
                });
            }
            await setTheme(theme);
        } catch (failure) {
            error.value = String(failure);
        }
    }

    function dispose() {
        disposed = true;
        ++revision;
        unlisten?.();
        unlisten = undefined;
    }

    return {error, start, setTheme, dispose};
}

let controller: ReturnType<typeof createAppAppearance> | undefined;

export function useAppAppearance() {
    if (!controller) {
        const bridge: AppearanceBridge = isTauri() ? getCurrentWindow() : {
            setTheme: async () => {
            },
            theme: async () => window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
            onThemeChanged: async handler => {
                const media = window.matchMedia("(prefers-color-scheme: dark)");
                const changed = () => handler({payload: media.matches ? "dark" : "light"});
                media.addEventListener("change", changed);
                return () => media.removeEventListener("change", changed);
            },
        };
        controller = createAppAppearance(bridge, theme => {
            document.documentElement.classList.toggle("dark", theme === "dark");
            document.documentElement.style.colorScheme = theme;
        });
    }
    return controller;
}
