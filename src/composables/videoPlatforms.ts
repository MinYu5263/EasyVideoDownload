export const platformIds = ["douyin", "bilibili", "youtube"] as const;
export type VideoPlatform = typeof platformIds[number];

const domains: Record<VideoPlatform, string[]> = {
    douyin: ["douyin.com"], bilibili: ["bilibili.com", "b23.tv"], youtube: ["youtube.com", "youtu.be"],
};

export function extractVideoLink(input: string): string | null {
    return input.match(/https?:\/\/[^\s<>"'“”，。！？；、（）)【】\]]+/i)?.[0] ?? null;
}

export function validateVideoLink(input: string, platform: VideoPlatform): { url: string; error: string | null } {
    const text = extractVideoLink(input);
    try {
        if (!text) return {url: "", error: "invalidLink"};
        const url = new URL(text);
        if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) return {
            url: "",
            error: "invalidLink"
        };
        const allowed = domains[platform].some(domain => url.hostname === domain || url.hostname.endsWith(`.${domain}`));
        return {url: url.href, error: allowed ? null : "platformMismatch"};
    } catch {
        return {url: "", error: "invalidLink"};
    }
}
