# 02 — Product spec (screen by screen)

Source of truth for *behaviour*. "Android:" notes describe what the app does (reference file in
brackets, relative to `app/src/main/java/net/theluckycoder/familyphotos/`). "Web:" notes describe
deliberate differences or additions. Where a detail is not specified here, copy Android.

## 0. Global

- **Dark theme only**, Android palette (see `01-architecture.md` §7).
- **Navigation** (Android: 4 tabs Photos / Folders / Device / Utilities via `NavigationSuiteScaffold`
  — bottom bar on phones, rail on wider screens) [`ui/screen/TopLevelScreen.kt`].
  Web: **Photos**, **Folders**, **Utilities** as the primary destinations, plus an **Upload** action.
  - `< 768px`: fixed bottom navigation bar (3 items) + an upload icon button in the page top bar.
  - `≥ 768px`: left navigation rail (icons + labels), with Upload as a prominent button at the top of
    the rail. `≥ 1280px`: the rail expands to a sidebar that also lists Favorites and Trash as
    secondary links.
  - The Device tab is **not ported** (see §12).
- **Toasts** instead of Android snackbars: typed success / error / info with Android's colours,
  bottom-centre, auto-dismiss after 4 s (errors 6 s), dismissible. Android has no undo; **web adds
  "Undo"** on "N photos moved to trash" (calls restore). There is no undo for moves (the original
  locations and owners aren't tracked).
- **Offline / errors:** Android shows a red "Failed to connect to server" card on the Timeline. Web:
  network failures show an error toast "Failed to connect to server"; 401 anywhere redirects to login.
- **Plurals:** "1 photo" / "N photos" everywhere (`core/format.js` + a Rust helper).
- **Keyboard (web addition):** documented in a `?` shortcuts dialog. Global: `g p` Photos,
  `g f` Folders, `g u` Utilities, `u` Upload, `?` help. Grid: see §2. Viewer: see §3.

## 1. Login [`ui/screen/LoginScreen.kt`]

- App icon, title "Family Photos", subtitle "Your self-hosted photo library".
- Fields: Username (trimmed and **lowercased** before submit, like Android), Password with a
  show/hide toggle. No server address field (same origin).
- Login button disabled until both fields are non-empty; shows a spinner + "Logging in…" while
  submitting.
- Errors: "Invalid username or password." (401) / "Cannot connect to the server." (network).
- After success, redirect to `?next=` if it is a same-origin relative path, else `/`.
- Already logged in → redirect to `/`.

## 2. Photos tab — Timeline [`ui/screen/tabs/TimelineTab.kt`, `ui/composables/PhotosList.kt`, `ui/viewmodel/TimelineViewModel.kt`]

**Header content (scrolls with the grid, not sticky):**
1. **Filter chips**: All / Personal / Family / Shared (DaisyUI filter/`btn` group; the selected one
   shows a check icon). Persisted in the `filter` pref and shared with the Folders tab and destination
   picker. Changing it reloads the grid (HTMX swap of the grid root, URL updated with `?filter=`).
2. **Memories row** ("On this day"): horizontal scroll of cards, 140 px wide, 3:4 aspect, 20 px
   radius, cover image with a gradient and the label "1 year ago" / "N years ago". Memories = photos
   taken 1–10 years ago within ±3 days of today's day-of-year (with year wrap-around), not trashed,
   respecting the filter, grouped by years-ago, ordered by years-ago ascending. Tapping a card opens
   the viewer on that group (context `memories:{n}`). Hidden when there are none.
3. The grid.

**Grid (shared by Timeline, Folder, Favorites):**
- Photos newest first, grouped by month. **Month header** label: full month name, plus the year only
  if it isn't the current year ("March", "December 2022"). Headers are not sticky.
- Header controls: a **select-all-in-month** circle (always visible; checked when every photo of the
  month is selected; toggles), and — only when there are more than 2 months — a **month picker**
  button (calendar icon) that opens the month picker.
- **Square tiles**, 1–2 px gaps, centre-cropped preview; ThumbHash placeholder until the preview
  loads; videos show a filled play-circle icon at the top right.
- **Zoom levels** (4, persisted in `zoom`, default 1). Columns per level:

  | Viewport width | L0 | L1 | L2 | L3 |
  |---|---|---|---|---|
  | < 640 px (phone portrait) | 3 | 4 | 5 | 7 |
  | 640–1023 px | 4 | 6 | 8 | 10 |
  | ≥ 1024 px | 5 | 8 | 10 | 13 |

  Changing zoom: **pinch** on touch (Android thresholds: accumulated scale > 1.8 → fewer columns,
  < 0.6 → more; one long pinch can step several levels), **Ctrl/⌘ + mouse wheel or trackpad pinch**
  (which browsers report as ctrl+wheel), keyboard `+`/`-`, and a small zoom control (−/+) in the page
  top bar on desktop. Keep the photo under the pinch/cursor anchored while columns change.
- **Month picker** [`ui/composables/MonthPickerDialog.kt`]: bottom sheet / dialog titled
  "Select a Month" listing every month: 56 px rounded cover thumbnail, month label (always with the
  year here), "N photos". Selecting scrolls so that month's header sits **20% from the top** of the
  viewport.
- **Fast scroller** [`ui/composables/MonthScrollIndicator.kt`] — must behave like Android:
  - A thumb (44×56 px, rounded on its left side, `secondary` colour, vertical-arrows icon) at the
    right edge. The track spans the viewport below the top bar (Android insets: 128 px top, 64 px
    bottom; web: top bar height + 16 px top, 16 px bottom / above the mobile bottom nav).
  - Shown only if there are **≥ 2 months** and **no selection is active**. Appears when the page
    scrolls, hides 2.5 s after scrolling stops (fade + slide out to the right), stays visible while
    hovered (web) or dragged.
  - **Passive position** = `scrollY / (scrollHeight − viewportHeight)` mapped onto the track, animated
    over 150 ms.
  - **Dragging**: the drag position (0–1) is mapped to a target position in the full layout; a binary
    search over the cumulative month offsets finds the month; the page **jumps only when the month
    changes**, placing that month's header 20% from the top. (Android gives a haptic tick per month;
    web: `navigator.vibrate?.(5)` on touch devices.)
  - **Web addition**: while dragging, a label bubble next to the thumb shows the month ("June 2024").
  - The native page scrollbar is hidden on the grid pages (`scrollbar-width: none`) so there is a
    single scroll affordance; wheel/touch scrolling are unaffected.
  - Because every month section reserves its exact height up front (see `01-architecture.md` §11),
    positions are correct even for months that haven't loaded yet. **This is the reason the old web
    scrollbar failed — it measured only loaded content.**
