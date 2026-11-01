// Service worker. `/sw.js` is this file with the two placeholders below filled in by the server
// (src/http/service_worker.rs), so a new asset version is a new worker.
//
// - App shell: every asset is precached per version and served from that cache, so a page's CSS and
//   modules never come from two different versions. A new worker waits until the page asks it to take
//   over (the "Update available" toast).
// - Previews: cache-first, oldest entries dropped past a cap. They hold a user's photos, so the
//   cache is emptied on sign-out and whenever the login page is opened.
// - Pages and `/ui/*`: always from the network; only a failed navigation shows the offline page.
// - Everything else (`/api/*`, uploads, downloads, renditions) is not touched.

const VERSION = '__ASSET_VERSION__';
const PRECACHE = __PRECACHE_PATHS__;

const OFFLINE_PAGE = '/assets/offline.html';
const SHELL_CACHE = `fp-shell-${VERSION}`;
const PREVIEW_CACHE = 'fp-previews';
const PREVIEW_PREFIX = '/api/preview/';
const PREVIEW_LIMIT = 5000;
// Listing a cache with thousands of entries isn't free, so the cap is enforced every few insertions
const TRIM_EVERY = 50;

// A development server serves assets that change without a new version: only the offline page is cached
const cacheAssets = VERSION !== 'dev';
let insertions = 0;

self.addEventListener('install', (event) => {
    event.waitUntil(
        caches.open(SHELL_CACHE).then((cache) =>
            // `reload`: don't fill the cache from the HTTP cache of an older version
            cache.addAll(PRECACHE.map((path) => new Request(path, { cache: 'reload' }))),
        ),
    );
});

self.addEventListener('activate', (event) => {
    event.waitUntil(
        (async () => {
            const names = await caches.keys();
            await Promise.all(names.filter((name) => name.startsWith('fp-shell-') && name !== SHELL_CACHE).map((name) => caches.delete(name)));
            // Lets the server start answering the navigation while this worker boots
            await self.registration.navigationPreload?.enable();
            await self.clients.claim();
        })(),
    );
});

self.addEventListener('message', (event) => {
    const { type, ids } = event.data ?? {};
    if (type === 'skip-waiting') {
        self.skipWaiting();
    } else if (type === 'forget-previews' && Array.isArray(ids)) {
        event.waitUntil(
            caches.open(PREVIEW_CACHE).then((cache) => Promise.all(ids.map((id) => cache.delete(`${PREVIEW_PREFIX}${id}`)))),
        );
    }
});

self.addEventListener('fetch', (event) => {
    const { request } = event;
    if (request.method !== 'GET') return;
    const url = new URL(request.url);
    if (url.origin !== location.origin) return;

    if (request.mode === 'navigate') {
        event.respondWith(navigate(event, url));
    } else if (url.pathname.startsWith(PREVIEW_PREFIX)) {
        // A range request is a video; previews are whole images
        if (!request.headers.has('range')) event.respondWith(preview(event));
    } else if (url.pathname.startsWith('/assets/') && cacheAssets) {
        event.respondWith(asset(request, url));
    }
});

async function navigate(event, url) {
    // Whoever signs in next must not see the previous user's photos
    if (url.pathname === '/login') event.waitUntil(caches.delete(PREVIEW_CACHE));
    try {
        return (await event.preloadResponse) ?? (await fetch(event.request));
    } catch {
        return (await caches.match(OFFLINE_PAGE)) ?? Response.error();
    }
}

async function asset(request, url) {
    // The page asks for a version this worker doesn't have yet (a deploy it hasn't taken over): the network knows it
    const version = url.searchParams.get('v');
    if (version !== null && version !== VERSION) return fetch(request);
    // Stored without the query: `?v=` only busts the HTTP cache
    return (await caches.match(url.pathname, { cacheName: SHELL_CACHE })) ?? fetch(request);
}

async function preview(event) {
    const { request } = event;
    const cache = await caches.open(PREVIEW_CACHE);
    const cached = await cache.match(request);
    if (cached) return cached;

    // Revalidate: the HTTP cache may hold a preview of an older photo that had this id
    const response = await fetch(request, { cache: 'no-cache' });
    // The original standing in for a preview that couldn't be made is `no-store`: not a preview
    if (response.status === 200 && !response.headers.get('cache-control')?.includes('no-store')) {
        event.waitUntil(store(cache, request, response.clone()));
    }
    return response;
}

async function store(cache, request, response) {
    await cache.put(request, response);
    if (++insertions % TRIM_EVERY !== 0) return;
    const keys = await cache.keys();
    // Oldest first: `keys()` is in insertion order
    await Promise.all(keys.slice(0, Math.max(0, keys.length - PREVIEW_LIMIT)).map((key) => cache.delete(key)));
}
