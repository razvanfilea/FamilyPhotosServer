// Ambient declarations for the frontend JS (checked by `npm run typecheck`, see jsconfig.json).
// Types only: nothing here ships to the browser.

/** The slice of HTMX the code uses (the vendored build is `assets/vendor/htmx.min.js`). */
interface Htmx {
    ajax(verb: string, path: string, context?: string | Element | Record<string, unknown>): Promise<void>;
    process(element: Element): void;
    remove(element: Element): void;
}

interface Window {
    htmx?: Htmx;
}

/** Detail of the `htmx:*` events the code listens to. */
interface HtmxEventDetail {
    elt: Element;
    xhr: XMLHttpRequest;
    parameters: FormData;
    [key: string]: any;
}

// Custom events: `htmx:*` and the `fp:*` ones of assets/js/core/events.js (data in `detail`)
interface GlobalEventHandlersEventMap {
    'htmx:afterSettle': CustomEvent<HtmxEventDetail>;
    'htmx:afterSwap': CustomEvent<HtmxEventDetail>;
    'htmx:beforeCleanupElement': CustomEvent<HtmxEventDetail>;
    'htmx:configRequest': CustomEvent<HtmxEventDetail>;
    'htmx:load': CustomEvent<HtmxEventDetail>;
    'htmx:responseError': CustomEvent<HtmxEventDetail>;
    'htmx:sendError': CustomEvent<HtmxEventDetail>;
    'fp:destination-change': CustomEvent<any>;
    'fp:flash-toast': CustomEvent<any>;
    'fp:folder-filter-change': CustomEvent<any>;
    'fp:grid-zoom': CustomEvent<any>;
    'fp:photo-activate': CustomEvent<any>;
    'fp:photos-added': CustomEvent<any>;
    'fp:photos-changed': CustomEvent<any>;
    'fp:photos-removed': CustomEvent<any>;
    'fp:section-loaded': CustomEvent<any>;
    'fp:selection-change': CustomEvent<any>;
    'fp:toast': CustomEvent<any>;
    'fp:viewer-closed': CustomEvent<any>;
}

// The pages query elements their own templates render, and the code relies on knowing their exact
// kind (`.value` of an input, `.open` of a dialog, `.content` of a template). Selectors that aren't
// a known tag name therefore give `any` instead of a bare Element; pass a type argument to be exact.
// The tag-name overloads are repeated because overloads of the last declaration win.
interface Element {
    closest<K extends keyof HTMLElementTagNameMap>(selectors: K): HTMLElementTagNameMap[K] | null;
    closest<E extends Element = any>(selectors: string): E | null;
}

interface ParentNode {
    querySelector<K extends keyof HTMLElementTagNameMap>(selectors: K): HTMLElementTagNameMap[K] | null;
    querySelector<E extends Element = any>(selectors: string): E | null;
    querySelectorAll<K extends keyof HTMLElementTagNameMap>(selectors: K): NodeListOf<HTMLElementTagNameMap[K]>;
    querySelectorAll<E extends Element = any>(selectors: string): NodeListOf<E>;
}

/** The API a module hangs on its element (`element.fpGrid`, `element.fpPicker`). */
interface HTMLElement {
    fpGrid?: any;
    fpPicker?: any;
    fpSelection?: any;
}

/** Loaded by `assets/vendor/hash-wasm/blake3.umd.min.js` in the hash worker. */
declare var hashwasm: {
    createBLAKE3(): Promise<{
        init(): void;
        update(data: Uint8Array): void;
        digest(encoding: 'hex'): string;
    }>;
};
