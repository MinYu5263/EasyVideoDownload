export interface LabError {
    code: string;
    detail: string
}

export interface LabFormat {
    id: string;
    width: number | null;
    height: number | null;
    codec: string;
    fps: number | null;
    bitrate: number | null;
    fileSize: number | null;
    watermarked: boolean | null;
}

export interface LabVideo {
    resultId: string;
    videoId: string;
    title: string;
    duration: number | null;
    cover: string | null;
    formats: LabFormat[];
}

export interface ObservedMedia {
    width: number;
    height: number;
    codec: string;
    duration: number | null;
    bitrate: number | null;
    fileSize: number;
}

export type LabPhase =
    "idle"
    | "parsing"
    | "ready"
    | "downloading"
    | "verifying"
    | "cancelling"
    | "completed"
    | "cancelled"
    | "failed";

export interface LabSnapshot {
    sessionId: string;
    revision: number;
    taskId: string | null;
    phase: LabPhase;
    parsed: LabVideo | null;
    receivedBytes: number;
    totalBytes: number | null;
    outputPath: string | null;
    observed: ObservedMedia | null;
    error: LabError | null;
    defaultDirectory: string;
    cookieConfigured: boolean;
    ffprobeAvailable: boolean;
}
