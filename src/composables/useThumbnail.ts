import {onUnmounted, ref, watch, type Ref} from "vue";
import {invoke, isTauri} from "@tauri-apps/api/core";
import {type PersistenceBridge, type PersistenceError, persistenceError} from "./useUiPreferences.ts";

interface ThumbnailBridge extends PersistenceBridge {
    createUrl: (data: { mime: string; bytes: number[] }) => string;
    revokeUrl: (url: string) => void;
    onError?: (error: PersistenceError) => void;
}

export function createThumbnail(bridge: ThumbnailBridge) {
    const source = ref<string | null>(null);
    let revision = 0, objectUrl: string | null = null, disposed = false;

    function release() {
        if (objectUrl) bridge.revokeUrl(objectUrl);
        objectUrl = null;
    }

    async function load(path: string | null, remote: string | null) {
        if (disposed) return;
        const current = ++revision;
        release();
        source.value = remote;
        if (!path || !bridge.desktop) return;
        try {
            const data = await bridge.invoke<{ mime: string; bytes: number[] }>("get_cached_thumbnail", {path});
            if (current !== revision || disposed) return;
            objectUrl = bridge.createUrl(data);
            source.value = objectUrl;
        } catch (e) {
            if (current === revision && !disposed) {
                source.value = remote;
                bridge.onError?.(persistenceError(e));
            }
        }
    }

    function dispose() {
        disposed = true;
        revision++;
        release();
    }

    return {source, load, dispose};
}

export function useThumbnail(cachePath: Ref<string | null>, remoteUrl: Ref<string | null>, onError?: (error: PersistenceError) => void) {
    const renderer = createThumbnail({
        desktop: isTauri(),
        invoke,
        onError,
        createUrl: data => URL.createObjectURL(new Blob([new Uint8Array(data.bytes)], {type: data.mime})),
        revokeUrl: url => URL.revokeObjectURL(url)
    });
    watch([cachePath, remoteUrl], ([path, remote]) => {
        void renderer.load(path, remote);
    }, {immediate: true});
    onUnmounted(renderer.dispose);
    return renderer.source;
}
