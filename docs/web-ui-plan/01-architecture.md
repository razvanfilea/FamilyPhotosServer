# 01 — Target architecture and shared contracts

Everything in this file is a **contract** between work packages. If a WP needs to change a
contract, it must record the change in its hand-off note and keep the old behaviour working for
packages that were already merged.

## 1. Stack

- **Server-rendered HTML** with Askama templates for every page and fragment.
- **HTMX 4** (self-hosted at `assets/vendor/htmx.min.js`, no CDN) for fragment loading, forms and
  dialogs.
- **Vanilla ES modules** (`<script type="module">`) for the interactive parts: grid, fast scroller,
  selection, viewer, upload. No bundler, no framework, no npm runtime dependencies. Third-party code
  is vendored under `assets/vendor/` with its license file next to it.
- **Tailwind CSS 4 + DaisyUI 5.** Only DaisyUI 5 variables (`--color-primary`, `--color-base-100`, …)
  are allowed; DaisyUI 4 variables (`--p`, `--bc`, `--b3`, …) are banned.
- **JSON API** (`/api/...`) only where JavaScript needs data rather than HTML (viewer neighbors, photo
  info, bulk mutations, hash checks).

## 2. URL scheme (page router, all require login except `/login`)

| URL | Page |
|---|---|
| `/` | Timeline (Photos tab). `?filter=all\|personal\|family\|shared` overrides the saved pref. |
| `/folders` | Folders tab. `?q=` search, `?filter=`, `?sort=`, `?view=grid\|list` override prefs. |
| `/folders/{id}` | Folder page |
| `/favorites` | Favorites (a "folder" whose source is the favourites table) |
| `/utilities` | Utilities hub (statistics card + tool buttons) |
| `/trash` | Trash |
| `/duplicates` | Duplicates |
| `/large-files` | Large files |
| `/settings` | Settings |
| `/upload` | Upload page (`?folder_id=` preselects a destination) |
| `/login` | Login (`?next=` honoured after success; only same-origin relative paths) |
| any page + `?photo={id}` | The same page with the viewer open on photo `{id}` (deep link). |

Unknown paths render `templates/errors/404.html` via a router fallback (full page, inside the shell).

Fragment routes live under `/ui/...` (page router, return HTML fragments, used only by HTMX/JS):
`/ui/grid/...`, `/ui/dialogs/...`, `/ui/folders/...`, etc. Each WP lists its own. Fragment routes
return **401 with header `HX-Redirect: /login?next=<current page>`** when the session is missing,
instead of a 302 (so HTMX never swaps the login page into a fragment target). WP0 implements this as
a shared helper.

The old routes (`/gallery/*`, `/folder/{id}`, `/photo/*`, `/favorite/*`, `/trash/*` mutations,
`GET /api/folders` in the page router) are **deleted** by WP0. `/folder/{id}` gets a permanent
redirect to `/folders/{id}` for old bookmarks.

## 3. Templates layout

```
templates/
  base.html                 # <html>, head, theme, assets, toast region, confirm dialog, modal root
  shell.html                # extends base: nav rail/sidebar (desktop) + bottom nav (mobile) + <main>
  macros/
    icons.html              # icon(name, filled=false, class="")
    photo.html              # photo_tile(...) — THE tile macro (see §8)
    ui.html                 # chips, segmented control, empty_state, badge, avatar, etc.
  errors/404.html
  user/login_page.html, user/login_error.html
  timeline/…  folders/…  favorites/…  trash/…  utilities/…  duplicates/…  large_files/…
  settings/…  upload/…  viewer/…  dialogs/…  grid/…
  input.css
```

Every page template extends `shell.html` and sets `{% block title %}`, the active nav item
(`nav_active`), and `{% block content %}`. Page-specific scripts go in `{% block scripts %}` as
`<script type="module" src="/assets/js/pages/<page>.js?v={{ asset_version }}">`.

## 4. Asset versioning and caching

