// Accent- and case-insensitive folder name search.

/** "Vacanță" → "vacanta" */
export function normalize(text) {
    return text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
}

/** Whether `name` matches the search `query`; an empty query matches everything. */
export function matchesQuery(name, query) {
    return normalize(name).includes(normalize(query.trim()));
}
