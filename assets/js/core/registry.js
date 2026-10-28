// Component registry: markup declares `data-component="name other"`, modules register a mount
// function per name. Components are mounted once per element on page load and whenever HTMX
// inserts content, and their cleanup runs when HTMX removes the element.

const factories = new Map();
/** @type {WeakMap<Element, Map<string, (() => void) | undefined>>} */
const mounted = new WeakMap();
let booted = false;

/**
 * @param {string} name
 * @param {(element: HTMLElement) => (() => void) | void} factory returns an optional cleanup
 */
export function register(name, factory) {
    if (factories.has(name)) {
        throw new Error(`Component "${name}" is already registered`);
    }
    factories.set(name, factory);
    if (booted) {
        mount(document.body);
    }
}

function mountElement(element) {
    const names = element.dataset.component.split(/\s+/).filter(Boolean);
    let instances = mounted.get(element);

    for (const name of names) {
        const factory = factories.get(name);
        if (!factory || instances?.has(name)) continue;

        if (!instances) {
            instances = new Map();
            mounted.set(element, instances);
        }
        instances.set(name, factory(element) ?? undefined);
    }
}

/** Mounts the registered components on `root` and its descendants. */
export function mount(root) {
    if (!(root instanceof Element)) return;
    if (root.matches('[data-component]')) mountElement(root);
    root.querySelectorAll('[data-component]').forEach(mountElement);
}

function unmountElement(element) {
    const instances = mounted.get(element);
    if (!instances) return;
    mounted.delete(element);
    for (const cleanup of instances.values()) {
        cleanup?.();
    }
}

/** Runs the cleanups of `root` and its descendants. Call it before removing elements without HTMX. */
export function unmount(root) {
    if (!(root instanceof Element)) return;
    root.querySelectorAll('[data-component]').forEach(unmountElement);
    unmountElement(root);
}

export function boot() {
    if (booted) return;
    booted = true;

    mount(document.body);
    document.addEventListener('htmx:load', (event) => mount(event.detail.elt));
    document.addEventListener('htmx:afterSettle', (event) => mount(event.detail.elt));
    // HTMX fires this for every element it removes, descendants included
    document.addEventListener('htmx:beforeCleanupElement', (event) => unmountElement(event.detail.elt));
}
