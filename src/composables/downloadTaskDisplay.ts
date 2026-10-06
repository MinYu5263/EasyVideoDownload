export function formatDownloadSpeed(bytesPerSecond: number | null | undefined): string | null {
    if (bytesPerSecond == null || !Number.isFinite(bytesPerSecond) || bytesPerSecond < 0 || bytesPerSecond > Number.MAX_SAFE_INTEGER) return null;
    const units = ['B/s', 'kB/s', 'MB/s', 'GB/s', 'TB/s', 'PB/s'];
    let speed = bytesPerSecond;
    let unit = 0;
    // Promote values that round to 1000.00 as well as exact unit boundaries.
    while (Number(speed.toFixed(2)) >= 1000 && unit < units.length - 1) {
        speed /= 1000;
        unit += 1;
    }
    return `${speed.toFixed(2)} ${units[unit]}`;
}

export function formatDownloadEta(seconds: number | null | undefined, translate: (key: string, args: {count: number}) => string): string | null {
    if (seconds == null || !Number.isFinite(seconds) || seconds < 0 || seconds > Number.MAX_SAFE_INTEGER) return null;
    const total = Math.ceil(seconds);
    const units = [
        {seconds: 86400, key: 'tasks.time.days'},
        {seconds: 3600, key: 'tasks.time.hours'},
        {seconds: 60, key: 'tasks.time.minutes'},
        {seconds: 1, key: 'tasks.time.seconds'},
    ];
    const index = units.findIndex(unit => total >= unit.seconds);
    const first = index === -1 ? units.length - 1 : index;
    const primary = units[first]!;
    const parts = [translate(primary.key, {count: Math.floor(total / primary.seconds)})];
    const secondary = units[first + 1];
    if (secondary) {
        const count = Math.floor((total % primary.seconds) / secondary.seconds);
        if (count > 0) parts.push(translate(secondary.key, {count}));
    }
    return parts.join(' ');
}
