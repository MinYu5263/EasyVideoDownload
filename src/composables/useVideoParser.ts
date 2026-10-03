import {computed, reactive, ref} from "vue";
import {extractVideoLink, platformIds, validateVideoLink, type VideoPlatform} from "./videoPlatforms.ts";

export {platformIds} from "./videoPlatforms.ts";

export interface VideoFormat {
    formatId: string;
    height: number | null;
    fps: number | null;
    extension: string | null;
    sizeBytes: number | null;
    sizeApproximate: boolean;
}

export interface VideoMetadata {
    id: string;
    title: string;
    thumbnail: string | null;
    duration: number | null;
    extension: string | null;
    cookieFallback: boolean;
    formats: VideoFormat[];
}

interface ParserBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    beforeParse: (platform: VideoPlatform) => Promise<boolean>;
    saveCookie: (platform: VideoPlatform, contents: string) => Promise<boolean>;
}

interface ParseError {
    code: string;
    detail?: string
}

interface PlatformState {
    link: string;
    resultLink: string;
    directory: string;
    video: VideoMetadata | null;
    phase: "idle" | "parsing" | "ready";
    error: ParseError | null;
    quality: string | null;
    frameRate: string | null;
    version: number;
}

function availableQualities(video: VideoMetadata | null) {
    return [...new Set(video?.formats.flatMap(format => format.height ? [String(format.height)] : []) ?? [])]
        .sort((a, b) => Number(b) - Number(a));
}

function availableFrameRates(video: VideoMetadata | null, quality: string | null) {
    return [...new Set(video?.formats.filter(format => quality === null || String(format.height) === quality)
        .flatMap(format => format.fps ? [String(format.fps)] : []) ?? [])]
        .sort((a, b) => Number(b) - Number(a));
}

export function createVideoParser(bridge: ParserBridge) {
    const platform = ref<VideoPlatform>(platformIds[0]);
    const drafts = reactive(Object.fromEntries(platformIds.map(id => [id, {
        link: "", resultLink: "", directory: "", video: null, phase: "idle", error: null,
        quality: null, frameRate: null, version: 0,
    }]))) as Record<VideoPlatform, PlatformState>;
    const draft = computed(() => drafts[platform.value]);
    const video = computed(() => draft.value.video);
    const resultLink = computed(() => draft.value.resultLink);
    const phase = computed(() => draft.value.phase);
    const error = computed(() => draft.value.error);
    const quality = computed(() => draft.value.quality);
    const frameRate = computed(() => draft.value.frameRate);
    const busy = computed(() => platformIds.some(id => drafts[id].phase === "parsing"));
    const qualities = computed(() => availableQualities(video.value));
    const frameRates = computed(() => availableFrameRates(video.value, quality.value));
    // yt-dlp returns formats in increasing preference order. Keep that order for ties.
    const selectedFormat = computed(() => {
        const formats = video.value?.formats.filter(format =>
            (quality.value === null || String(format.height) === quality.value) &&
            (frameRate.value === null || String(format.fps) === frameRate.value)) ?? [];
        return formats[formats.length - 1] ?? null;
    });
    const pasting = ref(false);
    let pasteVersion = 0;
    let disposed = false;

    function cancelPaste() {
        pasteVersion += 1;
        pasting.value = false;
    }

    function reset(selected: VideoPlatform = platform.value) {
        cancelPaste();
        const state = drafts[selected];
        state.version += 1;
        state.video = null;
        state.resultLink = "";
        state.phase = "idle";
        state.error = null;
        state.quality = state.frameRate = null;
    }

    function resetAll() {
        platformIds.forEach(reset);
    }

    function dispose() {
        disposed = true;
        resetAll();
    }

    function selectPlatform(next: VideoPlatform) {
        if (busy.value || platform.value === next) return;
        cancelPaste();
        platform.value = next;
    }

    function setLink(value: string) {
        if (busy.value) return;
        cancelPaste();
        if (draft.value.link !== value) {
            draft.value.link = value;
            draft.value.version += 1;
            draft.value.error = null;
        }
    }

    function setDirectory(value: string) {
        if (!busy.value) draft.value.directory = value;
    }

    async function pasteLink(read: () => Promise<string>) {
        if (busy.value || pasting.value || disposed) return false;
        const current = ++pasteVersion;
        pasting.value = true;
        try {
            const contents = await read();
            if (current !== pasteVersion || disposed || !contents.trim()) return false;
            setLink(extractVideoLink(contents) ?? contents.trim());
            return true;
        } catch (failure) {
            if (current === pasteVersion && !disposed) throw failure;
            return false;
        } finally {
            if (current === pasteVersion) pasting.value = false;
        }
    }

    function setQuality(value: string) {
        if (busy.value || quality.value === value || !qualities.value.includes(value)) return;
        draft.value.quality = value;
        draft.value.frameRate = frameRates.value[0] ?? null;
    }

    function setFrameRate(value: string) {
        if (!busy.value && frameRates.value.includes(value)) draft.value.frameRate = value;
    }

    function updateCookie(contents: string) {
        // Keep the visible result and choices. An explicit parse reads the latest saved Cookie.
        return bridge.saveCookie(platform.value, contents);
    }

    async function parse(): Promise<ParseError | undefined> {
        if (busy.value || disposed) return;
        cancelPaste();
        const selected = platform.value;
        const state = drafts[selected];
        const current = ++state.version;
        state.error = null;
        const validated = validateVideoLink(state.link, selected);
        if (validated.error) {
            state.error = {code: validated.error};
            return state.error;
        }
        if (!bridge.desktop) {
            state.error = {code: "desktopOnly"};
            return state.error;
        }
        state.link = validated.url;
        state.phase = "parsing";
        try {
            const saved = await bridge.beforeParse(selected);
            if (current !== state.version || disposed) return;
            if (!saved) throw {code: "cookieSaveFailed"};
            const result = await bridge.invoke<VideoMetadata>("parse_video", {
                platform: selected,
                input: validated.url
            });
            if (current !== state.version || disposed) return;
            state.video = result;
            state.resultLink = validated.url;
            state.quality = availableQualities(result)[0] ?? null;
            state.frameRate = availableFrameRates(result, state.quality)[0] ?? null;
            state.phase = "ready";
        } catch (failure) {
            if (current !== state.version || disposed) return;
            state.error = typeof failure === "object" && failure !== null && "code" in failure
                ? failure as ParseError : {code: "bridgeFailed", detail: String(failure)};
            state.phase = state.video ? "ready" : "idle";
            return state.error;
        }
    }

    return {
        platform,
        draft,
        video,
        resultLink,
        phase,
        error,
        quality,
        frameRate,
        busy,
        qualities,
        frameRates,
        selectedFormat,
        pasting,
        selectPlatform,
        setLink,
        setDirectory,
        pasteLink,
        cancelPaste,
        setQuality,
        setFrameRate,
        updateCookie,
        parse,
        reset,
        resetAll,
        dispose
    };
}
