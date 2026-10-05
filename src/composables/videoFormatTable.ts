import type {VideoFormat} from './useVideoParser.ts';

// Business labels and preference order come from Rust, including legacy records.
export function videoCodecLabel(format?: VideoFormat | null): string | null {
    return format?.codecLabel ?? null;
}

export function videoQualityLabel(format?: VideoFormat | null): string | null {
    return format?.qualityLabel ?? null;
}