- Extend `build.rs` so `ASSET_VERSION` hashes **all files** under `assets/css`, `assets/js`,
  `assets/vendor`, `assets/icons` in release builds (`"dev"` in debug). Keep `CSS_VERSION` as an alias
  or replace its usages. Templates get it as `asset_version`.
- Static assets are served with `Cache-Control: public, max-age=31536000, immutable` when requested
  with `?v=<hash>` in release builds, and `no-cache` otherwise (dev).
- ES module imports inside JS can't carry `?v=`: `app.js` imports use relative paths; set
  `Cache-Control: no-cache` on `/assets/js/**` (so the browser revalidates with an ETag/Last-Modified,
  which `ServeDir` provides) and immutable caching only on CSS, fonts, icons and vendor files. This
  keeps module graphs consistent after deploys.

## 5. JS architecture

```
assets/js/
  app.js                    # entry point, loaded on every page. Imports core, registers components, boots.
  core/
    registry.js             # component registry (see below)
    api.js                  # fetch wrapper: JSON in/out, 401 → redirect to /login?next=, error toast on failure
    toast.js                # toast(message, {type: 'success'|'error'|'info', action?: {label, onClick}, timeout?})
    dialog.js               # confirm({title, body (html string or Node), confirmLabel, danger, onConfirm: async () => {}}) → Promise<boolean>
    prefs.js                # get/set user prefs (cookie, see §6)
    events.js               # typed CustomEvent helpers (see §9)
    format.js               # date/size formatting (UTC convention), plural helper
    keyboard.js             # global shortcut registry with scopes (grid, viewer, dialog)
  vendor-wrappers/thumbhash.js  # ESM wrapper around the vendored decoder
  grid/ …                   # WP2 + WP4
  viewer/ …                 # WP3
  folders/ …, sharing/ …, destination-picker/ … # WP5
  upload/ …                 # WP7
  pages/ …                  # thin per-page entry modules
assets/vendor/
  htmx.min.js (+ LICENSE), thumbhash.js (+ LICENSE), blake3 (hash-wasm, + LICENSE, WP7)
```

**Component registry** (`core/registry.js`): markup declares `data-component="name"` (optionally
several, space-separated). `register(name, (element) => { …; return cleanup? })`. On
`DOMContentLoaded` and on every `htmx:after:process`, the registry mounts components on new
elements (once per element, tracked in a WeakMap). When HTMX removes an element
(a MutationObserver sees it leave the document), its cleanup runs. **No inline `onclick`, no globals on `window`** (the
only allowed global is `window.htmx` from the vendor script).

**Error handling (WP0):**
- `htmx:response:error` / `htmx:error` → error toast ("Something went wrong" / "Failed to connect
  to server"), unless the element has `data-silent-errors`.
- HTTP 401 on any HTMX or `api.js` request → `location = /login?next=<current path+query>`.
- The server's 400 text bodies are shown in the toast (they're user-facing messages); 500 bodies are
  not (generic message).

## 6. User preferences

Persisted in **one cookie** `fp_prefs` (Path=/, SameSite=Lax, 1 year, not HttpOnly so JS can update
it), value = URL-encoded form string, e.g. `filter=all&zoom=1&fsort=name_asc&fview=grid`. A Rust
extractor `Prefs` (WP0, `src/http/prefs.rs`) parses it with defaults so the **server renders the
right state on first paint** (no flash).

| Key | Values | Default | Used by |
|---|---|---|---|
| `filter` | `all`, `personal`, `family`, `shared` | `all` | Timeline, Folders tab, destination picker |
| `zoom` | `0`–`3` | `1` | All photo grids |
| `fsort` | `date_desc`, `name_asc`, `name_desc`, `count_desc` | `name_asc` | Folders tab, destination picker |
| `fview` | `grid`, `list` | `grid` | Folders tab, destination picker |

