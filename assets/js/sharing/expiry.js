// Share expiry choices of the sharing dialog.

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * The expiry for a preset as unix seconds, `null` for "No expiry".
 * @param {'none'|'day'|'week'|'month'} preset
 * @param {number} now ms
 * @returns {number|null}
 */
export function presetExpiry(preset, now = Date.now()) {
    switch (preset) {
        case 'none':
            return null;
        case 'day':
            return Math.floor((now + DAY_MS) / 1000);
        case 'week':
            return Math.floor((now + 7 * DAY_MS) / 1000);
        case 'month': {
            const date = new Date(now);
            date.setUTCMonth(date.getUTCMonth() + 1);
            return Math.floor(date.getTime() / 1000);
        }
        default:
            throw new Error(`Unknown expiry preset: ${preset}`);
    }
}

/**
 * The expiry for a date picked in a date input (`YYYY-MM-DD`): the end of that day (UTC, like all
 * dates in the app), as unix seconds. `null` if the value isn't a date.
 * @returns {number|null}
 */
export function dateExpiry(value) {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null;
    const start = Date.parse(`${value}T00:00:00Z`);
    return Number.isNaN(start) ? null : Math.floor((start + DAY_MS) / 1000) - 1;
}
