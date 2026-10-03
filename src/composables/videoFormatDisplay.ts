export function formatVideoSize(bytes: number | null | undefined): string | null {
    if (bytes == null || !Number.isSafeInteger(bytes) || bytes <= 0) return null;
    const units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let size = bytes;
    let unit = 0;
    while (size >= 1024 && unit < units.length - 1) {
        size /= 1024;
        unit += 1;
    }
    return `${size.toFixed(unit === 0 ? 0 : 1).replace(/\.0$/, "")} ${units[unit]}`;
}
