// The texts of the selection bar, its actions and the move dialog live in
// templates/selection/bar.html (`[data-selection-strings]`), so they stay out of the scripts.
// A text is a string, or [singular, plural] chosen by `count`; `{name}` is replaced by `vars`.

let strings;

/**
 * @param {string} key
 * @param {Record<string, string | number>} [vars]
 * @returns {string}
 */
export function s(key, vars = {}) {
    strings ??= JSON.parse(globalThis.document?.querySelector('[data-selection-strings]')?.textContent ?? '{}');
    const entry = strings[key] ?? key;
    const text = Array.isArray(entry) ? entry[vars.count === 1 ? 0 : 1] : entry;
    return text.replace(/\{(\w+)\}/g, (match, name) => String(vars[name] ?? match));
}