- **Selection** — see §5.
- **Tap/click a tile** opens the viewer on the whole context (the entire timeline with the current
  filter). **Enter/Space** on a focused tile does the same. Arrow keys move focus between tiles
  (grid navigation; web addition).
- **Empty state**: icon + "No photos yet" + an Upload button.

## 3. Photo viewer [`ui/composables/PhotoViewer.kt`, `ui/viewmodel/PhotoViewerViewModel.kt`, `ui/composables/player/VideoPlayer.kt`, `ui/dialog/NetworkPhotoInfoDialog.kt`]

- Full-screen overlay, black background, over the current page (URL gets `?photo={id}`; Back / Esc
  closes it). Opening/closing animates between the tile and the full image (**View Transitions API**
  where supported, else a 150 ms fade + scale from 0.96, like Android). On close, the grid scrolls so
  the tile of the photo you ended on is visible and animates back into it (Android does this too).
- **Navigation** through the entire context: swipe left/right (touch), arrow buttons on hover
  (desktop), keyboard ←/→. Neighbours ±1 are preloaded (web addition). At the ends, the arrow is hidden.
- **Zoom**: pinch, double-tap/double-click (cycles 1× → 2× → 1×), Ctrl/⌘+wheel or plain wheel
  (desktop), keyboard `+`/`-`/`0`. Pan when zoomed (drag). When zoomed in, swipe navigation is
  disabled and the bars hide (Android: bars shown only while zoom < 1.1).
