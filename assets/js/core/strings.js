// User-visible strings needed by JS live in templates/base.html (#app-strings) so they stay
// out of the scripts and can be translated later.

let strings;

/** @returns {string} */
export function t(key) {
    strings ??= JSON.parse(globalThis.document?.getElementById('app-strings')?.textContent ?? '{}');
    return strings[key] ?? key;
}
