# WP9 — Native feel (desktop and Android)

**After WP8 and the htmx 4 upgrade** (9.3 relies on htmx 4's `hx-boost`). Goal: the website should
feel like a native photos app — instant start, in-place navigation, smooth scrolling, gestures that
follow the finger, no reload flashes — in every browser and when installed.

**Priority (owner, session 7):** the native feel comes first, for **desktop and Android browsers**
(Chromium and Firefox, which the owner uses). **iOS-specific work is WP10** (`wp10-ios.md`), which may
be delayed (the owner has no iPhone to test on); WebKit/Mobile Safari runs are not part of WP9's checks.
Order: 9.3 navigation → 9.2 service worker → 9.4 gestures → 9.5 smoothness → 9.1 installable →
9.6 phone features. Test touch on a real Android phone (Chrome remote debugging).

## What already exists (don't redo)

- `assets/manifest.webmanifest` with `display: standalone`, icons 192/512, `apple-touch-icon`.
- Viewer: horizontal swipe between photos, swipe down to close (`viewer/math.js` `swipeOutcome`),
  zoom, View Transitions when opening/closing (`viewer.js`, respects reduced motion).
- Long-press drag-select (`grid/selection-drag.js`), the `contextmenu` suppression on tiles.
- Safe-area insets on the bottom nav and FAB, `100dvh` for the shell (`templates/input.css`).
- Web Share for photos (viewer, selection bar), thumbhash placeholders, month-section virtualisation.
- Sessions last 60 days from the last day of use (`main`: `http/session_activity.rs`).

## Tasks

### 9.1 Installable app, standalone mode
- Manifest: add `id`, `orientation: any`, maskable icons, `screenshots` (optional), and check
  `theme_color`/`background_color` against both themes.
- Launch: splash/background colour matches the shell so there's no white flash; the first paint is the
  shell, not a blank page.
- Standalone has **no browser back button and no pull-to-refresh**: every non-tab page (folder,
  utilities sub-pages, settings, upload) needs an in-app back control, and the timeline/folder pages
  get a pull-to-refresh gesture (only in standalone, `display-mode: standalone`).
- External links (if any) open outside the app; in-app links stay in it (`scope`).
- Safe areas everywhere: viewer top/bottom bars, dialogs as bottom sheets, toasts, selection bar,
  month picker; landscape on notched phones (left/right insets).

### 9.2 Instant start: service worker
- `assets/sw.js` registered from `app.js`, scope `/`. Strategy:
  - App shell (CSS, JS modules, icons, fonts, vendor): precache per `ASSET_VERSION`, drop old caches on
    activate. JS is served `no-cache` today because module imports can't carry `?v=`: the SW makes
    that cheap (serve from cache, revalidate in the background, swap on next launch).
  - Previews (`/api/preview/{id}`): cache-first with a size cap (LRU by count, e.g. 5 000 entries);
    they're keyed by id, so the cache must be cleared when a photo is replaced or deleted
    (`fp:photos-removed` → `postMessage` to the SW).
  - HTML pages and `/ui/*` fragments: network-first (always fresh), with an offline fallback page.
  - Never cache `/api/*` JSON, uploads, downloads, `/api/screen` beyond the HTTP cache, or anything
    with `Set-Cookie`.
- Update flow: a new SW waits; show "Update available — reload" as a toast instead of swapping under
  the user.
- Must not break: session redirects (401 → login), `HX-Redirect`, the upload page's CSP and worker.

### 9.3 App-like navigation
- Switch tab navigation to **in-place swaps** (HTMX `hx-boost` on the shell nav, today
  `hx-boost="false"`), so the shell, scroll positions per tab and the nav stay mounted. Requires:
  every page's scripts mount/unmount cleanly through the component registry (they should already),
  `<title>` and active-tab state updated, history and deep links (`?photo=`) correct, and the
  per-page CSP (upload) still applied — if upload can't be boosted, keep it a full load.
- **View Transitions** between tabs and into folder pages (cross-fade + slide like iOS), with
  `prefers-reduced-motion` respected.
- Restore the scroll position per tab when coming back (timeline, folders).
- Bottom nav: active-tab tap scrolls to top (iOS convention); a second tap on Photos goes to "today".

### 9.4 Gestures (Photos.app parity)
- Viewer: swipe-down-to-close **follows the finger** (image scales/fades with the drag, background
  dims) and settles with spring-like easing; horizontal paging follows the finger with momentum;
  fix the known swipe-velocity issue (`viewer/math.js` `swipeOutcome` averages the velocity over the whole
  gesture instead of measuring it at release, so a slow drag ending in a flick only navigates past 25 % of the width).
- Pinch and double-tap zoom anchored at the touch point; pan when zoomed; no conflict with paging.
- Grid: pinch on the grid changes zoom level (`fp:grid-zoom`), like Photos.app.
- Long-press a tile → selection mode (already) with a small scale feedback; drag extends it.
- Edge-swipe back on sub-pages where it doesn't fight Safari's own gesture (standalone only).
- `touch-action` set per surface (viewer image `none`, buttons `manipulation`, grid `pan-y`), no
  accidental text selection or callouts (`-webkit-touch-callout: none` on tiles/UI chrome).

### 9.5 Smoothness and image quality
- Profile scrolling on a phone and on desktop: target no dropped frames during fling scrolls and
  fast-scroller drags. Animate only `transform`/`opacity`; passive listeners; batch DOM writes in
  `requestAnimationFrame`; `content-visibility: auto` + `contain-intrinsic-size` on far sections.
- Memory: keep decoded images bounded (unloading far sections already helps); check that Safari
  doesn't kill the tab on a 200 000-photo library (WP8's seed).
