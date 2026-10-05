import {computed, reactive, ref} from "vue";
import {extractVideoLink, platformIds, validateVideoLink, type VideoPlatform} from "./videoPlatforms.ts";
import type {DownloadPageState} from "./useDownloadPageState.ts";

export {platformIds} from "./videoPlatforms.ts";

export interface VideoFormat {
    width?: number | null;
    qualityLabel?: string | null;
    qualityLabelSource?: string | null;
    codecLabel?: string | null;
    videoCodec?: string | null;
    bitrate?: number | null;
    watermarked?: boolean | null;
    nativeResultId?: string | null;
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
    defaultFormatId: string | null;
}

interface ParserBridge {
    desktop: boolean;
    invoke: <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
    beforeParse: (platform: VideoPlatform) => Promise<boolean>;
    saveCookie: (platform: VideoPlatform, contents: string) => Promise<boolean>;
    onChange?: (platform: VideoPlatform) => void;
    cacheThumbnail?: (url: string, platform: VideoPlatform) => Promise<string | null>;
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
    version: number;
    selectedFormatId: string | null;
    thumbnailCachePath: string | null;
    parsedAt: string | null;
    parserFingerprint: string | null;
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
    const busy = computed(() => phase.value === "parsing");

    // Preserve the explicit UI selection. Rust owns the default and format order.
    function formatFor(state: PlatformState) {
        return state.video?.formats.find(format => format.formatId === state.selectedFormatId) ?? null;
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
                formats: saved.formats,
                defaultFormatId: null,
            } : null;
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
        state.selectedFormatId = state.thumbnailCachePath = state.parsedAt = state.parserFingerprint = null;
        changed(selected);
    }

    function resetAll() {
        platformIds.forEach(id => reset(id));
    }

    function invalidateToolFormats() {
        reset('bilibili');
        reset('youtube');
    }

    function dispose() {
        disposed = true;
        resetAll();
    }

    function selectPlatform(next: VideoPlatform) {
        if (disposed || platform.value === next) return;
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

    function selectFormat(formatId: string) {
        if (busy.value || disposed) return false;
        const format = video.value?.formats.find(item => item.formatId === formatId);
        if (!format) return false;
        draft.value.selectedFormatId = format.formatId;
        changed();
        return true;
    }

    function updateCookie(contents: string) {
        // Keep the visible result and choices. An explicit parse reads the latest saved Cookie.
        return bridge.saveCookie(platform.value, contents);
    }

    function isCurrentError(selected: VideoPlatform, failure: ParseError) {
        return !disposed && drafts[selected].error === failure;
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
            if (result.thumbnail && bridge.cacheThumbnail) cachePath = await bridge.cacheThumbnail(result.thumbnail, selected);
            if (current !== state.version || disposed) return;
            state.video = result;
            const initial = result.formats.find(f => f.formatId === state.selectedFormatId)
                ?? result.formats.find(f => f.formatId === result.defaultFormatId) ?? null;
            state.selectedFormatId = initial?.formatId ?? null;
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
        busy,
        selectedFormat,
        pasting,
        selectPlatform,
        setLink,
        setDirectory,
        resetDirectory,
        applyDefaultDirectories,
        pasteLink,
        cancelPaste,
        selectFormat,
        updateCookie,
        isCurrentError,
        parse,
        reset,
        resetAll,
        invalidateToolFormats,
        dispose,
        restore,
        snapshot
    };
}