`core/prefs.js` exposes `getPref(key)`, `setPref(key, value)` (rewrites the cookie). Pages that
change a pref update the URL/state locally and set the cookie; no server round trip needed just to
save a pref.

## 7. Theme, typography, icons

- One DaisyUI 5 custom theme `familyphotos`, dark, set as default (`data-theme="familyphotos"` on
  `<html>`), built from the Android Material palette:

  | DaisyUI | Android role | Hex |
  |---|---|---|
  | `--color-primary` / `primary-content` | primary / onPrimary | `#78DC77` / `#00390A` |
  | `--color-secondary` / content | primaryContainer / onPrimaryContainer | `#005313` / `#94F990` |
  | `--color-accent` / content | tertiaryContainer / onTertiaryContainer | `#1F4D52` / `#BCEBF0` |
  | `--color-neutral` / content | surfaceVariant / onSurfaceVariant | `#424940` / `#C2C9BD` |
  | `--color-base-100` | background/surface | `#1A1C19` |
  | `--color-base-200` | slightly lighter surface | `#222520` |
  | `--color-base-300` | surface container high | `#2C302A` |
  | `--color-base-content` | onSurface | `#E2E3DD` |
  | `--color-error` / content | error / onError | `#FFB4AB` / `#690005` |
  | `--color-warning` | secondary (yellow) | `#DBC900` |
  | `--color-info` | tertiary | `#A0CFD4` |
  | `--color-success` | primary | `#78DC77` |

  Extra custom properties: `--fp-outline: #8C9388`, `--fp-error-container: #93000A`,
  `--fp-on-error-container: #FFDAD6`, `--fp-inverse-surface: #E2E3DD`, `--fp-inverse-on-surface: #1A1C19`.
  Toasts use Android's colours: success = secondary (primaryContainer), error = error-container,
  info = inverse surface.
- Font: system font stack (`ui-sans-serif, system-ui, …`); no Google Fonts.
- **Icons: Material Symbols Rounded**, self-hosted as an **SVG sprite** at `assets/icons/sprite.svg`,
  generated from the npm dev-dependency `@material-symbols/svg-400` by a script
  `scripts/build-icons.mjs` (reads the icon list from `scripts/icons.txt`, writes the sprite with one
  `<symbol id="name">` and one `<symbol id="name-fill">` per icon). Add `npm run icons`. The macro
  `icon(name, filled=false, class="")` renders
  `<svg class="icon {class}" aria-hidden="true" focusable="false"><use href="/assets/icons/sprite.svg?v=…#name[-fill]"/></svg>`.
  Icon-only buttons must have `aria-label`. This removes the ligature-font problems (unfilled hearts,
  flash of ligature text, screen readers reading "chevron_left").

## 8. The photo tile contract

One Askama macro renders every photo tile in every grid (`templates/macros/photo.html`):

```jinja
{% macro photo_tile(photo, index, badge="") %}
<button type="button"
        class="photo-tile"
        data-photo-id="{{ photo.id }}"
        data-index="{{ index }}"           {# position in the context's full ordering #}
        data-video="{{ photo.is_video }}"
        data-deletable="{{ photo.deletable }}"  {# user has Delete access; gates bulk actions #}
        data-testid="photo-tile"
        aria-label="{{ photo.aria_label }}"  {# e.g. "Photo, 8 Jun 2024 14:03" / "Video, …" #}
        style="view-transition-name: none">
  <canvas class="photo-tile__hash" data-thumbhash="{{ photo.thumb_hash_b64 }}" aria-hidden="true"></canvas>
  <img class="photo-tile__img" src="/api/preview/{{ photo.id }}" loading="lazy" decoding="async" alt="">
  {% if photo.is_video %}<span class="photo-tile__video">{{ icon("play_circle", true) }}</span>{% endif %}
  <span class="photo-tile__check" aria-hidden="true">{{ icon("radio_button_unchecked") }}{{ icon("check_circle", true) }}</span>
  {% if badge != "" %}<span class="photo-tile__badge">{{ badge }}</span>{% endif %}
</button>
{% endmacro %}
```