- **Sharper thumbnails:** a 3-column grid on a 3× phone needs about 390 px; previews are 320 px
  (`previews/generate.rs` `PREVIEW_SIZE`). Options for the owner: a 2nd larger preview size with
  `srcset`, or raise the preview size (all previews regenerate; disk + CPU cost). Measure first.
- Viewer: preload the next/previous screen renditions; decode before swapping
  (`img.decode()`) so paging never shows a blank frame.
- Video: `playsinline`, poster from the preview.

### 9.6 Phone features
- Upload: the file input opens the photo library (`accept="image/*,video/*" multiple`); keep the
  screen awake during uploads (Screen Wake Lock API, released when the queue is done or the page is
  hidden); resume the queue cleanly after the app was backgrounded; HEIC uploads keep working.
- Share: Web Share already used; make sure it works for several photos and videos (file size
  limits; fall back to sharing links).
- Dark mode follows the system by default; status bar colour matches.
- Typography: system font stack; 44 px minimum touch targets.

### 9.7 Optional (ask the owner before starting each)
- **Web Push** (Chrome/Firefox/Android; iOS 16.4+ installed apps only): "Alice shared 12 photos with you", "new photos in a
  shared folder". Needs VAPID keys, a subscriptions table (**migration**), a sender in the server and a
  settings toggle.
- **Passkeys** (WebAuthn, Face ID login) next to passwords. Needs a credentials table (**migration**)
  and changes to the login flow; `/api/login` for Android must stay as is.
- A minimal offline mode: recently viewed photos and the last timeline page available offline.

## Owns

`assets/sw.js`, `assets/manifest.webmanifest`, `templates/base.html`/`shell.html` head and nav changes,
gesture code in `viewer/` and `grid/`, `playwright/specs/pwa.spec.ts` (manifest, SW registration and
caching rules, standalone-only UI via `emulateMedia`/`display-mode`), `docs/web-ui-plan/handoff/wp9.md`.

## Rules

- Same as every package: no `/api` change, no migration without the owner's OK (9.7 needs both),
  CSP stays strict (no inline scripts; the SW is a same-origin file), desktop and Android behaviour
  unchanged unless it's a clear improvement.
- Every gesture has a non-gesture equivalent (buttons, keyboard) for accessibility.
- Keep `check.sh --firefox` green. WebKit/Mobile Safari runs belong to WP10.

## Acceptance criteria

- Installed (Chrome desktop and Android): opens full-screen with no white flash, safe areas correct in
  portrait and landscape, back and pull-to-refresh available in-app. (iPhone: WP10.)
- Second launch shows the timeline from cache in < 1 s on a phone; app shell works offline with an
  offline page; a deploy shows "Update available" instead of breaking the running app.
- Tab switches don't reload the shell and keep scroll positions; transitions respect reduced motion.
- Viewer gestures follow the finger (paging, swipe-down, pinch, double-tap) without jank on a real
  Android phone and with a trackpad on desktop; grid pinch-to-zoom works.
- No dropped-frame hitches in a fling scroll through the 200 000-photo seed on a mid-range Android phone
  (recorded with Chrome DevTools' performance panel), no tab crash from memory.
- Uploads keep the screen awake and resume after backgrounding.
- `pwa.spec.ts` and the full suite green on Chromium and Firefox (known failures only).
