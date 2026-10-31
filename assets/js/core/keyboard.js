// Keyboard shortcuts with scopes. Scopes form a stack on top of the always present "global" one;
// a key is looked up from the top scope down, stopping at the first modal scope (dialog, viewer),
// so shortcuts of the page underneath don't fire while it's open.
//
// Keys: "k", "?", "+", "shift+a", "ctrl+k", or sequences like "g p" (typed within 1 second).
// Events coming from text fields are ignored. A handler returning false declines the key.

const SEQUENCE_TIMEOUT = 1000;

/** @type {Map<string, Map<string, (event: KeyboardEvent) => void | false>>} */
const handlers = new Map([['global', new Map()]]);
/** @type {{name: string, modal: boolean}[]} */
const stack = [{ name: 'global', modal: false }];
let pending = [];
let pendingTimer;
let listening = false;

function normalizeCombo(combo) {
    const parts = combo === '+' || combo.endsWith('++')
        ? [...combo.slice(0, -1).split('+').filter(Boolean), '+']
        : combo.split('+');
    const key = parts.pop();
    const mods = parts.map((m) => m.toLowerCase()).sort();
    return [...mods, key.length === 1 ? key.toLowerCase() : key].join('+');
}

/** The normalized combo for a keyboard event. Shift is implied by printable symbols like "?". */
export function eventCombo(event) {
    const key = event.key.length === 1 ? event.key.toLowerCase() : event.key;
    const mods = [];
    if (event.altKey) mods.push('alt');
    if (event.ctrlKey) mods.push('ctrl');
    if (event.metaKey) mods.push('meta');
    if (event.shiftKey && !(event.key.length === 1 && event.key.toLowerCase() === event.key.toUpperCase())) {
        mods.push('shift');
    }
    return [...mods.sort(), key].join('+');
}

function normalizeKeys(keys) {
    return keys.trim().split(/\s+/).map(normalizeCombo).join(' ');
}

function isTextField(target) {
    return target instanceof Element
        && (target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])') !== null);
}

function activeScopes() {
    const scopes = [];
    for (let i = stack.length - 1; i >= 0; i--) {
        scopes.push(stack[i].name);
        if (stack[i].modal) break;
    }
    return scopes;
}

/** The handlers of a key sequence, from the top scope down */
function findHandlers(sequence) {
    return activeScopes().flatMap((scope) => handlers.get(scope)?.get(sequence) ?? []);
}

function isPrefix(sequence) {
    const prefix = `${sequence} `;
    return activeScopes().some((scope) =>
        [...(handlers.get(scope)?.keys() ?? [])].some((keys) => keys.startsWith(prefix)));
}

function onKeyDown(event) {
    if (event.defaultPrevented || event.isComposing || isTextField(event.target)) return;
    if (['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return;

    clearTimeout(pendingTimer);
    pending.push(eventCombo(event));

    // Try the longest pending sequence first, then fall back to just the last key
    for (const sequence of [pending.join(' '), pending.at(-1)]) {
        // A handler returning false declines the key: the next one gets it
        const handler = findHandlers(sequence).find((candidate) => candidate(event) !== false);
        if (handler) {
            pending = [];
            event.preventDefault();
            return;
        }
        if (isPrefix(sequence)) {
            pending = sequence.split(' ');
            pendingTimer = setTimeout(() => { pending = []; }, SEQUENCE_TIMEOUT);
            return;
        }
    }
    pending = [];
}

/**
 * @returns {() => void} a function unregistering it
 */
export function register(scope, keys, handler) {
    if (!listening) {
        listening = true;
        document.addEventListener('keydown', onKeyDown);
    }
    if (!handlers.has(scope)) handlers.set(scope, new Map());
    const normalized = normalizeKeys(keys);
    handlers.get(scope).set(normalized, handler);
    return () => {
        if (handlers.get(scope)?.get(normalized) === handler) {
            handlers.get(scope).delete(normalized);
        }
    };
}

/**
 * Activates a scope on top of the current ones. Modal scopes hide the scopes below them; a
 * non-modal scope pushed while a modal one is open (a page mounting behind a deep-linked viewer)
 * goes below it, so it can't take keys from the modal scope.
 */
export function pushScope(name, { modal = false } = {}) {
    const firstModal = stack.findIndex((scope) => scope.modal);
    stack.splice(modal || firstModal < 0 ? stack.length : firstModal, 0, { name, modal });
}

/** Removes the topmost occurrence of a scope. */
export function popScope(name) {
    for (let i = stack.length - 1; i > 0; i--) {
        if (stack[i].name === name) {
            stack.splice(i, 1);
            return;
        }
    }
}