- The view model is `PhotoTileView { id, is_video, deletable, thumb_hash_b64: String (standard
  base64, empty if none), aria_label, created_at_unix }`, built by one Rust function `PhotoTileView::from_photo(&Photo)`
  (WP0, `src/http/views.rs`). Video detection uses one shared function `utils::media::is_video(name)`
  (WP1) with the extension list: `3gp 3g2 mp4 m4v mov mkv webm avi mpg mpeg ts mts m2ts flv wmv ogv`.
- Native `loading="lazy"` replaces the old JS lazy loader. The thumbhash canvas is decoded **when the
  tile scrolls near the viewport** (one shared IntersectionObserver in `grid/thumbhash-observer.js`),
  and hidden once the `<img>` fires `load`.
- Selection state is expressed only through classes/attributes that WP4 toggles:
  `.is-selected` + `aria-pressed="true"` on the tile, and `.selecting` on the grid root. Styling for
  these ships with WP2's CSS so WP4 only toggles state.
- Clicking a tile dispatches `fp:photo-activate` (see §9). The viewer (WP3) listens; the selection
  module (WP4) intercepts it in selection mode.

## 9. Custom events (`core/events.js`)

All events bubble from the element that caused them and are `CustomEvent`s with a `detail` object.

| Event | Detail | Fired by → consumed by |
|---|---|---|
| `fp:photo-activate` | `{photoId, index, contextKey, tile}` | grid tile click / Enter → viewer (WP3); selection (WP4) calls `preventDefault()` in selection mode |
| `fp:selection-change` | `{count, ids: Set}` | selection (WP4) → selection bar, fast scroller hides while count > 0 |
| `fp:photos-removed` | `{ids: number[], contextKey?}` | any mutation (trash, move out, delete) → grid removes tiles and updates month counts; viewer skips them |
| `fp:photos-changed` | `{photos: PhotoJson[]}` | favorite toggle, restore → grid/viewer refresh tile state |
| `fp:photos-added` | `{ids: number[], contextKey?}` | Undo of a trash (viewer, WP4 bulk Undo) → grid rebuilds its sections from the server (a removal during that fetch may come back with it) |
| `fp:grid-zoom` | `{level}` | zoom controls → grid |
| `fp:section-loaded` | `{section, month, tiles: Element[]}` | grid (WP2), after inserting a page of tiles → selection (WP4) re-applies `.is-selected` |
| `fp:viewer-closed` | `{photoId}` | viewer → grid scrolls the tile into view |

## 10. Photo list contexts (`contextKey`)

Every grid and the viewer operate on a **context**: an ordered list of photos defined on the server by
`PhotoListContext` (WP1, `src/http/context.rs`). It is serialised as a short string `contextKey` used
in URLs and `data-context` attributes:

| contextKey | Meaning | Ordering | Month sections |
|---|---|---|---|
| `timeline:all` / `timeline:personal` / `timeline:family` / `timeline:shared` | Timeline with filter | `created_at desc, id desc` | yes |
| `folder:{id}` | Photos in a folder | `created_at desc, id desc` | yes |
| `favorites` | User's favourites | `created_at desc, id desc` | yes |
| `trash` | Trashed photos the user can delete | `trashed_on asc, id asc` (oldest first, like Android) | no |
| `large-files` | Non-trashed photos > 50 MiB | `file_size desc, id desc` | no |
| `memories:{years_ago}` | "On this day", N years ago | `created_at desc, id desc` | no |
| `duplicates:{group_index}` | Not a grid context (see WP6) | – | – |

