// Formatting helpers. Dates follow the UTC "wall clock" convention (stored UTC is shown as is),
// and must match `src/http/views.rs`.

const UNITS = ['KB', 'MB', 'GB', 'TB', 'PB'];

/** 1024-based sizes like the Android app: "512 B", "1.2 MB", "15 GB". */
export function formatBytes(bytes) {
    if (Math.abs(bytes) < 1024) {
        return `${bytes} B`;
    }

    let value = bytes / 1024;
    let unit = 0;
    while (Math.abs(value) >= 1023.5 && unit < UNITS.length - 1) {
        value /= 1024;
        unit += 1;
    }

    return Math.abs(value) < 9.95
        ? `${value.toFixed(1)} ${UNITS[unit]}`
        : `${Math.round(value)} ${UNITS[unit]}`;
}

/** "1 photo" / "3 photos" */
export function plural(count, singular, pluralForm = `${singular}s`) {
    return `${count} ${count === 1 ? singular : pluralForm}`;
}

const monthLong = new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', month: 'long' });
const weekdayLong = new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC', weekday: 'long' });

const pad = (n) => String(n).padStart(2, '0');

/**
 * @param {number} unix seconds
 * @param {'monthHeader'|'monthWithYear'|'viewer'|'long'|'accessible'} style
 *   monthHeader: "June" in the current year, else "June 2022"; monthWithYear: "June 2024";
 *   viewer: "8 Jun 2024 · 14:03"; long: "Saturday, 8 June 2024 · 14:03"; accessible: "8 Jun 2024 14:03"
 * @param {number} now current time in ms, for the current year
 */
export function formatDate(unix, style, now = Date.now()) {
    const date = new Date(unix * 1000);
    const month = monthLong.format(date);
    const shortMonth = month.slice(0, 3);
    const year = date.getUTCFullYear();
    const day = date.getUTCDate();
    const time = `${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}`;

    switch (style) {
        case 'monthHeader':
            return year === new Date(now).getUTCFullYear() ? month : `${month} ${year}`;
        case 'monthWithYear':
            return `${month} ${year}`;
        case 'viewer':
            return `${day} ${shortMonth} ${year} · ${time}`;
        case 'long':
            return `${weekdayLong.format(date)}, ${day} ${month} ${year} · ${time}`;
        case 'accessible':
            return `${day} ${shortMonth} ${year} ${time}`;
        default:
            throw new Error(`Unknown date style: ${style}`);
    }
}