- **Progressive loading**: thumbhash (sized to the photo aspect) → cached preview → screen rendition
  (`/api/screen/{id}`) with a crossfade. When zoomed beyond the screen rendition's resolution, load
  the original (`/api/download/{id}?inline=1`) if the browser can decode it.
- **Bars** (toggle with a tap/click on the image; auto-hide after 3 s of mouse inactivity on
  desktop — web addition):
  - **Top bar**: Back (close), title = date `"8 Jun 2024 · 14:03"`, favourite star (filled when
    favourite; toggles via `POST/DELETE /api/favorite/{id}`; the UI updates only on success).
    Web addition: an overflow menu with "Download", "Copy link".
  - **Bottom bar** (icon + label buttons, evenly spaced): **Delete** (confirm dialog → trash),
    **Share**, **Move**, **Info**.
    - Share (web): Web Share API with the file (`navigator.share({files})`) when supported (mobile),
      otherwise copy the deep link (`/?photo={id}` in its context) to the clipboard with a toast
      "Link copied". Only logged-in users with access can open it.
    - Move opens the move dialog (WP4/WP5) with this single photo.
    - Actions the user isn't allowed to perform are hidden: Delete/Move need Delete access (own,
      public, or a shared folder with `can_delete`).
- **Keyboard**: ←/→ navigate, Esc close, `f` favourite, `i` info, `Delete`/`#` trash (with confirm),
  `m` move, `d` download, `+`/`-`/`0` zoom, Space play/pause (video). Shortcuts ignore events with
  Ctrl/⌘/Alt modifiers (except zoom) and are disabled while a dialog is open.
- **Video**: `<video>` with custom controls like Android: centre controls (back 5 s, play/pause,
  forward 5 s on translucent black circles), bottom bar with `mm:ss / mm:ss` time, mute button, seek
  slider. Not autoplayed (Android prepares and pauses); web: autoplay muted is **off**. Pauses when the
  viewer navigates away. Keyboard (web addition): Space play/pause, `j`/`l` ±5 s, `Shift+M` mute
  (plain `m` is Move).
- **Info panel** (side panel on desktop ≥ 1024 px, bottom sheet on mobile; text selectable):
  - Header: file name (max 2 lines), date "Saturday, 8 June 2024 · 14:03".
  - Location card (only when EXIF has GPS): "lat, lon" with 4 decimals, and "Open in Maps" linking to
    `https://www.openstreetmap.org/?mlat=…&mlon=…#map=15/…/…` (web replacement for Android's `geo:`).
  - Details card:
    1. Type + resolution: "Image (JPG)" / "Video (MP4)" + "4032 × 3024 (12.2 MP)".
    2. Camera: "Model (Lens)" and "f/1.8 · 1/250s · 26mm · ISO 100" (exposure < 1 s shown as 1/x).
    3. Folder name (link to the folder) or "No folder", and file size.
    4. Visibility: "Family" (public) or "Private", with the owner's display name for shared photos.
  - Rows with no data are omitted.
- **Delete dialog** [`ui/dialog/DeletePhotosDialog.kt`]: title "Delete photos" / "Delete photo",
  text "Are you sure you want to move these photos to the trash?", a row of 64 px thumbnails of the
  affected photos (max ~8, then "+N"), red **Delete** button (spinner while running, dialog not
  dismissable while running, closes only on success), Cancel. Permanent variant: title "Delete
  permanently", red delete-forever icon, text "… cannot be recovered."
- After trashing the current photo, the viewer moves to the next photo (or previous at the end, or
  closes if it was the last), and emits `fp:photos-removed`.

## 4. Folders tab [`ui/screen/tabs/NetworkFoldersTab.kt`, `ui/composables/FolderComposables.kt`, `ui/viewmodel/FoldersTabViewModel.kt`]

- Header: **search** field "Search folders…" (accent- and case-insensitive, clear ✕, filters as you
  type — client-side is fine since folder counts are small), **filter chips** (All / Personal / Family
  / Shared; the same `filter` pref), **sort** dropdown (Date (Newest), Name (A to Z) [default],
  Name (Z to A), Photo Count (Most)), **grid/list toggle**. Sort and view are persisted.
- **Web addition:** "New folder" button (Android only creates folders from Move/Upload). Opens the
  folder name dialog (§6) and navigates to the new folder.
- Folder type: owner null → **Family**; owner = me → **Personal**; otherwise → **Shared**.
- Filter: Personal = my folders, Family = public folders, Shared = folders shared with me, All = all.
- Counts are non-trashed photos; cover = newest non-trashed photo.
- **Grid view**: 3 / 4 / 6 / 8 columns by width (Android: 3 portrait, 5 landscape). Square cover,
  16 px radius, bottom gradient with the type icon (person / family / share), name (1 line, ellipsis),
  "N photos".
- **List view**: 1 column on phones, 2 on wide screens. 72 px rounded thumbnail, name,
  "N photos • Personal/Family/Shared".
- Empty folders are shown with a placeholder cover (owner decision).
- Empty state: "No folders" (+ "New folder" button); search with no results: "No folders match".
- Ties in sorting fall back to name A→Z.

## 5. Selection & bulk actions [`ui/composables/PhotosSelectionBar.kt`, `ui/composables/PhotoUtilitiesActions.kt`, `ui/composables/Modifiers.kt`]

- **Entering selection**: long-press a tile (touch, 400 ms), click the tile's check circle (desktop;
  the circle appears on hover), `Ctrl/⌘+click` a tile, or the month select-all circle. **Selection
  mode = the selected set is non-empty.**
