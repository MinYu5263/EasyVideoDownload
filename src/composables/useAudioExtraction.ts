import {computed, ref} from 'vue';
import {invoke, isTauri} from '@tauri-apps/api/core';
import {type Event, listen, type UnlistenFn} from '@tauri-apps/api/event';

export interface AudioTrack {
    index: number;
    codec: string;
    codecLabel: string;
    outputFormat: string;
    duration: number | null;
    bitrate: number | null;
    sampleRate: number | null;
    channels: number | null;
    language: string | null;
    title: string | null;
    isDefault: boolean;
}

export interface AudioInfo {
    id: string;
    fileName: string;
    tracks: AudioTrack[];
    defaultTrack: number;
}

export interface AudioSnapshot {
    revision: number;
    phase: 'idle' | 'parsing' | 'ready' | 'extracting' | 'completed' | 'failed';
    progress: number | null;
    info: AudioInfo | null;
}

interface Bridge {
    desktop: boolean;
    invoke: typeof invoke;
    listen: (name: string, handler: (event: Event<AudioSnapshot>) => void) => Promise<UnlistenFn>;
}

export function createAudioExtraction(bridge: Bridge) {
    const state = ref<AudioSnapshot>({revision: -1, phase: 'idle', progress: null, info: null});
    const selectedTrack = ref<number>();
    const submitting = ref(false), ready = ref(false);
    const busy = computed(() => submitting.value || ['parsing', 'extracting'].includes(state.value.phase));
    const track = computed(() => state.value.info?.tracks.find(t => t.index === selectedTrack.value));
    let unlisten: UnlistenFn | undefined, disposed = false;

    function merge(snapshot: AudioSnapshot) {
        if (disposed || snapshot.revision <= state.value.revision) return;
        if (snapshot.info?.id !== state.value.info?.id) selectedTrack.value = snapshot.info?.defaultTrack;
        state.value = snapshot;
    }

    async function connect() {
        if (!bridge.desktop || ready.value || disposed) return;
        const stop = await bridge.listen('audio-extraction-state', event => merge(event.payload));
        if (disposed) {
            stop();
            return;
        }
        unlisten = stop;
        try {
            merge(await bridge.invoke<AudioSnapshot>('get_audio_extraction_state'));
            ready.value = true;
        } catch (error) {
            stop();
            unlisten = undefined;
            throw error;
        }
    }

    async function command(name: string, args?: Record<string, unknown>) {
        if (!ready.value || disposed || busy.value) return false;
        submitting.value = true;
        try {
            merge(await bridge.invoke<AudioSnapshot>(name, args));
            return true;
        } catch (error) {
            // The native response can fail after a state event was lost; refresh real state.
            try {
                merge(await bridge.invoke<AudioSnapshot>('get_audio_extraction_state'));
            } catch { /* Preserve the original operation error. */
            }
            throw error;
        } finally {
            submitting.value = false;
        }
    }

    async function parse(path: string) {
        return command('parse_audio_source', {path});
    }

    async function extract() {
        if (!state.value.info || selectedTrack.value === undefined) return false;
        return command('extract_audio', {sourceId: state.value.info.id, trackIndex: selectedTrack.value});
    }

    function dispose() {
        disposed = true;
        ready.value = false;
        unlisten?.();
    }

    return {
        state, selectedTrack, track, submitting, ready, busy, connect, parse, extract,
        clear: () => command('clear_audio_source'), merge, dispose, desktop: bridge.desktop
    };
}

export function useAudioExtraction() {
    return createAudioExtraction({desktop: isTauri(), invoke, listen});
}
