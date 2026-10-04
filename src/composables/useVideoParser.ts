import {computed, reactive, ref} from "vue";
import {extractVideoLink, platformIds, validateVideoLink, type VideoPlatform} from "./videoPlatforms.ts";
import type {DownloadPageState} from "./useDownloadPageState.ts";

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
    onChange?: (platform: VideoPlatform) => void;
    cacheThumbnail?: (url: string) => Promise<string | null>;
}

interface ParsedVideo {
    metadata: VideoMetadata;
    parsedAt: string;
    parserFingerprint: string
}

interface ParseError {
    code: string;
    detail?: string
}

interface ResultPreview {
    video: VideoMetadata;
    format: VideoFormat | null;
    thumbnailCachePath: string | null;
}

interface PlatformState {
    link: string;
    directory: string;
    directoryEdited: boolean;
    video: VideoMetadata | null;
    previousResult: ResultPreview | null;
    phase: "idle" | "parsing" | "ready";
    error: ParseError | null;
    quality: string | null;
    frameRate: string | null;
    version: number;
    selectedFormatId: string | null;
    thumbnailCachePath: string | null;
    parsedAt: string | null;
    parserFingerprint: string | null;
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
        link: "",
        directory: "",
        directoryEdited: false,
        video: null,
        previousResult: null,
        phase: "idle",
        error: null,
        quality: null,
        frameRate: null,
        version: 0,
        selectedFormatId: null,
        thumbnailCachePath: null,
        parsedAt: null,
        parserFingerprint: null,
    }]))) as Record<VideoPlatform, PlatformState>;
    const draft = computed(() => drafts[platform.value]);
    const video = computed(() => draft.value.video);
    const resultLink = computed(() => draft.value.video ? draft.value.link : "");
    const phase = computed(() => draft.value.phase);
    const error = computed(() => draft.value.error);
    const quality = computed(() => draft.value.quality);
    const frameRate = computed(() => draft.value.frameRate);
    const busy = computed(() => platformIds.some(id => drafts[id].phase === "parsing"));
    const qualities = computed(() => availableQualities(video.value));
    const frameRates = computed(() => availableFrameRates(video.value, quality.value));
    // yt-dlp returns formats in increasing preference order. Keep that order for ties.
    function formatFor(state: PlatformState) {
        const formats = state.video?.formats.filter(format =>
            (state.quality === null || String(format.height) === state.quality) &&
            (state.frameRate === null || String(format.fps) === state.frameRate)) ?? [];
        return formats.find(format => format.formatId === state.selectedFormatId) ?? formats[formats.length - 1] ?? null;
    }

    const selectedFormat = computed(() => formatFor(draft.value));
    const displayVideo = computed(() => video.value ?? draft.value.previousResult?.video ?? null);
    const displayFormat = computed(() => video.value ? selectedFormat.value : draft.value.previousResult?.format ?? null);
    const displayThumbnailCachePath = computed(() => video.value ? draft.value.thumbnailCachePath : draft.value.previousResult?.thumbnailCachePath ?? null);
    const pasting = ref(false);
    let pasteVersion = 0;
    let disposed = false;

    function changed(id: VideoPlatform = platform.value) {
        if (!disposed) bridge.onChange?.(id);
    }

    function snapshot(id: VideoPlatform = platform.value): DownloadPageState {
        const state = drafts[id], metadata = state.video, format = formatFor(state);
        return {
            platform: id,
            inputLink: state.link,
            videoId: metadata?.id ?? null,
            title: metadata?.title ?? null,
            thumbnailUrl: metadata?.thumbnail ?? null,
            thumbnailCachePath: state.thumbnailCachePath,
            durationSeconds: metadata?.duration ?? null,
            extension: metadata?.extension ?? null,
            formats: metadata?.formats ?? [],
            selectedFormatId: format?.formatId ?? null,
            selectedHeight: format?.height ?? null,
            selectedFps: format?.fps ?? null,
            cookieFallback: metadata?.cookieFallback ?? false,
            downloadDirectory: state.directory,
            directoryCustomized: state.directoryEdited,
            parserFingerprint: state.parserFingerprint,
            parsedAt: state.parsedAt,
            updatedAt: ""
        };
    }

    function restore(states: DownloadPageState[]) {
        for (const saved of states) {
            const state = drafts[saved.platform];
            if (!state) throw {code: "loadFailed", detail: "Unknown persisted platform"};
            const format = saved.formats.find(item => item.formatId === saved.selectedFormatId);
            if (saved.videoId !== null && (!format || format.height !== saved.selectedHeight || format.fps !== saved.selectedFps)) throw {
                code: "loadFailed",
                detail: "Invalid persisted format selection"
            };
            state.previousResult = null;
            state.link = saved.inputLink;
            state.directory = saved.downloadDirectory;
            state.directoryEdited = saved.directoryCustomized;
            state.video = saved.videoId !== null ? {
                id: saved.videoId!,
                title: saved.title!,
                thumbnail: saved.thumbnailUrl,
                duration: saved.durationSeconds,
                extension: saved.extension,
                cookieFallback: saved.cookieFallback,
                formats: saved.formats
            } : null;
            state.quality = saved.selectedHeight === null ? null : String(saved.selectedHeight);
            state.frameRate = saved.selectedFps === null ? null : String(saved.selectedFps);
            state.selectedFormatId = saved.selectedFormatId;
            state.thumbnailCachePath = saved.thumbnailCachePath;
            state.parsedAt = saved.parsedAt;
            state.parserFingerprint = saved.parserFingerprint;
            state.phase = state.video ? "ready" : "idle";
            state.error = null;
            state.version++;
        }
    }

    function cancelPaste() {
        pasteVersion += 1;
        pasting.value = false;
    }

    function reset(selected: VideoPlatform = platform.value, retainPreview = false) {
        cancelPaste();
        const state = drafts[selected];
        if (retainPreview && state.video) {
            state.previousResult = {
                video: state.video,
                format: formatFor(state),
                thumbnailCachePath: state.thumbnailCachePath
            };
        } else if (!retainPreview) {
            state.previousResult = null;
        }
        state.version += 1;
        state.video = null;
        state.phase = "idle";
        state.error = null;
        state.quality = state.frameRate = null;
        state.selectedFormatId = state.thumbnailCachePath = state.parsedAt = state.parserFingerprint = null;
        changed(selected);
    }

    function resetAll() {
        platformIds.forEach(id => reset(id));
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
        if (busy.value || disposed) return;
        cancelPaste();
        if (draft.value.link !== value) {
            draft.value.link = value;
            reset(platform.value, true);
        }
    }

    function setDirectory(value: string) {
        if (!busy.value && !disposed) {
            draft.value.directory = value;
            draft.value.directoryEdited = true;
            changed();
        }
    }

    function resetDirectory(directory: string) {
        if (busy.value || disposed) return;
        draft.value.directory = directory;
        draft.value.directoryEdited = false;
        changed();
    }

    function applyDefaultDirectories(directories: Record<VideoPlatform, string>) {
        for (const id of platformIds) {
            if (!drafts[id].directoryEdited && drafts[id].directory !== directories[id]) {
                drafts[id].directory = directories[id];
                changed(id);
            }
        }
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
        draft.value.selectedFormatId = null;
        changed();
    }

    function setFrameRate(value: string) {
        if (!busy.value && frameRates.value.includes(value)) {
            draft.value.frameRate = value;
            draft.value.selectedFormatId = null;
            changed();
        }
    }

    function selectFormat(formatId: string) {
        if (busy.value || disposed) return false;
        const format = video.value?.formats.find(item => item.formatId === formatId);
        if (!format) return false;
        draft.value.quality = format.height === null ? null : String(format.height);
        draft.value.frameRate = format.fps === null ? null : String(format.fps);
        draft.value.selectedFormatId = format.formatId;
        changed();
        return true;
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
        if (state.link !== validated.url) {
            state.link = validated.url;
            changed(selected);
        }
        state.phase = "parsing";
        try {
            const saved = await bridge.beforeParse(selected);
            if (current !== state.version || disposed) return;
            if (!saved) throw {code: "cookieSaveFailed"};
            const response = await bridge.invoke<ParsedVideo | VideoMetadata>("parse_video", {
                platform: selected,
                input: validated.url
            });
            if (current !== state.version || disposed) return;
            const result = "metadata" in response ? response.metadata : response;
            let cachePath: string | null = null;
            if (result.thumbnail && bridge.cacheThumbnail) cachePath = await bridge.cacheThumbnail(result.thumbnail);
            if (current !== state.version || disposed) return;
            state.video = result;
            state.quality = availableQualities(result)[0] ?? null;
            state.frameRate = availableFrameRates(result, state.quality)[0] ?? null;
            state.selectedFormatId = null;
            state.thumbnailCachePath = cachePath;
            state.parsedAt = "metadata" in response ? response.parsedAt : null;
            state.parserFingerprint = "metadata" in response ? response.parserFingerprint : null;
            state.phase = "ready";
            changed(selected);
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
        displayVideo,
        displayFormat,
        displayThumbnailCachePath,
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
        resetDirectory,
        applyDefaultDirectories,
        pasteLink,
        cancelPaste,
        setQuality,
        setFrameRate,
        selectFormat,
        updateCookie,
        parse,
        reset,
        resetAll,
        dispose,
        restore,
        snapshot
    };
}