- In selection mode: click/tap toggles; **Shift+click** selects the range from the last toggled tile
  (web addition); unselected tiles show an unchecked circle (white, 85% opacity); selected tiles get a
  40% black scrim, a filled check circle in `primary`, and a slight scale (0.85 → 1 on the icon).
- **Drag-to-select** (Android's `photoGridDrag`): long-press then drag (touch) or mouse-down on a
  check circle then drag (desktop). The selection is always the **contiguous range anchor..current**
  in context order (`data-index`); dragging back shrinks it. **Auto-scroll** when the pointer is within
  40 px of the top/bottom of the viewport, speed proportional to depth. Haptic on start
  (`navigator.vibrate`).
- Month select-all selects every photo of the month; if the month isn't fully loaded, the grid loads
  its remaining pages first (or the server returns its ids — see WP4).
- `Ctrl/⌘+A` selects all **loaded** tiles in the current context (web addition). `Esc` clears.
- The **fast scroller hides** while selecting.
- **Selection bar**: floating pill at the top centre (tertiary container colour `#1F4D52` /
  `#BCEBF0`, 56 px tall), animated in/out; left: ✕ + "N selected" (count animates); right: actions.
- Actions (network photos) — Android: Delete, Share, Move. Web:
  - **Delete** → delete dialog → `POST /api/trash` → toast "N photos moved to trash" with **Undo**.
    Only enabled if every selected photo is deletable; otherwise disabled with a tooltip
    "You can't delete some of the selected photos".
  - **Move** → move dialog (destination picker) → `POST /api/move` → toast "N photos moved".
  - **Add to favorites** (web addition): favourites all selected.
  - **Download** (web addition): downloads each selected file sequentially via hidden `<a download>`
    (no zip endpoint); warns above 20 files.
  - **Share** on web = the Web Share API with files if available (mobile), else hidden.
- On trash/move success the selection clears; on failure it stays.

## 6. Folder page [`ui/screen/FolderScreen.kt`, `ui/viewmodel/FolderScreenViewModel.kt`]

- Top bar: Back, folder name, a "N photos" pill, and a **type badge**: Family / Personal /
  "Shared (N)" (my private folder shared with N people) / Shared (someone else's folder shared with me,
  plus the owner's name).
- Actions (web gates them properly — Android shows them always):
  - **Share** (people icon): only for a **private folder I own**. Opens the sharing dialog (§7).
    For a shared-with-me folder, a read-only "People with access" view is available.
  - **Rename / edit** (pencil): for folders I own or public folders. Opens the folder name dialog
    with the name and a Personal/Family segmented toggle (changing visibility = `is_public`). Confirm
    disabled while the name is blank (trimmed). `PATCH /api/folders/{id}`. Toasts "Folder renamed" /
    error text (e.g. "A folder with this name already exists").
  - **Delete folder** (web addition, overflow menu): owner or public folder, only when the folder
    has no photos (owner decision).
  - **Upload** FAB (Android: cloud-upload FAB): only if `can_upload`. Opens `/upload?folder_id={id}`.
- Body: the same month-grouped grid (context `folder:{id}`), selection with actions gated by
  `can_delete` (shared folders without delete permission: no Delete/Move).
- Empty state: "This folder is empty" + Upload button (if `can_upload`).

## 7. Sharing dialog ("Manage access") [`ui/dialog/FolderSharingBottomSheet.kt`, `core/data/repository/SharingRepository.kt`]

- Title "Manage access", section "Members".
- Owner row: avatar with the initial, display name, "Owner". (Public folders can't be shared —
  the dialog isn't offered; Android shows "Everyone in Family / Shared access".)
- Member rows: avatar initial, display name (fallback to user id), current permission in italics:
  **View only** (no flags), **Can upload** (`can_upload`), **Can upload & delete** (both).
  Delete-without-upload is not offered.
- Owner can change a member's permission (radio menu → `PUT /api/sharing/{id}`) or **Remove member**
  (red, confirm → `DELETE /api/sharing/{id}`).
- **Add members**: expands a list of all members (`/api/members`) minus the owner and existing
  grantees; picking one creates a **View only** share (`POST /api/sharing`).
- **Web addition — expiry**: each member row can show "Expires 12 Oct 2026"; the add flow and the
  permission menu offer "No expiry / 1 day / 1 week / 1 month / custom date". Requires WP5's backend
  change to make `expires_at` updatable (additive field on PUT).
- Toasts: "Member added" / "Permissions updated" / "Member removed" / "Failed to …".
- Grantees see the list read-only.
- Share links (tokens) are out of scope (owner decision).

## 8. Move / Upload destination picker [`ui/composables/UploadComposables.kt`, `ui/screen/MovePhotosScreen.kt`, `ui/screen/UploadPhotosScreen.kt`, `core/data/model/UploadChoice.kt`]

A shared component (WP5) used by the Move dialog (WP4) and the Upload page (WP7).
- Showcase row: up to 8 thumbnails of the photos involved + "N photos" (upload: file thumbnails).
- Filter chips (same `filter` pref): Family preselects "Family gallery", Personal preselects
  "Personal gallery", Shared clears the choice (must pick a folder), All keeps the current choice.
  Default choice: Personal gallery.
- Destinations:
  - "Into Personal Gallery" (no folder, private) — shown for All / Personal.
  - "Into Family Gallery" (no folder, public) — shown for All / Family.
  - Folder list (the Folders tab component with search, sort, grid/list), filtered by the chip and
    **limited to folders where I can upload** (`can_upload`).
  - "Create new folder" (hidden under Shared): folder name dialog (name + Personal/Family). The
    folder is created when the user confirms the move/upload (`POST /api/folders`; the server's
    upsert returns an existing folder with the same name, matching Android's reuse-by-name).
- Choosing a folder collapses the list into a highlighted "selected folder" row (name, type, ✕ to
  clear).
- Output: `{kind: 'root', isPublic}` | `{kind: 'folder', folderId}` | `{kind: 'new', name, isPublic}`.
- Move semantics: root → `POST /api/move?make_public=<bool>`; folder → `?target_folder_id=`.
  Confirm button "Move photos" / "Upload photos"; disabled with no choice.

## 9. Utilities tab [`ui/screen/tabs/UtilitiesTab.kt`, `ui/viewmodel/UtilitiesViewModel.kt`]

- **Photo Statistics card**: counts only my photos + public photos, not trashed:
  Family (count, size), Personal (count, size), Images (count, size), Videos (count, size),
  Total "N photos • X GB".
- A 2-column grid (3–4 on desktop) of tool buttons with coloured icons: **Favorites**, **Trash**,
  **Duplicates**, **Large Files**, **Settings**, **Upload**. (Camera is Android-only.)

## 10. Favorites / Trash / Duplicates / Large files [`ui/screen/FolderScreen.kt` (favorites source), `ui/screen/TrashScreen.kt`, `ui/screen/DuplicatesScreen.kt`, `ui/screen/LargeFilesScreen.kt`]

- **Favorites**: the folder page layout with the badge "Favorites ★", context `favorites`, all
  readable photos I favourited (including shared-folder photos — web fixes the old own+public
  restriction). Unfavouriting from the viewer removes the tile.
- **Trash**:
  - Top bar with count; info line "Photos in trash are automatically deleted after 30 days".
  - Flat grid (3 cols on phones … up to 10 on desktop), oldest-trashed first.
  - Each tile shows a "N days" badge = `30 − days since trashed_on` (floored at 0); red when ≤ 7.
  - Selection actions: **Restore** (no confirm, toast "N photos restored"), **Delete forever**
    (permanent dialog → `DELETE /api/delete/{id}` per photo, parallel with a limit of 4, toast
    "N photos deleted permanently").
  - Web additions: header actions **Restore all** and **Empty trash** (confirm, permanent).
  - Clicking a tile opens the viewer in the trash context (Android does nothing) with a bottom bar of
    Restore / Delete forever / Info instead of the normal actions.
  - Includes trashed photos from shared folders where I have `can_delete`.
  - Empty state "Trash is empty".
- **Duplicates**:
  - Data: `GET /api/duplicates` groups (server-side, blake3 hash) — the page is server-rendered.
  - Top bar: title, "N groups", Info action for the current group's first photo.
  - One group at a time (pager with prev/next and swipe), showing the first photo large (zoomable
    like the viewer), and "Which do you want to keep?" with one button per copy showing a
    Family/Personal icon and the folder name (or italic "No folder"), plus date and size.
  - Choosing one → confirm dialog → trash all other copies → the group disappears; toast.
  - States: "Failed to load duplicates" / "No duplicates found".
- **Large files**: flat grid of non-trashed photos > 50 MiB (my photos + public + shared I can read),
  largest first, size badge on each tile (e.g. "1.2 GB"), click opens the viewer (context
  `large-files`), selection with Delete/Download. Empty state "No files larger than 50 MB".

## 11. Settings [`ui/screen/SettingsScreen.kt`, `ui/viewmodel/SettingsViewModel.kt`]

- Account: display name, user id, **Sign out** (confirm dialog → `POST /api/logout` → `/login`).
- Display: default photo filter, grid zoom level (with a live 4-step control), folders sort, folders
  grid/list (all write the `fp_prefs` cookie).
- About: server version (`env!("CARGO_PKG_VERSION")`) and asset version.
- Web addition (optional, WP6 stretch): **Change password** (current + new + confirm) —
  `POST /api/profile/password` (new endpoint, additive). Changing it invalidates other sessions
  (axum-login uses the password hash as the session auth hash).
- Android-only settings **not ported**: backup over mobile data, cache size, clear cache, server
  address.

## 12. Android features intentionally NOT ported

| Android feature | Why not / web replacement |
|---|---|
| **Camera** (`camera/` module: CameraX with timer, grid overlay, zoom presets, date overlay burned into JPEGs, saves to `DCIM/Camera`) | Device hardware feature; it never talks to the server. No web equivalent. Upload handles getting photos in. |
| **Device tab** (local MediaStore albums, backup toggles per album, "N photos pending backup", Backup Now / Clear Queue, cloud-done badges on local photos) | No device library on the web. Replaced by the **Upload page**: drag & drop files **or whole folders**, an upload queue with retries and per-file progress, and "already on server" detection via BLAKE3 hashes computed in the browser (WP7). |
| **Backup worker** (periodic automatic uploads) | Not possible in a browser tab. The upload queue survives in the page while it's open. |
| **Share = download to device then system share sheet** | Web Share API with files where available, else copy link / download. |
| **Local photo viewer, delete-from-device, "Saved" badge** | N/A. |
| **Pull to refresh / sync** | The web reads the server directly; no local DB. |
| **Cache size settings** | Browser-managed. |
| **Server address on login** | Same origin. |
