export function formatVideoSize(bytes: number | null | undefined): string | null {
    if (bytes == null || !Number.isSafeInteger(bytes) || bytes <= 0) return null;
    const units = ["B", "kB", "MB", "GB", "TB", "PB"];
    let size = bytes;
    let unit = 0;
    while (size >= 1000 && unit < units.length - 1) {
        size /= 1000;
        unit += 1;
    }
    return `${size.toFixed(unit === 0 ? 0 : 1).replace(/\.0$/, "")} ${units[unit]}`;
}
