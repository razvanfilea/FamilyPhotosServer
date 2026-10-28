// User preferences, persisted in the `fp_prefs` cookie so the server renders the right state on
// first paint (parsed by `src/http/prefs.rs`). The value is a URL-encoded form string.

export const PREFS_COOKIE = 'fp_prefs';

export const PREF_DEFAULTS = Object.freeze({
    filter: 'all',
    zoom: '1',
    fsort: 'name_asc',
    fview: 'grid',
});

const ONE_YEAR = 60 * 60 * 24 * 365;

/** Reads the prefs out of a `document.cookie` style string, applying defaults. */
export function parsePrefs(cookieString) {
    const prefs = { ...PREF_DEFAULTS };
    const cookie = cookieString
        .split(';')
        .map((part) => part.trim())
        .find((part) => part.startsWith(`${PREFS_COOKIE}=`));

    if (cookie) {
        const params = new URLSearchParams(cookie.slice(PREFS_COOKIE.length + 1));
        for (const key of Object.keys(PREF_DEFAULTS)) {
            if (params.has(key)) prefs[key] = params.get(key);
        }
    }
    return prefs;
}

/** The cookie value for `prefs`, omitting defaults. */
export function serializePrefs(prefs) {
    const params = new URLSearchParams();
    for (const [key, value] of Object.entries(prefs)) {
        if (key in PREF_DEFAULTS && String(value) !== PREF_DEFAULTS[key]) {
            params.set(key, String(value));
        }
    }
    return params.toString();
}

/** @returns {string} */
export function getPref(key) {
    return parsePrefs(document.cookie)[key];
}

export function setPref(key, value) {
    if (!(key in PREF_DEFAULTS)) {
        throw new Error(`Unknown pref: ${key}`);
    }
    const prefs = { ...parsePrefs(document.cookie), [key]: String(value) };
    document.cookie = `${PREFS_COOKIE}=${serializePrefs(prefs)}; Path=/; Max-Age=${ONE_YEAR}; SameSite=Lax`;
}