Filter semantics (web; matches Android's `PhotoType`):
- `personal` = `user_id = me`
- `family` = `user_id is null`
- `shared` = photos in folders owned by someone else that are shared with me (non-expired grantee
  permission) — i.e. `user_id <> me and user_id is not null` **and** readable.
- `all` = union of the three.
- All contexts except `trash` exclude trashed photos.

**Invariant (critical for the grid and fast scroller):** for month-sectioned contexts, the month
summary query (counts per month) and the month page query must use exactly the same predicate and
month definition, so section heights computed from counts always match the photos actually
delivered. `check_invariants` in `context_repo.rs` tests this.

The month definition (changed in WP8): `photos.created_at` is always UTC ISO text
(`YYYY-MM-DDTHH:MM:SS[.f]Z`; the repo converts to UTC on every write and the WP8 migration
normalized old `YYYY-MM-DD HH:MM:SS` rows). Summaries group by `substr(created_at, 1, 7)` and the
month page, ids and bounds queries use the range `created_at >= 'YYYY-MM' and created_at <
'YYYY-MM+1'`, which `idx_photos_user_created_at_desc` and `idx_photos_folder_created` can serve.
`strftime('%Y-%m', created_at)` is no longer used. Within one second a `.250Z` fraction sorts before a plain
`Z` (`'.'` < `'Z'`), so order inside a second is not strictly chronological (same as before).

## 11. The month-section grid protocol (WP1 serves it, WP2 consumes it)

The Android app pre-computes the full grid layout from month summaries (`TimelineLayout`), so it can
show placeholders for unloaded photos, jump to any month, and drive the fast scroller from counts.
The web grid does the same:

1. The page renders **every month section up front**, without photos:
   ```html
   <section class="month" data-month="2024-06" data-count="137"
            data-loaded="false" id="m-2024-06" aria-labelledby="m-2024-06-h">
     <header class="month__header"> <h2 id="m-2024-06-h">June</h2> … select-all toggle, month picker button … </header>
     <div class="month__grid" style="--count: 137"></div>
   </section>
   ```
   The grid's reserved height is `ceil(count / cols) * cell_size + gaps`, computed in CSS
   (`round(up, …)`) or set as `--rows` by JS on resize. Empty sections show nothing but still occupy
   exactly their final height, so **scroll position is stable and meaningful before anything loads**.
2. When a section comes within ~2 viewport heights of the viewport, JS fetches
   `GET /ui/grid/month?ctx=<contextKey>&month=2024-06&offset=0` → HTML fragment of up to **200 tiles**
   plus, if more remain, a sentinel `<div class="month__more" data-next-offset="200">`. Offsets are
   within the month (a month can contain thousands of photos). `index` on each tile = global position
   in the context (the server computes it as the sum of counts of previous months + offset + i).
3. Sections further than ~6 viewport heights away are **unloaded** (children removed, `data-loaded`
   back to `false`) to keep the DOM small in large libraries. Height stays reserved.
4. Flat contexts (trash, large files, memories) use a single section with no header and the same
   loading endpoint (`month` omitted).
5. Mutations: when photos are removed (`fp:photos-removed`), the grid removes their tiles, decrements
   `data-count` of their sections, and removes sections that reach 0.

## 12. Viewer protocol (WP3)

- Opening: `fp:photo-activate` → viewer overlay opens on the photo, `history.pushState` to the current
  URL + `?photo={id}`. Direct navigation to `…?photo={id}` makes the server render the page with the
  viewer's initial state embedded (`<div id="viewer-root" data-initial-photo="{id}" data-context="…">`).
- Navigation data: `GET /api/context/neighbors?ctx=<contextKey>&photo_id={id}&before=30&after=30` →
  `{photos: [ViewerPhoto], index_of_current, total}` where `ViewerPhoto = {id, name, created_at,
  is_video, thumb_hash, file_size, user_id, folder_id, is_favorite}`. The viewer extends its window by
  requesting more when within 10 of either edge. This lets the viewer walk the **whole** context, not
  just loaded tiles (fixes an old-UI bug; matches Android's shared pager).
- Images: thumbhash → `/api/preview/{id}` (already cached from the grid) → `/api/screen/{id}` (≈2048px
  webp, WP1). "Original" (for zooming past 100% of the screen rendition, or when the browser supports
  the format and the file is small) is `/api/download/{id}?inline=1`.
- Info: `GET /ui/viewer/info/{id}` → HTML fragment for the info panel. WP3 renders it from the
  structured `PhotoDetails` produced by WP1's `utils::exif::photo_details(...)` (the same data is also
  exposed as JSON at `GET /api/photos/{id}/info`).

## 13. Dialogs and toasts

- One `<dialog id="app-dialog">` in `base.html`; `core/dialog.js` opens it with HTML content (from a
  string, a `<template>`, or an HTMX fragment loaded into it via `hx-target="#app-dialog-content"`).
  Dialogs are DaisyUI `modal` + native `<dialog>` (focus trap and Esc for free). Bottom-sheet style on
  narrow screens (`modal-bottom sm:modal-middle`), mirroring Android's bottom sheets.
- Toast region: `<div id="toasts" class="toast toast-center toast-bottom" role="status" aria-live="polite">`.
  Toasts can carry one action button (used for **Undo** after trashing/moving — a web addition).
  Server responses can trigger toasts with the header `HX-Trigger: {"fp:toast": {"message": "...", "type": "success"}}`;
  `core/toast.js` listens for `fp:toast` on `document.body`.

## 14. Rust-side organisation for new code

```
src/http/
  context.rs          # PhotoListContext (WP1)
  prefs.rs            # Prefs extractor (WP0)
  views.rs            # shared view models: PhotoTileView, FolderCardView, date formatting (WP0, extended by others)
  fragments.rs        # HX-Redirect-on-401 helper, HX-Trigger toast helper (WP0)
  pages/
    mod.rs            # page router (WP0 owns; others add route lines)
    timeline.rs       # WP2
    grid.rs           # /ui/grid/* (WP2)
    viewer.rs         # /ui/viewer/* (WP3)
    folders.rs        # WP5
    sharing.rs        # WP5
    utilities.rs, trash.rs, favorites.rs, duplicates.rs, large_files.rs, settings.rs  # WP6
    upload.rs         # WP7
    user.rs           # WP0
  api/
    context.rs        # /api/context/neighbors (WP1)
    photos_info.rs    # /api/photos/{id}/info (WP1)
    screen.rs         # /api/screen/{id} (WP1) — or inside download.rs
    …existing files
src/repo/
  context_repo.rs     # queries for PhotoListContext (WP1)
  stats_repo.rs       # statistics, memories, large files (WP1)
```

Date formatting lives in **one** Rust module (`src/http/views.rs` → `fn format_date(...)`), following
the UTC convention (owner decision). Formats (from Android):
- month header: `"June"` if current year else `"June 2022"`
- viewer title: `"8 Jun 2024 · 14:03"`
- info panel: `"Saturday, 8 June 2024 · 14:03"`

## 15. Testing conventions

- **Rust:** `#[sqlx::test]` tests for every new repo query (in `src/repo/tests.rs` or a `tests`
  module next to the new repo file), including permission edge cases (own / public / shared with
  grantee / shared expired / not shared). Handler-level tests where practical using
  `tower::ServiceExt::oneshot` on the router.
- **Playwright:** specs under `playwright/specs/<feature>.spec.ts`. Selectors: `data-testid` only
  (plus roles/labels for a11y assertions). **No `if (visible)` guards, no `waitForTimeout`**; assert
  deterministically. The harness from WP0 provides a fresh seeded server per run (two users `alice`
  and `bob`, fixture photos across several months/years incl. a video, public and private folders,
  one folder shared from alice to bob).
- `data-testid` naming: kebab-case nouns, e.g. `photo-tile`, `month-section`, `fast-scroller-thumb`,
  `selection-bar`, `selection-count`, `viewer`, `viewer-next`, `folder-card`, `toast`.
