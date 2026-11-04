# Web UI rewrite — progress and remaining work

Coordinator's tracking file (last updated 2026-10-29, session 4). **Start the next session
by reading this file**, then `01-architecture.md` and the WP's plan, and continue from "Resume here".

Integration branch: `webui` (from `folder`), checked out in the main checkout. Package branches:
`webui-wpN-<name>` (git can't have both `webui` and `webui/...` refs). Worktrees live under
`.claude/worktrees/` (excluded via `.git/info/exclude`). Nothing is pushed; no PRs.

## The owner's original instructions (session 1)
- Claude acts as the **coordinator**: it doesn't implement work packages itself but runs one agent per
  WP, each in its own worktree and branch from `webui`. Prompt: read the root `CLAUDE.md`,
  `docs/web-ui-plan/01-architecture.md`, `02-product-spec.md` and the WP's plan (absolute paths in the main checkout:
  they're untracked, worktrees don't have them); implement the scope, meet the acceptance criteria, write the tests;
  when the plan is ambiguous pick what the Android app does; write the hand-off note to
  `docs/web-ui-plan/handoff/<wp>.md` (untracked, never committed).
  Hand-off notes go in `docs/web-ui-plan/handoff/wpN.md`. Agent final reports stay under 500 words.
- Phases: 1 = WP0 + WP1, 2 = WP2 + WP5, 3 = WP3 + WP4 + WP6 + WP7, 4 = WP8. Packages in a phase run
  in parallel. A phase starts only when everything from the previous phase is merged.
- After each agent: the coordinator runs `cargo build`, `cargo test`, `cargo clippy`, `npm run prod`
  and `npm run test` itself, checks the acceptance criteria, and merges into `webui` in phase order.
  If an agent fell short, send it back with the specific gaps; don't fix things yourself.
- **Ask the owner before:** any DB migration, any non-additive change to an existing `/api` endpoint
  (the Android app depends on them), or changing an earlier owner decision (UTC as wall clock for dates; a folder can be deleted only with
  zero photos incl. trashed; no share links; dark theme only; a 2048 px screen rendition; empty folders shown).
- **Models (always):** Sonnet for coding (incl. rebases, cleanups, small fixes); Opus at medium effort for
  reviews. `CLAUDE_CODE_SUBAGENT_MODEL=sonnet` is set in `.claude/settings.local.json`, so only
  reviews need an explicit `model: opus`, `effort: medium`.
- The owner's dev server runs on port 8080 against `test.db`: never touch it.
- The owner asked for a **review agent** that reviews a coding agent's work and sends it feedback.
  This was done for WP0 and worked well; keep doing it for every package.

## Owner decisions made in session 1
1. The base64 bump was committed on `webui` (`6e8d574`).
2. **`/api/preview/{id}` must fall back to the original photo** when a preview can't be generated,
   as before WP1. The fallback is sent `no-store` with no ETag. Implemented in `1f6641a`. This
   overrides the plan's "503, no original fallback" rule. `/api/screen/{id}`, which is web-only,
   still returns 503.
3. **`/api/delete/{id}` stays owner-only.** WP6 must hide "Delete forever" (and exclude those photos
   from "Empty trash") for trashed photos the user doesn't own; restore still works.
4. The owner cares a lot about **not changing `/api` routes or behaviour**. Review every package's
   diff to existing endpoints and report any behaviour change explicitly.

## State (session 2, 2026-10-28)

**Owner rules, session 2:**
- **No `Co-Authored-By: Claude` or other AI attribution in commits** (the owner's global CLAUDE.md).
  Session 1 and the early session-2 agents got this wrong, so `webui` has ~10 such commits
  (WP0/WP1), and WP2/WP5 have some on their branches. Squash-merge new packages with a clean message.
  History rewrite: done in session 3 (see below).
- **No `.sqlx` files for test-only queries in git.** Run `cargo sqlx prepare` without `--all-targets`.
  Test and clippy with `DATABASE_URL=sqlite:<worktree>/wp.db`, not `SQLX_OFFLINE=true`. `webui`
  dropped 13 such files in `08f1902`; they are still in WP1's commit `9fc8c28` in the history.
- `main` and `webui` have **separate histories** (`webui` came from `folder`). Bring `main` fixes
  into `webui` with a cherry-pick, never a merge.

| Item | Branch | State |
|---|---|---|
| Plan docs | `webui` | committed `90c92f4` |
| WP1 Backend core, plus the preview-fallback fix-up | — | **merged** (`9762454`, `1f6641a`); branch deleted |
| WP0 Foundation | — | **merged** (`76f3e46`) after the coordinator's checks (128 Rust tests, clippy only the 2 known `password_hash.rs` warnings, 73 passed / 4 skipped Playwright, old-UI grep empty). `/api` diff: a 404 fallback (same behaviour as before) and `/api/login` now trims + lowercases the user id (Android already does both; no ids in `test.db` affected), plus `next` for the web form. Branch deleted. |
| Rust test rewrite | — | **merged** as the squash `02bd1ef` (88 tests, clippy clean, no production changes, test-only `.sqlx` dropped). Review found no must-fix items; the nice-to-haves were done. Branch deleted. |
| Folder event-log fix | `main` @ `d19ddfe` (local, not pushed) | **On `main`** with the owner's OK, and cherry-picked into `webui` as `78f1328`, with its tests ported to the rewrite's helpers (90 tests, clippy clean). |
| WP2 Grid/Timeline | `webui-wp2-grid-timeline` @ `fe161b9` (`.claude/worktrees/wp2`, port 8111) | **Coding agent done** (it has already merged `webui` 78f1328). Reported 104 Rust tests, clippy clean, 26 unit tests and 169 passed / 17 skipped Playwright; no `/api` diff, no `.sqlx` changes. Shared files touched: `pages/mod.rs`, `page_context.rs`, `context.rs`, `context_repo.rs`, `macros/photo.html` (`badge_kind` arg), `input.css`, `shortcuts.html`. Deviations are in `handoff/wp2.md`. **Next: review agent → fixes → coordinator checks → squash-merge**, then tell WP5. |
| WP5 Folders & Sharing | `webui-wp5-folders-sharing` @ `e5df6be` (`.claude/worktrees/wp5`, port 8112) | **Coding agent done except the folder-page grid** (marked `GRID GAP (WP2)` in `templates/folders/folder_page.html`; it has already merged `webui` 78f1328). Reported 110 Rust tests, clippy clean, 159 passed / 4 skipped Playwright; `.sqlx` has 4 production queries. **`/api` changes to show the owner:** new `DELETE /api/folders/{id}` (204/404/401/409 when not empty), `PUT /api/sharing/{id}` takes an optional `expires_at` (absent = unchanged), and **`POST /api/sharing` with a past `expires_at` is now 400 instead of a 500** (a behaviour change for invalid requests only). **The owner approved all three (session 2).** Also `core/api.js` shows 409 bodies. **Next:** after WP2 is merged, the WP5 agent (or a fresh one) merges `webui` and fills the grid gap (see `handoff/wp5.md`); then review agent → checks → squash-merge. |

New agents were told to put new Rust tests in `#[cfg(test)]` blocks inside files they own, to avoid
conflicts with the test rewrite. Tip: the filesystem is btrfs, so new worktrees get
`cp -a --reflink=always` copies of an existing `target/` and `node_modules/` for fast builds.

## Session 3 (2026-10-28)
Session 3 started review
agents for WP2 and WP5 in parallel (WP5's review excludes the grid gap). Plan: a fresh coding agent
fixes WP2's review findings → checks → squash-merge WP2 → a fresh WP5 agent fixes its findings and
fills the grid gap → short re-review of the grid part → checks → squash-merge WP5 → **history
cleanup of `webui`** → Phase 3.

**Owner decision, session 3 (replaces "rewrite at the end, after WP8"):** clean up `webui`'s
history **once WP2 and WP5 are merged and their worktrees removed, before any Phase 3 worktree is
created** (so no branch is based on the old commits). Scope: strip `Co-Authored-By: Claude`/AI
trailers from every commit message, remove WP1's test-only `.sqlx` files from `9fc8c28` (which
makes `08f1902` empty → drop it), keep all content and authorship otherwise. **The owner also wants
the history linear** (no merge commits), with short, easy-to-follow commit messages; squashing is
allowed (e.g. one commit per package/fix). Tag the old tip
(`webui-pre-cleanup`) first, verify `git diff webui-pre-cleanup webui` shows only the `.sqlx`
removal (i.e. empty, since `08f1902` already dropped them), then run the checks.

**Session 3 progress:**
- WP2 review: no `/api` diff. 2 must-fix items (touch on the fast-scroller track jumps months; rustfmt
  indent in `context_repo.rs`) plus should-fix items. A fix agent is running in `wp2`. The "huge single
  month isn't bounded in the DOM / needs N/200 sequential loads" finding is documented for WP8.
- WP5 review: no must-fix items, no unapproved `/api` diff. To fix when it merges `webui` after WP2:
  the member-count badge counts expired shares (`templates/sharing/dialog.html:20`); write the Android
  `syncFolders` replaceAll check into `handoff/wp5.md`; `replace('{name}', () => …)` in
  `folders/actions.js`; rename the misnamed e2e test at `sharing.spec.ts:141`; `busy` stuck after a
  failed re-fetch in `sharing-dialog.js`; plus the grid gap.
- **Open item 1 (expiry text comparison), DECIDED:** fix it as a separate commit on `main`, then put
  it into `webui`; WP5 waits for it. An agent is running: branch `fix-share-expiry` (worktree
  `expiry`, from `main`) plus a port, `webui-share-expiry` (worktree `expiry-webui`, port 8118). Trash
  cleanup dates are checked too. **DONE:** `main` @ `8feee10`, `webui` @ `4414824` ("Compare share
  expiry dates as datetimes"): `datetime(x)` on both sides of every `expires_at`/`trashed_on`
  comparison (trash purge was a day late too). On `webui`, the shared `Dataset`'s `expired` share
  now ends hours ago on the same UTC day, and `shared-delete` ends in 3 h. Coordinator checks: main
  89 tests, webui 90 tests + 73/4 e2e, clippy clean. Left: the `expires_at > created_at` CHECK in a
  migration has the same text flaw (harmless). Worktrees and branches removed. Nothing pushed.

- **WP2 MERGED** as the squash `3591ae6` (after the review fixes `931e0d1` and a merge of `webui`).
  Coordinator checks: 104 Rust tests, clippy and rustfmt clean, 26 unit tests, 178 passed / 20 skipped
  e2e; no `/api`, `.sqlx` or migration diff. Worktree and branch removed.
- WP5 finishing agent started (merge `webui`, `datetime()` in its own `folder_cards` SQL, grid gap,
  review items). Next: short review of the new parts → checks → squash-merge → history cleanup → Phase 3.

- **WP3 started early, at the owner's request** (before WP5 merged), in `.claude/worktrees/wp3`, branch
  `webui-wp3-viewer`, based on `3591ae6`, port 8113. It was told not to merge `webui`. After the history
  cleanup, rebase it with `git rebase --onto <new webui> webui-pre-cleanup` (or squash it onto the new tip).

- **WP5 MERGED** (round-2 review approved; checks: 125 Rust, clippy and sqlx check clean, 34 unit, 272/20
  e2e). Nice-to-haves left for later: the expiry e2e uses `now + 3 s` (use +5 if it flakes); the folder
  page's "Delete folder" item doesn't appear when the last photo is removed in place (WP4/WP6 note).
- **HISTORY CLEANUP DONE.** The old tip is tagged `webui-pre-cleanup` (`b805d7a`). `webui` is now 9 linear
  commits on `ae2d03f` (which is on `origin/folder`), one-line messages, no trailers, no test-only `.sqlx`
  in any tree, final tree byte-identical:
  `3bf67ce` plan · `409e3b0` base64 · `6414a9a` WP1 · `0f33751` WP0 · `f163003` test rewrite ·
  `0b021f7` folder cursor fix · `1776098` expiry fix · `1c8d652` WP2 · `cdf71d2` WP5.
  **From now on: one squash commit per package, one-line message, keep it linear.**

- **Owner, session 3: rebase `webui` onto `main`.** This replaces the session-2 "separate histories,
  cherry-pick only" rule: from now on `webui` descends from `main`. It brings in `main`'s `8457760`
  (path-name validation, shared-folder-scoped upload dedupe, move target on the write connection, dropping
  per-user shares on going public), which `webui` was missing, plus checkout v7. An agent is doing it in
  `.claude/worktrees/rebase` (branch `webui-on-main`, port 8119): 7 commits on `8feee10`; the cursor and
  expiry fix commits are dropped, their tests are kept. Afterwards the coordinator checks it, moves
  `webui` there, and rebases WP3 (from `3591ae6`/`1c8d652`) onto the new WP2 commit.

- **REBASE DONE.** `webui` = `main` (`8feee10`) + 7 commits: `36eee6a` plan · `4210042` base64 ·
  `3641e9b` WP1 · `7a33a35` WP0 · `83daf17` test rewrite (it now also holds the ported cursor and expiry
  tests) · `20989ef` WP2 · `ce4213b` WP5. Diff to the old tip `cdf71d2`: only `8457760` (shares dropped on
  going public, move target on the write connection, upload and **check-hashes** dedupe limited to the
  target shared folder, `.dockerignore`/Dockerfile, 2 new `.sqlx` files, tests) plus checkout v7.
  `is_safe_path_component` is merged into WP1's stricter `validate_folder_name`. Coordinator checks:
  127 Rust, clippy and sqlx check clean, 34 unit, 272/20 e2e.
  **Open for the owner:** `/api/upload` with an unsafe name (`../../x`) is 400 on `main` but is
  sanitised to `x` on `webui` (WP1's `sanitize_file_name`).
  **WP3:** rebase with `git rebase --onto 20989ef 3591ae6` (the old WP2 commit; tag `webui-pre-cleanup`
  holds it). Delete the tag afterwards.

- **Upload names, DECIDED:** keep `main`'s 400 "Invalid file name". An agent is making the commit
  `Reject unsafe upload file names` in `.claude/worktrees/upload-name` (branch `webui-upload-name`, port 8120).
  Check it, then fast-forward `webui` and rebase WP3 onto it.
- **WP3 coded** (`d7d7b5b`; viewer, info panel, delete dialog; no `/api` diff). Rebased onto `webui` `ce4213b`
  as `24a12b8` with no conflicts; tag `webui-pre-cleanup` deleted. Known integration gap: WP5's folder page
  doesn't call `.viewer_context(&ctx)`. The review agent is running (port 8113), told to treat WP5 gaps
  as must-fix. Next: a fix agent → checks → squash-merge → WP4, WP6, WP7.

## Session 4 (2026-10-29)
**Owner decision:** WP7 starts now (once the upload
fix is on `webui`), in parallel with WP3's review/fix; WP4 and WP6 start after WP3 is merged. Reviews
are written to `.claude/worktrees/reviews/wpN.md` (excluded from git).
- Found: `cargo fmt --check` on `webui` shows drift in `api/move_photos.rs`, `api/photos_misc.rs`,
  `repo/photos_hash_repo.rs` and `tasks/mod.rs` (it came in with `main`'s `8457760`). Agents are told
  to leave those 4 files alone. **Fixed** with the owner's OK: `webui` `aae45e5` "Format with rustfmt"
  (only those 4 files; `cargo fmt --check` is clean). Rebase `upload-name` and WP3 onto it.
- **Upload-name fix MERGED:** `webui` `e19c337` "Reject unsafe upload file names" (fast-forward, on
  `aae45e5`). Coordinator checks: 127 Rust, clippy and fmt clean, 34 unit, 272/20 e2e. Edge cases that
  differ from `main` (all from WP1's `sanitize_file_name`, accepted): `..hidden`, `" .. "` and
  whitespace-only names become `<uuid>`, and names are trimmed. A NUL name fails in multipart parsing
  (500, same as main). Worktree and branch removed.
- **WP7 started:** worktree `.claude/worktrees/wp7`, branch `webui-wp7-upload` from `e19c337`, port 8116.
  It was told not to depend on WP3/WP4 and to leave seams for them.
- **WP3 review done** (`.claude/worktrees/reviews/wp3.md`). No must-fix: the folder page works in
  the browser (permissions are correct for bob read-only, a `can_delete` grantee, the owner and public),
  but only through the JS fallback to the grid's `data-context`. Should-fix: S1 `folder_page` doesn't
  call `viewer_context`; S2 Undo doesn't restore the tile (spec deviation); S3 a race in the trash/Undo
  e2e test; S4 a stale hand-off. 8 nice-to-haves. `/api`: only `pub(crate)` on the test harness. Checks:
  133 Rust, 42 unit, 302/28 e2e.
- WP3 rebased onto `e19c337` as `5382166` (no conflicts). **A fix agent is running** (port 8113): S1–S4
  plus the cheap nice-to-haves. Next: checks → squash-merge → start WP4 and WP6.

- **WP3 MERGED** as the squash `e8abc6a` (after the fix commit `fb537f5`: S1–S4 and N1–N3, N5–N8; N4,
  swipe velocity at release, is a known issue in `handoff/wp3.md` §7). Coordinator checks: 133 Rust,
  clippy, fmt and sqlx check clean, 42 unit, 305/31 e2e (one load flake in `grid.spec.ts:1034` while
  WP7 ran in parallel; it passed 10/10 on its own and the full rerun was green). `/api`: only `pub(crate)`
  on the test harness. Worktree and branch removed.
  **For WP4:** after the bulk Undo, emit `fp:photos-added {ids}` (new, in `core/events.js` and the
  architecture doc's §9) instead of reloading sections. The real `openMoveDialog` must not show its own
  toast. The `selection/move-dialog.js` stub is WP3's seam; replace it.
  **For WP6:** the favorites, trash and large-files pages must call `ctx.viewer_context(&context)`, like
  timeline and the folder page.
  **For WP7:** it branched from `e19c337` (before WP3), so rebase it onto `e8abc6a` before merging.
- **WP7 coded, NOT reviewed, NOT merged** (owner: no more agents this session). Branch
  `webui-wp7-upload` @ `dd62246` (one commit, rebased by the coordinator onto `e8abc6a`). There were
  union conflicts in `scripts/icons.txt` and `templates/input.css` (both sides kept), and
  `assets/icons/sprite.svg` was regenerated with `npm run icons`. Coordinator checks after the rebase:
  134 Rust, clippy, fmt and sqlx check clean, 58 unit, 341/35 e2e. `/api`, `.sqlx`, migrations: no diff.
  **Things for the review agent to look at:**
  - The app-wide CSP gains `'wasm-unsafe-eval'` in `script-src` (`src/http/mod.rs`) for the BLAKE3
    worker. Could it be scoped to `/upload` only, and is it acceptable at all? Ask the owner.
  - It vendors hash-wasm 4.12.0's BLAKE3 UMD file (22 KB, MIT, licence included): a new third-party
    dependency.
  - 3 retries = 4 attempts (follows Android; the plan says "up to 3 attempts").
  - `/api/upload` returns 401 both for an expired session and for "can't upload to this folder"; the
    page tells them apart with `/api/profile`.
  - The `/upload` heading changed to "Upload" (`src/http/tests.rs`, `shell.spec.ts`).
  - E2E: uploads run as carol with EXIF years 2045–2049 (WP3 uses 2031–2033); `can_upload` uses an
    alice temp folder shared with carol.
- **Owner, session 4: start no more agents after the WP3 fix agent and the WP7 coding agent finish.**
  The coordinator still runs the checks. WP3 (already reviewed) can be squash-merged if it's green.
  WP7 stays on its branch until a review agent runs in the next session.

- **Owner, session 4: after everything is done, do a performance pass**, especially of what runs on
  the server (queries per page/fragment, background tasks, preview/screen generation), and
  **re-evaluate every SQL index** we have. This goes into WP8 (or a follow-up). New or dropped indexes
  are migrations, so the owner must approve them.

## Session 5 (2026-10-29)
**Owner decisions, session 5:** WP4 and WP6 start now, **in parallel** with WP7's review and fixes. **WP7's CSP:** scope
`'wasm-unsafe-eval'` to `/upload` only; every other page keeps the strict policy.
- **Owner uses Firefox.** Decision: Firefox is a **coordinator-only check** before each merge; agents keep
  running Chromium only. `webui` `e99ee9f` "Add an opt-in Firefox e2e project": `FP_E2E_FIREFOX=1` adds a
  `firefox` project (Firefox 151 is installed in the main checkout's `node_modules`; copy that
  `node_modules` into a worktree, not wp7's). Run it with `--project=firefox`. A scratch worktree
  `.claude/worktrees/ff` (detached, port 8117) is kept for it. **Baseline on `e99ee9f`:** 140 passed,
  4 skipped, 3 failed:
  `viewer.spec.ts:432` (Firefox has no `clipboard-read` permission: test only), `viewer.spec.ts:475` (the
  H.264 fixture shows `video-error`; most likely Playwright's Firefox build has no H.264 decoder, while a
  real Firefox uses the system ffmpeg), `grid.spec.ts:428` (fast-scroller label empty; passed 3/3 alone, a
  load flake). Treat these 3 as known; new Firefox failures block a merge.
- **Owner report: "clicking a photo opens the wrong one".** Not a code bug: the viewer opens the right id.
  The owner's `storage/.previews` has thumbnails left over from older databases (files dated Jan–Jun 2026;
  ids were reused when `test.db` was rebuilt): 744 of 1749 checked previews are of a different photo, so
  tiles *show* the wrong image. All 54 `screen/` renditions (made today) are correct. Renditions are keyed
  by id only and sent `private, max-age=604800`, so the browser keeps stale ones for a week too. Code removes
  renditions on purge/scan, so production is only affected if a DB is reset while `PREVIEWS_PATH` is kept
  (WP8 could note it in the docs).
- **Started (in parallel):**
  - WP7 **review agent** (Opus, medium; a first Sonnet run was stopped right away) in `.claude/worktrees/wp7` (port 8116) → `.claude/worktrees/reviews/wp7.md`.
    The app-wide CSP is flagged must-fix (scope it to `/upload`).
    **Review done** (`reviews/wp7.md`): no `/api`, `.sqlx` or migration diff; all acceptance criteria met;
    134 Rust, 58 unit, 340/35 e2e (+1 `folders.spec.ts` flake, WP5's; it flaked twice, passes alone).
    M1 CSP: the hash **worker takes the CSP of its own script response**, so `/upload` *and*
    `/assets/js/upload/hash-worker.js` need `'wasm-unsafe-eval'` (still within the owner's "upload only"
    intent). S1: upload specs leave carol's photos and the "Uploads"/"Fresh" folders behind (fails with
    `--repeat-each=2`). S2: an expired session gives one `beforeunload` prompt per queued file and the wrong
    row message. **Fixed** in `e647cf4` (M1, S1, S2, N1, N2, N5; N3/N4 skipped, in the hand-off). CSP: one
    `content_security_policy!` macro → strict + upload policies; global layer is `if_not_present` (no other
    handler sets a CSP, checked); `/upload` route layer + `asset_cache_headers` for the worker; Rust test
    covers both sides. Agent: 135 Rust, 58 unit, 343/37 e2e, no `/api` diff. Coordinator rebased the branch
    onto `e99ee9f` (`60b36cc`, `6f181fb`, clean).
    **WP7 MERGED** as the squash `d86d1e9` "Rebuild the upload page on the new UI (WP7)". Coordinator checks:
    135 Rust, clippy/fmt/sqlx check clean, 58 unit, 343/37 e2e (Chromium), Firefox 154 passed / 4 skipped /
    the 3 known failures (`grid.spec.ts:428` failed in both full Firefox runs; it passes alone, so it may be
    order-dependent rather than load: look at it in WP8). Worktree and branch removed.
    **WP4 and WP6 must rebase onto `d86d1e9`** (or later) before their checks.
- **Owner approved tooling (session 5):** (1) JS type-checking (`checkJs`, `npm run typecheck`) and (2) a
  committed `scripts/check.sh` running every coordinator check (`--port`, `--firefox`, `--no-e2e`). A Sonnet
  agent is on it: worktree `.claude/worktrees/tooling`, branch `webui-tooling` from `d86d1e9`, port 8121. It
  must not edit WP4's (`grid/selection*.js`, `selection/*`) or WP6's (`pages/{utilities,trash,duplicates,
  settings}.js`) files. Merge it before WP4/WP6, which then rebase and must pass `typecheck`.
  **Extended (owner):** `noUnusedLocals`/`noUnusedParameters`, `knip` (unused files/exports/deps; delete
  dead code, ignore only documented future seams and WP4/WP6 paths) in `check.sh` (cargo-machete was dropped later). **Tooling coded** (`42944fa` typecheck + knip, `150078c` check.sh; on `d86d1e9`; no Rust diff).
  TypeScript 7.0.2 with `strict: false` + `checkJs`, `noUnusedLocals`, `noUnusedParameters`, `noImplicitThis`,
  `strictBindCallApply`, `noFallthroughCasesInSwitch`, `noImplicitReturns`. Left off: `strictNullChecks` (192
  errors), `noImplicitAny` (491), `strictFunctionTypes` (3), `useUnknownInCatchVariables` (9): candidates for
  WP8. Loosening: non-tag `querySelector`/`closest` return `any`. Playwright specs are type-checked too.
  knip removed 1 dead constant + 14 needless exports. tsc found no real bugs. cargo-machete isn't installed
  (skipped). check.sh: `--port`, `--firefox`, `--firefox-strict`, `--no-e2e`; DB = `<target>/fp-check.sqlite`
  (recreated each run, never `.env`). **WP4/WP6 after rebasing:** pass typecheck + knip; WP6 adds its page
  scripts to `entry` in `knip.jsonc`. **MERGED** as the squash `c076324` "Add JS type-checking, knip and a
  check script" after the coordinator's `check.sh --firefox` run (135 Rust, 58 unit, 343/37 e2e, Firefox
  154/4/3 known). Worktree and branch removed. The main checkout needs `npm install` (typescript, knip).
  **From now on the coordinator's checks = `scripts/check.sh --port <p> --firefox`.**
  **cargo-machete dropped (owner):** its only finding was a false positive (`kamadak-exif`, imported as
  `exif`); removed from `check.sh` and `tests.md` in `webui` `c1bf5f8` "Drop cargo-machete from
  the check script" (the interim `Cargo.toml` ignore commit was discarded before it was used).
  Plus the **bloat checklist** below for every review (owner approved items 1 and 2 of 6; not
  approved yet: a simplify pass before each merge, `CLAUDE.md` style rules). Later, for WP8:
  a debug-build stale-CSS warning, versioned rendition URLs (stale thumbnails), and in the perf pass: time
  preview generation on the owner's library, then add `-define jpeg:size=<2×size>` **before the input** in
  `run_magick` (`src/previews/generate.rs`: today it decodes full 24 MP JPEGs), time again; consider libvips
  (what Immich uses, via sharp) only if still slow or HEIC dominates. Owner: don't switch away from
  ImageMagick without measuring.
  - WP4 **coding agent**: worktree `.claude/worktrees/wp4`, branch `webui-wp4-selection` from `e8abc6a`,
    port 8114. Told to replace WP3's `move-dialog.js` stub, emit `fp:photos-added` after the bulk Undo,
    gate bulk "Delete forever" to own/public photos, and leave the trash/favorites e2e to WP6
    (listed under "For WP6" in its hand-off).
    **WP4 coded** (one commit on `e8abc6a`; 1448 code / 761 tests / 81 docs lines). Agent: 133 Rust, 51 unit,
    337/52 e2e, clippy/fmt/sqlx clean, no `/api`/`.sqlx`/migration diff. Adds `data-owner` on tiles
    (`PhotoTileView.owner`), `resolveDestination(…, {silent})`, `selection/keys.js` (not the `grid` keyboard
    scope: it broke a viewer deep-link test), "Remove from favorites" in the favorites bar; trash/favorites e2e
    for WP6 in `handoff/wp4.md` §5. Next: rebase onto `webui`
    after the tooling merge → typecheck/knip → review (with WP6) → fixes via SendMessage → checks → merge.
  - WP6 **coding agent**: worktree `.claude/worktrees/wp6`, branch `webui-wp6-utilities` from `e8abc6a`,
    port 8115. Told to call `viewer_context`, keep "Delete forever"/Empty trash owner-only, use no own
    selection, and add the selection e2e after WP4 is merged (the coordinator sends it back to merge `webui`).
    **WP6 coded** (on `e8abc6a`, not rebased; ~1900 code incl. inline Rust tests + sprite / ~600 tests / ~50
    docs). Agent: 143 Rust, 42 unit, 349/37 e2e, clippy/fmt/sqlx clean. `/api`: only the approved
    `POST /api/profile/password` (+1 `.sqlx`). New page route `GET /ui/trash/deletable-ids` (own + public
    trashed ids) for Empty trash. **For the review to scrutinise:** trash e2e actions are mocked with
    `page.route` (never hit the server) to avoid racing other specs on shared users; `loginAs` now retries 5 s
    (carol's password window); new carol duplicate fixtures `dup-a.jpg`/`Copies/dup-b.jpg`. The shared e2e
    users (alice/bob/carol) are becoming a source of races: consider per-spec users in WP8.
    **Left after WP4 merges:** selection import on favorites/trash/large-files pages, their selection e2e
    (`handoff/wp4.md` §5), rebase, typecheck/knip.

## Resume here
**Session 9: WP9 9.3, 9.2 (service worker) and the 9.4 swipe-velocity fix are committed on `webui`; next 9.4 pinch-zoom on the grid and the finger-following viewer swipe, see "Session 9".**

**Session 8: login and htmx 4 merged, `webui` = `dc7f866`. `webui` = `938b880` on `main` `7ff3e85` (no docs in it). Next: WP9 (see "Session 8").**

**Start of session 7: WP8.** Rules: fixes go back to the original agent, bloat checklist, Firefox check.
Read the Session 6 section below for the details of everything decided.

**State:** `main` (local, not pushed) = `8feee10` → `66ab395` sqlx 0.9 + deps → `cbd4da8` write-lock fix →
`e510651` upload fix. The owner has `main` checked out in the main checkout. **`webui` = `3220f53`, rebased onto
`main` (14 commits; old tip tagged `webui-pre-main-rebase` = `75dcf72`)**, coordinator `check.sh --firefox` green
(151 Rust, 67 unit, 422/73 e2e, Firefox 205/6/3 known). Root `CLAUDE.md` rewritten for `webui` (untracked).
**WP8 is RUNNING** (Sonnet): worktree `wp8`, branch `webui-wp8-hardening` from `3220f53`, ports 8128 (e2e) /
8129 (perf). It keeps `docs/web-ui-plan/handoff/wp8.md` (checklist + "Resume here") and makes a `WIP WP8: …`
commit after every item: **if it was cut off, resume from that file** (new agent, same worktree). **`main` is now
`edc114a`**: + `dea181d` "Extend sessions once a day while they're in use" (`http/session_activity.rs`, `last_seen`
day in the session, after the handler) + `edc114a` "Don't log trashed photos as present in a folder" (the
`update_photo` branch, and `insert_folder_creation_event_logs` skips trashed photos, which also fixes
`update_thumb_hashes`; 98 tests, clippy clean). **`webui` is NOT rebased onto these two yet**: do it when WP8 is done (rebase `webui`, then WP8
on top), not while WP8 runs.
Owner: no "mark nullable returned columns" commit on `main`. Perf scripts kept in
`.claude/worktrees/reviews/perf-inputs/`.

**WP8 = `wp8-hardening.md` plus these owner decisions (session 6):**
- Perf testing on a **200 000-photo** seeded DB (not 50k), mirroring prod's mix of date formats.
- **All** of `reviews/perf.md`'s speed-ups, including: timeline "On this day" N+1; `/ui/grid/month` start/count
  query; favorites `exists` → `in`; summaries without the window; `folder_cards` rewrite; folder page double
  access check; info panel 3 → 1 queries; previews N+1; thumbhash 2 → 1 `magick`; preview
  `-define jpeg:size=640x640` (not for screen renditions); the sync/scan `(? is null …) or …` query; blocking fs
  in async tasks.
- **ONE migration, with a down migration where possible:** normalize old `created_at` text
  (`'YYYY-MM-DD HH:MM:SS'` → `'YYYY-MM-DDTHH:MM:SSZ'`, 52 083 of 100 903 prod rows; the down can't restore
  it, so it only restores the index) + `idx_photos_folder_created (folder_id, created_at desc, id desc) where
  folder_id is not null` replacing `idx_photos_folder_id`. Verify sqlx accepts a reversible pair next to the simple
  migrations; test up and down. Then switch month matching from `strftime` to a date range (update the
  architecture doc §10 invariant) and make every write store UTC `…Z`.
- **File scan:** read existing photos via `read_pool`, walk the disk + EXIF with no connection held, then one short
  write tx per user that re-checks each new path (no duplicates with concurrent uploads/scans).
- **Leave alone:** move and trash cleanup holding the write connection (keeps DB and files consistent).
- **Not now:** response compression (prod is behind a Ferron v2 proxy; make the HTML smaller first: the timeline
  page is 497 KB). Dropped: versioned rendition URLs, the debug stale-CSS warning.
- Also: per-spec e2e users; stricter tsc flags (ask which); root `CLAUDE.md` update.

**Order after WP8 (owner, session 6):** (1) rebase `webui` onto `main` `edc114a`, WP8 on top → checks →
squash-merge WP8; (2) **HTMX upgrade** (2.0.11 → the current major, htmx 4; vendored `assets/vendor/htmx.min.js`
+ licence; ~35 `hx-*` attributes in `templates/` and ~10 `htmx:*` listeners in `assets/js` (registry mounting on
`htmx:afterSettle`/`htmx:load`, cleanup on `htmx:beforeCleanupElement`, error toasts on
`htmx:responseError`/`sendError`), the `HX-Redirect`/`HX-Trigger` response headers in `http/fragments.rs`).
Read the official 2→4 migration guide first (**owner, session 7: give the Sonnet agent
https://htmx.uk/docs/get-started/migration/ and have it read the whole page, incl. "Checklist" and "Migration Notes"**) (events renamed, attribute inheritance explicit, fetch-based,
error responses/history handling changed); keep behaviour identical; `check.sh --firefox` green; one squash
commit "Upgrade htmx to 4" — do it BEFORE WP9 because WP9 switches tab navigation to `hx-boost`.
**Step 2 of the same package (owner):** move the request → server HTML → swap cases from JS to htmx:
forms and dialogs that today open via `htmx.ajax` and then submit through `api.js` and patch the DOM by hand
(settings password change, folder name dialog, sharing dialog, folder create/rename/delete and the folder list
refresh) become `hx-post`/`hx-get` with the server returning the updated fragment or the form with its errors.
New HTML routes go under `/ui/*` next to the JSON `/api` ones, which stay unchanged for Android. Keep the
grid, viewer, selection/drag-select and upload queue in plain JS. Delete the JS that becomes unused (knip).
Second commit "Move forms and dialogs to htmx";
(3) **WP9 "Native feel"** (owner, session 7: desktop + Android; **iOS is a separate WP10**, `wp10-ios.md`, likely delayed: the owner has no real iPhone): plan written in `docs/web-ui-plan/wp9-native-feel.md` (untracked in the
main checkout; commit it on `webui` with the progress file when the owner decides). Its 9.7 items (Web Push,
passkeys) need migrations → ask first.

**Open questions for the owner:** `BEGIN IMMEDIATE` for write transactions (protects against the CLI as a second
writer process) — on `main` now or in WP8?; an HTML size target; which tsc flags; marking failed
previews/thumbhashes (needs a migration); `scripts/sqlx-prepare.sh` (always prepare against a fresh DB).
Later: the 2048 px screen rendition; the `/api` findings in "Open items" 6 and 7; where `webui` goes at the end.

## Session 9 (2026-11-01): WP9 started, no agents (coordinator codes it)
- **9.3 done** (`Navigate between pages in place`): `<body hx-boost:inherited='{"swap":"innerHTML scroll:top","transition":true}'>`
  (shell pages only, via `body_attrs`); links that need a full load keep `hx-boost="false"` (upload, plain
  return-only/sharing forms, dialog backdrop). Scroll position per entry in `history.state.scrollY` (htmx 4 refetches
  on traversal), tapping the open tab scrolls to top (`nav-top`), reduced motion covers the root transition.
  Gotcha: never use `hx-target:inherited`/`hx-swap:inherited` on body, they hit every other hx-* element. New spec
  `navigation.spec.ts`. `check.sh --firefox` green (168 Rust, 65 unit, 467/84 e2e, Firefox 229 + 3 known).
  Second tap on Photos at the top scrolls the "On this day" row back to its first card (owner's choice). Next: 9.2 service worker.
- **`webui` is already on `main`** (`7ff3e85`, 21 commits on top; `d65f077`/`59586a9` are the session and trashed-photo
  commits): the old "not rebased onto `dea181d`/`edc114a`" notes are obsolete.
- **Owner: no `BEGIN IMMEDIATE`** for write transactions (closed).
- **tsc flags:** `strictFunctionTypes` and `useUnknownInCatchVariables` are on (amended into `f3e4b8c`; `isAbortError` in
  `core/api.js`, `register()` factories take `element: any`). Still off: `strictNullChecks` (231 errors in `assets/js`, 9 in the
  specs; owner hasn't decided), `noImplicitAny` (602 / 8; skip).
- **Chip padding and `hx-boost` audit:** both done and folded into `7f1131c` (the owner amended them in). Timeline chips use
  the folders page padding; `hx-boost="false"` removed from the 404 link and the sharing add-panel form, kept on the
  `/upload` links (own CSP), the dialog backdrop form and the folder name form (return-only mode has no `hx-post`).
- **9.2 done** (`575bc03` "Add a service worker for instant start and an offline page"): `assets/sw.js` served as `/sw.js`
  (`http/service_worker.rs` fills in `ASSET_VERSION` + the precache list from `css js vendor icons`, not `fav`).
  Release: shell precached per version (73 files, ~590 KB) and served from that cache, a new worker waits and the
  "A new version is available" toast (`core/service-worker.js`) lets it take over, then reloads. Previews cache-first
  (cap 5 000, oldest first, `no-store` fallbacks skipped), emptied on sign-out and on opening `/login`, entries
  dropped on `fp:photos-removed`. Navigations fall back to `assets/offline.html` (icon inlined: Chromium doesn't send
  SVG `<use>` fetches through the worker). Dev server (`ASSET_VERSION=dev`): offline page only. Playwright now blocks
  service workers by default (config + hand-made contexts); only `service-worker.spec.ts` allows them (its offline test
  is skipped in Firefox: its offline emulation doesn't reach the worker's own requests). Differences from the plan: FIFO
  not LRU; no background revalidation of JS (per-version cache instead). `check.sh --firefox`: 171 Rust, 69 unit,
  483/84 e2e, Firefox 234/11 + the 3 known (the 4th, the offline test, is now skipped there). Release build checked by
  hand in Chromium (assets from the worker, in-place navigation, offline page, no console errors).
  Settings has a Storage row "Cached previews" with a Clear button (the page deletes the `fp-previews` cache itself; misses
  are fetched with `cache: 'no-cache'`, so stale HTTP-cached thumbnails of a reused id are revalidated). Amended into
  the same commit. Owner asked about a bundler: not now (revisit with compression on; an esbuild experiment on the 200k
  perf setup would decide). Idea for later: `modulepreload`
  links per page (some pages import 20+ modules on a cold load; not measured).

- **9.4 swipe-velocity fix done** (`Measure the swipe velocity at release`): `releaseVelocity` (last 100 ms of pointer samples,
  `viewer/math.js`) replaces the whole-gesture average in `swipeOutcome` (`vx` instead of `duration`); a flick navigates whatever
  the distance, a flick against the drag cancels, a finger that rested before lifting is at rest. Unit tests + a CDP touch e2e
  with explicit timestamps (fails on the old logic, passes now). Vertical close is still distance-only (a flick-down close is not
  done). Don't run two Playwright projects at once against one server: the state-changing specs race (a false failure in
  `viewer.spec.ts:355`). Next 9.4: pinch-zoom on the grid, finger-following viewer swipe.

- **Prod risks (owner asked, nothing decided):** (1) the new service worker: `/sw.js` is `no-cache`, so a fix reaches users and a broken
  worker can't pin itself, but it was only hand-checked in a Chromium release build, so try it on the owner's own devices first;
  (2) service workers and the `Secure` session cookie need HTTPS (Ferron terminates TLS: check it forwards the scheme);
  (3) the WP8 migration (normalises ~52 000 `created_at` values, swaps an index; migrations run on startup): back up and run on a
  copy of prod first; (4) the approved `/api` changes (same-timestamp order by id, 500 on an fs-check error in delete/upload,
  new preview bytes, stricter upload names, the new password endpoint): **the Android app has not been smoke-tested against
  `webui`**; (5) reused photo ids after a DB reset show old thumbnails (Settings > Storage clears the browser side; the server's
  `PREVIEWS_PATH` needs clearing too); (6) no response compression yet; (7) the CLI scan is a second writer (writes wait up to 60 s);
  (8) untested: WebKit/iOS (WP10), gestures on a real phone, Firefox's 3 known failures + the skipped offline test.
  Offered: a deploy checklist (backup DB + previews, Android smoke test, staged rollout, rollback = `main`).

## Session 8 (2026-10-31)
- **Owner: redesign the login page (web + Android) before htmx 4.** Sonnet agent in worktree `login` (branch
  `webui-login` from `39c3554`, e2e port 8134): no card; abstract tile-mosaic backdrop; split layout ≥ 900 px,
  top band on narrow screens; labelled 52 px fields without leading icons. Behaviour, HTMX, testids unchanged.
  Screenshots in `/tmp/fp-login/`. A matching prompt for the Android app was given to the owner. Next: owner
  looks at the screenshots → coordinator `check.sh --firefox` → squash-merge.
- **Login coded:** `webui-login` = `1c9fcab` "Redesign the login page" (auth/a11y/shell green; button text now
  "Sign in") + `213e322` coordinator fix "Pin playwright-core so axe uses Playwright's copy" (a fresh `npm install`
  pulled playwright-core 1.64 for `@axe-core/playwright` → typecheck failed in `a11y.spec.ts`; the main checkout's
  node_modules has no `@axe-core/playwright` at all: run `npm install` there). Typecheck/knip/67 unit green.
  **Owner: likes the tiles, keep the design as is.** Coordinator check: 161 Rust, 67 unit, 461/84 e2e, Firefox 225/11 +
  the 3 known (rerun after `npx playwright install firefox`: a fresh worktree's node_modules has no Firefox, so
  check.sh's first Firefox run failed 208 at launch). **LOGIN MERGED: `webui` = `213e322`** (fast-forward); worktree
  and branch removed, `ff` moved. Owner: `npm install` in the main checkout.
- **Owner: stop after the running agents finish (usage).** No review or new agent until the owner says so.
- **Owner: htmx 4 in parallel.** Sonnet agent in worktree `htmx4` (branch `webui-htmx4` from `39c3554`, port 8135):
  commit 0 fixes the two racy specs, commit 1 "Upgrade htmx to 4" (4.0.0, migration guide first), commit 2 "Move
  forms and dialogs to htmx" (+ `/ui/*` handler tests), note in `handoff/htmx4.md`. Only the login form's `hx-*`
  attributes overlap with the login agent: merge login first, then rebase htmx4. Then: Opus review (big package)
  → coordinator `check.sh --firefox` → merge.
- **The htmx4 agent was cut off by the usage limit** after `58d9ef3` (racy tests) and `002846b` "Upgrade htmx to 4". Commit 2
  was uncommitted in the worktree (~31 files, no `handoff/htmx4.md`). A new Sonnet agent picked it up in the same worktree
  (port 8135). **htmx4 coded:** `529b599` "Move forms and dialogs to htmx" (+ `handoff/htmx4.md`); `check.sh --no-e2e`
  green (167 Rust, 65 unit), scoped e2e 136 passed / 2 skipped (Mobile Chrome password specs). Not rebased, no Firefox.
  **Owner: go ahead.** Rebased onto `213e322` (no conflicts): `87bf0fc`, `809062c`, `2493f8f` (a stray
  `test-results/.last-run.json` removed from it); `--no-e2e` + touched specs green. **Opus review done** (`reviews/htmx4.md`:
  S1 Enter in the custom-date field shares with the first member; B1–B6; N1–N12; Chromium+Firefox touched specs green).
  **Owner decisions:** keep the clamped "1 month" expiry (note it); server-only validation + B6; add an `Origin` check
  on non-GET `/ui/*` (403 if present and foreign). Fixes S1, B1–B6, N1–N8, N10, N12 sent back to the coding agent
  (skip N9; N11 `hx-boost="false"` left for WP9). **Fixes done:** `87bf0fc`, `1b288ea`, `a6cedde` (168 Rust, 65 unit,
  touched e2e 202/8 skipped; `same_origin_writes` middleware). **Owner: clean up the commits** → the same agent splits
  `a6cedde` into password / folders / sharing / cross-origin check (tree identical, each commit builds + tests).
  **Owner: cherry-pick `1e9508f` (serde_with → helpers) onto `main`:** `main` = `7ff3e85` "Replace serde_with with a small
  serde helper" on `59586a9` (only `base64_option`: `context.rs`/`expires_at` are webui-only, so `double_option` was
  dropped there; 99 tests, clippy only the 2 known). `webui` is not rebased onto it yet.
  **Split done:** `87bf0fc`, `1b288ea`, `bf220b7` password, `ca579af` folders, `bc9e277` sharing, `476b416` cross-origin
  check (tree = `a6cedde` + handoff SHAs; each commit builds + tests; old tip kept as branch `backup-a6cedde`).
  Coordinator `check.sh --firefox` on `476b416`: 168 Rust, 65 unit, 461/84 e2e, Firefox 226/11 + the 3 known.
- The owner squashed login + the playwright-core pin into `6220b8b` "Redesign the login page" (tree = `213e322`);
  htmx4 replayed onto it (trees unchanged; handoff note now names commits by title, not SHA).
  **HTMX 4 MERGED: `webui` = `dc7f866`** (`943edd8`, `c573c35`, `a766d87`, `cbf1ef3`, `d5c8af5`, `dc7f866`). Worktree
  `htmx4` and branches removed; `ff` moved. Nothing pushed.
- **Next:** rebase `webui` onto `main` `7ff3e85` (its "Replace serde_with with two small serde helpers" will
  conflict lightly in `serde_helpers.rs`: keep webui's `double_option`), then WP9 (`wp9-native-feel.md`) in a fresh
  session. Open: `hx-boost="false"` cleanup (review N11) belongs to WP9; N9 (`get_share`) skipped.
- **Rebased `webui` onto `main` `7ff3e85`: `webui` = `f1211f5`** (24 commits; tree identical to `dc7f866`, so no re-check;
  old tip tagged `webui-pre-serde-rebase`). webui's serde commit is now `fcb24cd` "Add a double_option serde helper".
  The owner has `main` checked out in the main checkout. **Next: WP9 in a fresh session.**
- **Owner: no docs on `webui`.** History rewritten (filter-branch over `main..webui`): `docs/web-ui-plan/` removed from
  every commit, "Add the web UI rewrite plan" dropped, "(WPn)" stripped from titles, and every comment/TODO/README
  reference to the docs or WP numbers rewritten in the commit that introduced it (no commit mentions them now).
  **`webui` = `938b880`** (23 commits; code = `f1211f5` except those comments). Tags `webui-pre-docs-removal`,
  `webui-pre-docrefs`. The docs now live only as untracked files in `docs/web-ui-plan/` of the main checkout.
  From now on: hand-off notes and plans stay untracked; agents must not commit them or reference them in code/commits.

## Session 7 (2026-10-31)
- The first WP8 agent was cut off during item 5 (smaller HTML): items 1–4 are WIP commits up to `fd0fde8`;
  uncommitted item-5 work in `wp8` (`askama.toml` whitespace = "minimize", `grid/macros.html`, `macros/icons.html`,
  `macros/photo.html`). A fresh Sonnet WP8 agent resumes from `handoff/wp8.md` in the same worktree (ports 8128/8129).
- The owner rewrote `main` (`481c565` write-lock, …). Then, at the owner's request, the `upload.rs` tests were shortened and
  folded into their commits: **`main` = `66ab395` → `481c565` → `c855981` uploads → `dde23ab` sessions → `b8a427c`
  trashed-photo log** (98 tests, clippy only the 2 known warnings). Rebase `webui` onto this `main` after WP8.
  The owner then folded the `.dockerignore` allowlist (`.claude/` was 20 GB of build context) into `main`: now
  `c855981` → `d65f077` → `59586a9`. The main checkout is now on `webui`.
- **WP8 coded** (items 5–11, tip `bdcb6d7`): agent's check.sh 159 Rust, 67 unit, 465/74 e2e; no Firefox run; WebKit
  can't launch on this host (missing system libs). New specs: security, android-api, a11y; per-spec users frank…judy.
  **Opus review running** → `reviews/wp8.md` (incl. Firefox).
- **WebKit on this host (owner: OK, low priority):** Playwright's `webkit-2311` is built for Ubuntu 24.04; Arch lacks
  ICU 74, `libxml2.so.2`, `libjxl.so.0.8`, `libbacktrace.so.0` and some flite voices. Plan: a `check.sh --webkit` that
  runs the `webkit` + `Mobile Safari` projects in `mcr.microsoft.com/playwright:v<ver>-noble` against the host's e2e
  server. Part of WP10 (iOS), not before.
- **Owner: Web Push and passkeys are for later** (both need migrations); **Web Push is the more useful one**. When it
  comes: one `push_subscriptions` table and one sender for web and Android (the Android app could use UnifiedPush,
  which speaks the same Web Push protocol, or FCM as a second sender; or local notifications from its periodic sync
  with no server change). Subscription routes for Android would be additive `/api` endpoints.
- **WP8 review done** (`reviews/wp8.md`; check.sh + Firefox green, only the 3 known Firefox failures). Fixes S2–S5,
  B1–B4, cheap N items (incl. N9 CLAUDE.md) sent back to the WP8 agent; M1 waits for
  the owner. **Owner approved the `/api` behaviour changes:** same-timestamp order by id, same-day photos in true time
  order after the migration, 500 on an fs-check IO error in delete/upload, new preview bytes (`jpeg:size`).
  **S1 pre-check on prod: no rows** (owner, session 7): every non-`Z` `created_at` parses, the migration is safe; owner: no guard in the migration.
- **M1 `perf.spec.ts` (owner: yes):** Sonnet agent in worktree `perf-spec` (branch `webui-perf-spec` from `bdcb6d7`,
  e2e 8131, perf server 8132, 200k seed in `/tmp/fp-perf`). Opt-in spec (`PERF=1`, `PERF_BASE_URL`). Cherry-pick its
  commit(s) onto WP8 after the review fixes.
- **Comment cleanup DONE:** `webui-comments` `add790e` (33 lines removed, comments only; check.sh green). Merge it
  with WP8 after the review fixes.
- **Owner: comment cleanup.** Sonnet agent in worktree `comments` (branch `webui-comments` from `bdcb6d7`, port 8130)
  deletes restating comments in `git diff e510651 HEAD` → one commit "Remove comments that restate the code". Combine
  with WP8's review fixes (rebase one onto the other) before the WP8 squash-merge.

- **Owner: merge WP8 now, perf spec afterwards.** Squashed in worktree `rebase3` (branch `webui-on-main3` from
  `3220f53`): `7bfd2cc` WP8 (from `389d7a2`) + `21ad9c3` comment cleanup. A Sonnet agent rebases it onto `main`
  `59586a9` (port 8133). Then: coordinator `check.sh --firefox` → move `webui` (the owner has `webui` checked out in the
  main checkout) → cherry-pick the perf-spec commit when its agent is done.

- **Rebase done:** `webui-on-main3` = `main` `59586a9` + 17 commits, tip `39c3554` (… `6226862` WP8, `911d44e` comments,
  `39c3554` "Add an opt-in browser perf spec"). Coordinator check (on `911d44e`): 161 Rust, 67 unit, 459/74 e2e + 2
  failures, Firefox 223/6 + the 3 known + the same 2. Those 2 pass 20/20 (Chromium) and 10/10 (Firefox) alone; they are
  test races: `folders.spec.ts:251` (SQLite reuses the deleted folder's id for a parallel spec's new folder → 200) and
  `sharing.spec.ts:145` (expiry `now + 3 s` under load). Fix both in the next package. Perf spec numbers in
  `handoff/wp8.md` 8.2 (all budgets met except long tasks: max 165–341 ms, headless software raster; follow-up idea:
  decode thumbhashes in idle slices, measure on a real GPU first).
- **WP8 MERGED: `webui` = `39c3554`** (on `main` `59586a9`; old tip tagged `webui-pre-wp8` = `3220f53`). Worktrees
  `wp8`, `comments`, `perf-spec`, `rebase3` and their branches removed; `ff` moved to `39c3554`. Nothing pushed.
  **Next: `/clear`, then the htmx 4 upgrade (Sonnet coordinator; see the usage rules), with the two flaky tests fixed
  first or in it.** Owner still to smoke-test the Android app against `webui`.

**Usage rules (owner, session 7; defaults from now on):**
- Test runs: the coding agent runs `check.sh --no-e2e` plus only the specs it touched; the reviewer reads the diff
  and runs only the specs that diff touches; **the coordinator does the one full `check.sh --firefox` before merging.**
- Agent reports ≤ 200 words; prompts short (point at files instead of repeating them).

## Session 6 (2026-10-30)
- WP4 rebased by the coordinator onto `10c25bd` (`b13276b`, `4edafc8`; clean). A fresh Sonnet agent is doing
  the `aria-disabled` fix in `wp4` (port 8114). Next: coordinator `check.sh --port 8114 --firefox` →
  squash-merge → rebase WP6 → WP6 fixes (fresh Sonnet agent, the original isn't resumable).
- **Owner: run all agents.** WP6 rebased onto `10c25bd` (`c78a982`, clean). A fresh Sonnet WP6 fix
  agent runs in parallel (`wp6`, port 8115): phase A = items not needing WP4 (knip entries, M1, S1, S3, S4,
  bloat, `library.ts` reuse) → WIP commit → it stops and waits. After WP4 merges, SendMessage it for phase B
  (rebase on `webui`, S2 `chunk`/`runPool`, M2 selection e2e, `check.sh --firefox`, one fix commit).
- WP4 agent: the `aria-disabled` fix was already in `4edafc8` (no new commit); coordinator verified `bar.js`.
  Coordinator `check.sh --port 8114 --firefox`: 137 Rust, 67 unit, 377/60 e2e, Firefox 177/5/3 known; no
  `/api`/`.sqlx`/migration diff. **WP4 MERGED** as the squash `26f5656`. Worktree and branch removed. WP6 agent
  told to continue with phase B.
- **WP8 perf + SQL-index analysis started early** (Opus medium, read-only): detached worktree `perf`, DB copy
  in `/tmp/perf`, port 8123 if needed → `reviews/perf.md` (queries per request, `explain query plan` per index,
  background tasks, `run_magick` with/without `jpeg:size`, ranked recommendations; index changes = proposals).

- **Owner bug (fix on `main` too): the app is unusable while photo hashes are computed.** Causes: (1)
  `tasks/hash.rs`/`thumb_hash.rs` hold a `write_pool` tx (1 connection) during all the hashing; (2) every
  request takes the write connection for reads: the session store (`SqliteStore::new(write_pool)`) and
  `UsersRepository` (axum-login's user load) both use it. (tower-sessions 0.14 only saves modified sessions.)
  **Owner's design (one commit on `main`):** hashes + thumbhashes in batches of 256 (keyset read on `read_pool`,
  hash with no connection, one short write per batch, failed writes logged and skipped); `UsersRepository` reads
  on `read_pool`, writes on `write_pool`; the session store gets its own pool (2 connections, not read-only).
  `file_scan`, `trash` untouched (the first agent's broader attempt was discarded). Sonnet agent in worktree
  `writelock`, branch `fix-write-lock` from `8feee10`. Then: coordinator checks → fast-forward `main` →
  cherry-pick into `webui` (WP6's password change uses `UsersRepository`: expect a conflict).
- WP6 fixed (`b15ffb4` on `f0856f9`, rebased on `26f5656`): agent's check.sh 146 Rust, 67 unit, 422/73 e2e,
  Firefox 205/6/3 known. `/api` diff: only the approved `POST /api/profile/password`. Coordinator check.sh:
  the same counts, RESULT ok. **WP6 MERGED** as the squash `75dcf72`. Worktrees `wp6`, `perf` and the branch are
  removed; `ff` moved to the `webui` tip. **Phase 3 is complete.**
- **The owner rewrote `webui`** (Oct 31, 00:05) before WP6 merged: the tooling and "drop machete" commits are
  one commit `009528c`; then `03d21e9` serde, `6338c54` WP4. The final tree is identical to `26f5656`. WP6 was
  rebased onto it (`--onto webui 26f5656`, same tree as the checked `b15ffb4`).
- **WP8 perf report done:** `.claude/worktrees/reviews/perf.md` (scripts and raw data in `/tmp/perf/`). Top items:
  every logged-in request does 2 write-pool queries (the `fix-write-lock` change fixes that); **`/api/upload`
  opens its write tx before the body is received** (`api/upload.rs:136`→`:148`; a slow 6 MB upload stalled
  everyone for 8.5 s); `file_scan` holds the tx during the disk walk; timeline: 15 queries/196 ms, 10 of them
  "On this day" covers (`timeline.rs:144`); `/ui/grid/month` 81 ms → ~8 ms with rewrites (the month-filter
  one changes a rule in the architecture doc: owner); other query rewrites (favorites, month counts, folder
  cards); no HTML compression (497 KB → 25.6 KB gzip); one proposed migration (`idx_photos_folder_id` →
  (folder, date, id)); `jpeg:size=640x640` makes previews 2.1× faster; don't use it for screen renditions.
- The coordinator stopped all agents by mistake when the owner asked to stop only the write-lock agent; WP6
  and perf were resumed.

- **Owner decisions (after the perf report):** (1) the upload fix is a **second commit on `fix-write-lock`**
  ("Receive uploads before taking the write connection"; sent to the write-lock agent). (2) **sqlx 0.9 next, on
  `main` first**, before WP8. A Sonnet agent runs in worktree `sqlx9`, branch `sqlx-0.9` from `8feee10` (the
  session-4 plan: git revs of upstream `axum-login`/`tower-sessions-sqlx-store`, a manual session-survival
  check on port 8125). Order on `main`: `fix-write-lock` (2 commits) → the sqlx agent rebases → "Update sqlx to
  0.9" → then both are brought into `webui` (cherry-pick, or rebase `webui` onto `main`; ask the owner) → WP8.

- **Owner: also update the other Rust deps on `main`** with the sqlx update: a second commit "Update
  dependencies" on `sqlx-0.9` (`cargo update` + every direct dep to its latest release; majors that need big
  rewrites or change behaviour are held back and listed). npm not included.
- `fix-write-lock` coded: `fab8eb3` (batched hashes/thumbhashes, users on `read_pool`, `Pools {read, write,
  session}`) and `c1e639f` (upload body received before the write tx; the target is re-resolved inside it). Agent:
  92 tests, clippy only the 2 known warnings. **Opus review running** → `reviews/writelock.md` (it also checks
  that the new concurrency tests fail on the old code, and why `cargo sqlx prepare` rewrote 8 unrelated `.sqlx`
  files on `main`).

- **Perf decisions (owner, prod has ~100k photos, so all speed-ups go into WP8):** index migration
  `idx_photos_folder_created` **approved**; the 4 `main`-relevant items (file-scan lock, thumbhash 2 → 1 `magick`,
  preview `jpeg:size=640x640`, the sync/scan `(? is null …) or …` query) go into **WP8 on `webui` only**;
  **compression: not now** (the owner wants smaller HTML first; prod sits behind a Ferron v2 reverse proxy that may
  compress); **month range (Q3): pending** the owner running the `month_mismatch` / `not_utc_z` query on prod.
  If adopted, WP8 must also guarantee UTC on every write.
- **sqlx 0.9 + deps coded** on `sqlx-0.9` (on the old `main` 8feee10, not rebased yet): `de52a63` "Update sqlx to
  0.9" + "Update dependencies". `axum-login` git rev `151c72d`, `tower-sessions-sqlx-store` rev `d18c9bf`, one
  `tower-sessions` 0.15. Sessions survive (old cookie → 200). 89 tests, `sqlx prepare --check` clean, all 66 `.sqlx`
  rewritten (new format). **Trap:** sqlx 0.9 infers `insert … returning *` columns as non-null (NULL → `""`/`0`
  silently); fixed with explicit columns + `as "x?"` in 3 queries on `main`; **check every `returning` in `webui`**
  when bringing it over. Next: after `fix-write-lock` lands, the agent rebases + regenerates `.sqlx` → checks.

- **`main` is now `8feee10` → `66ab395` "Update sqlx to 0.9 and the other dependencies"** (local, not pushed).
  Coordinator checks: 89 tests, clippy only the 2 known warnings, release build; an argon2 rc.8 hash logs in on
  the 0.6.0 build; sessions survive (agent). The owner's "remove useless comments" cleanup (13 restating
  comments from `ddf8b67`, which is already on `origin/main`) is squashed into it. (The coordinator briefly moved
  `main` to the `webui` tip by a bad `cd` and restored it via `commit-tree`; no push.)
- **Write-lock review** (`reviews/writelock.md`): M1 the separate session pool is a second writer → other
  write txs fail at once with "database is locked" (busy_timeout doesn't apply to a read→write upgrade after
  another commit). Proposed fix: a ~25-line `SessionStore` wrapper (load on `read_pool`, writes on `write_pool`),
  drop the session pool. M2 a batch panic now kills the periodic tasks → **owner: log it and continue**. Should-fix:
  a real hashing test, a logged-in GET while the write connection is busy, re-try a batch minus a deleted photo,
  upload temp file in the storage folder. Owner asked whether the read pre-check → write tx in `c1e639f` can
  de-sync: no, the target and dedupe are re-read inside the write tx; it relies on a single writer (M1).
  **Pending:** the owner's OK for the wrapper, then send M1/M2/should-fix + comment trim back to the agent, which
  rebases onto `66ab395` and regenerates `.sqlx`.

- **Owner: go ahead with `fix-write-lock`.** Sent back to its agent: rebase onto `66ab395`, M1 `SessionStore`
  wrapper (no session pool), M2, should-fix, bloat + restating comments removed, `.sqlx` from a fresh empty DB
  (only its own queries), session-survival check vs the `66ab395` build. Then: coordinator checks → ff `main`.
- **`.sqlx` churn explained:** sqlx infers SQLite expression column types from the query plan, which depends on
  `sqlite_stat1` (the write pool's `optimize_on_close` runs `analyze`), so `prepare` on `test.db`, on a fresh DB
  and on an agent DB give different `type_info`/`nullable` for 3 queries (`2ec654…`, `8f4f0a…`, `a94211…`). All
  have type overrides, so the generated code is the same. Proposed (owner hasn't answered): a
  `scripts/sqlx-prepare.sh` that always prepares against a freshly migrated empty DB.

- **`main` = `8feee10` → `66ab395` sqlx 0.9 + deps → `cbd4da8` write-lock fix (session-store wrapper, batched
  hashes/thumbhashes, users on `read_pool`) → `e510651` uploads before the write connection.** Local, not pushed.
  Coordinator checks: 94 tests, clippy only the 2 known warnings, release build; the two new concurrency tests
  fail on the old code (agent); sessions from the `66ab395` build survive (agent). `prepare --check` on a fresh DB
  differs only in the known churn file `2ec654…`. `main` was fast-forwarded in the main checkout (the owner has
  `main` checked out there). Next: bring `main` into `webui` (ask: cherry-pick or rebase `webui` onto `main`).
- **WP8 decisions (owner):**
  - **Month by date range: yes**, plus **normalizing old dates** (`'YYYY-MM-DD HH:MM:SS'` → `…T…Z`, 52 083 of
    100 903 prod rows) **and the new folder index (`idx_photos_folder_created`, drop `idx_photos_folder_id`) in
    ONE migration, with a down migration where possible** (the index part is reversible; the date rewrite can't
    be undone exactly, since the old-format rows aren't known afterwards: the down only restores the index; say
    so in a comment). Check that sqlx accepts a reversible `.up.sql`/`.down.sql` pair next to the existing
    simple migrations, and test the up and the down.
  - Versioned rendition URLs and the debug stale-CSS warning: **non-issues, dropped.**
  - Still open: HTML size target; the file-scan design; move/trash-cleanup write holders; marking failed
    previews/thumbhashes (migration); which stricter tsc flags; 200k-photo seed (decided: yes).

- **Owner: move/trash-cleanup write holders stay** (holding the write connection keeps the DB consistent with
  the filesystem). **Rebase `webui` onto the new `main` (`e510651`)**: a Sonnet agent in worktree `rebase2`,
  branch `webui-on-main2` (copy of `75dcf72`), ports 8121/8126. It drops the now-empty base64 bump, ports
  `main`'s upload design into `webui`'s upload.rs, `AuthSession::user().await`, `db::SessionStore` in the test
  harness, fixes every `returning` for the sqlx 0.9 nullability trap, `.sqlx` per commit from a fresh DB, and runs
  `check.sh --firefox` plus a session-survival check vs the `75dcf72` build. Then: coordinator checks → move
  `webui` → WP8.

## Remaining phases

Every package follows the same loop:
1. Worktree + branch from the current `webui`.
2. Coding agent (template + environment notes below).
3. Review agent → feedback sent to the coding agent → fixes.
4. The coordinator runs the checks (with `main.css` deleted first) and the acceptance criteria, and
   reviews the `/api` diff.
5. Merge in phase order and remove the worktree.

| Phase | Packages | E2E ports | Package-specific notes |
|---|---|---|---|
| 2 | WP2 Grid/Timeline, WP5 Folders & Sharing | 8111, 8112 | WP2: `wp2-grid-timeline.md` line ~88 still gives "503" as an example of a failed preview; `/api/preview` now falls back to the original. WP5 builds the folder-page grid after WP2 lands, so it may need a follow-up merge of `webui`. |
| 3 | WP3 Viewer, WP4 Selection, WP6 Utilities, WP7 Upload | 8113–8116 | WP6: decision 3 above. WP4 and WP6 depend on WP3's `openDeleteDialog`, and WP6 on WP4's selection bar, all built in parallel, so expect integration fix-ups; merge in the order WP3 → WP4 → WP6 → WP7. |
| 4 | WP8 Hardening/QA | 8117 | An index = a migration → ask the owner. Remove leftover `#[expect(dead_code)]` / `#[allow(dead_code)] // TODO(WP…)` markers. Update root `CLAUDE.md`. |

### WP4 + WP6 review (session 5, 2026-10-29)
- (The **combined WP4 + WP6 review**, Opus medium, ran after the
  coordinator rebased both onto `d86d1e9`: WP4 clean, WP6 union conflict in `scripts/icons.txt` → both kept,
  sprite regenerated. Reviews → `reviews/wp4.md`, `reviews/wp6.md`; integration tested in a scratch worktree
  `review-int`, port 8122.)
- **WP4 + WP6 review done** (`reviews/wp4.md`, `reviews/wp6.md`). No unapproved `/api` diff in either.
  WP4: M1 the Family-gallery move test leaves public trashed photos (they appear in every trash); S1 `keys.js`
  is a second shortcut system and the real bug is in `core/keyboard.js` `pushScope` (`+` zooms the grid behind
  a deep-linked viewer) → fix there and delete `keys.js`; bloat items. WP6: M1 the password test ends every
  other carol session for the run (`upload.spec.ts:331` 401) → dedicated user, `sharing.spec` accepts a 4th
  member, revert the `loginAs` retry; M2 selection e2e on trash/favorites/large-files (no import needed;
  recipes in `wp6.md`); S1 run real restores/deletes (narrow only the id lists); S2 reuse WP4's pool/chunking,
  Restore-all partial failure; S3 global `.fp-field` override; S4 duplicates SQL → one repo query; keep
  `/ui/trash/deletable-ids`. Checks: WP4 375/0, WP6 382/1, merged 413/2 e2e.
  **WP4 fixes sent to its original agent**: rebase onto `c1bf5f8`, fixes, `check.sh`.
  WP4 fixed in `eb3e11b` (on `c1bf5f8`): `core/keyboard.js` `pushScope` puts non-modal scopes below the first
  modal one + handlers may return `false` to decline (coordinator read the diff: sound); `keys.js` deleted;
  M1 + bloat + N1–N3. Skipped (WP6's to do): test-helper duplication with `library.ts`, `chunk`/`runPool`
  reuse in `trash.js`, the stale "add the selection import" line in WP6's docs. Coordinator `check.sh
  --firefox`: all ok, 135 Rust, 67 unit, 377/60 e2e, **but a new Firefox failure** `selection.spec.ts:366`:
  Firefox doesn't dispatch clicks on `disabled` buttons, so the "why it's disabled" toast never shows →
  sent back to WP4 (use `aria-disabled`, not `disabled`). Then re-check → squash-merge.
- **Send review findings back to the original coding agent** (`SendMessage`) instead of a
  fresh fix agent; **one Opus reviewer covers WP4 + WP6 together**; agents print only test summaries
  (`--reporter=line | tail`).

### Bloat checklist to give every review agent (owner, session 5)
Every review gets a **"Bloat" section** besides Must/Should/Nice-to-have. Each finding names a concrete
deletion or merge (file:line → what to remove or reuse), and real duplication is should-fix. Ask:
- Does the package reimplement something that already exists (`assets/js/core/*`, `grid/*`, `utils/`,
  `views.rs`, repo methods, template macros)? Name the existing helper.
- Dead code: unused functions, exports, parameters, options, CSS classes, template blocks, `data-*` attributes
  nothing reads, test helpers used once.
- Defensive code for impossible states: try/catch, null checks or fallbacks with no real failure case.
- Abstractions with a single user (wrappers, config objects, classes) that inlining would make clearer.
- Comments that only restate the code, and hand-off/docs prose that repeats the plan.
- Tests that pin implementation details or duplicate another test's coverage.
- Report the lines added (code / tests / docs) and say whether that's in proportion to the scope.
Coding agents also report their lines added; a package far above the plan must justify it in its hand-off.
Tools: `npm run typecheck` (with `noUnusedLocals`/`noUnusedParameters`), `npm run knip`
run in `scripts/check.sh` once the tooling branch is merged.

### Environment notes to give every agent
- Work only inside its worktree (absolute paths, `cd` into it for every command). Never touch port
  8080 or the main checkout's `test.db`.
- Root `CLAUDE.md` is untracked; agents read it at `/home/razvan/dev/Rust/FamilyPhotosServer/CLAUDE.md`.
- Cargo: use its own `DATABASE_URL=sqlite:<absolute worktree>/wp.db` (`sqlx database create &&
  sqlx migrate run`). Worktrees are nested in the main checkout, so sqlx's `.env` lookup otherwise
  finds the main `.env` (`sqlite:test.db`, relative) and fails. `SQLX_OFFLINE=true` works only for
  non-test builds. `cargo sqlx prepare` without `--all-targets`, so there are no test-only `.sqlx` files.
- Tests: follow `docs/web-ui-plan/handoff/tests.md` (shared `Dataset`, `TestServer`/`Client`).
- Playwright: `FP_E2E_PORT=<assigned port>` (the harness defaults to 8081 and fails fast if the port
  is busy). It needs Node ≥ 22.13. The harness now builds `main.css` itself.
- **No AI attribution trailer in commits.** No push, no merge by agents.
- Stop and report before: a migration, a non-additive `/api` change, or changing an Open-questions decision.

## Open items for the owner (5 is decided; the rest are unanswered)
- Unfixed production findings from the tests rewrite: `get_events_for_user` never returns `NoEvents` (the 409 in
  `api/sync.rs` is dead); `delete_old_folder_events(n)` cuts by global event id, not per-folder count; `If-Range` with
  the (weak) ETag never resumes, only `Last-Modified` does; moving a photo you can only delete into your own folder
  makes it yours; `GET /api/sharing/folder/{id}` shows other grantees to a grantee.
1. **FIXED in session 3 (see above).** Share expiry / trash cleanup date comparison (from WP1): dates are stored as text in two
   formats, so a share can expire up to a day late. Not fixed; the fix must change every access
   query together. Should a package (WP8?) do it?
5. **Event-log bugs: DECIDED in session 2.** The owner wants Android behaviour left alone unless a
   change is well motivated.
   - Folder-log trimming plus missed stale cursors: a phone that wasn't opened across a cleanup could
     silently miss shared-folder changes forever. **Approved fix:** keep the last 500 events per
     folder, and treat a cursor older than the folder's oldest remaining event as stale, so the
     server sends the full `photos` list (Android already handles that response). **Done:** on
     `main` as `d19ddfe` (local, not pushed) and cherry-picked into `webui` as `78f1328`.
   - `NoEvents`: the current behaviour is correct (empty log + cursor 0 → 200). **Done:** the dead
     variant is removed in the same commit, with no behaviour change.
   - A trashed photo updated in a folder logs an "added" event: **left as a known issue.** It's
     practically unreachable, and the only effect is a stray entry in a grantee's Android Trash list.
7. **New findings from the test rewrite (not fixed, details in `handoff/tests.md`):** moving a
   photo you can only delete (`shared_delete`) into your own folder makes it yours (the target
   folder's owner takes over); `POST /api/move` silently skips photos you can't delete (200 `[]`);
   `GET /api/sharing/folder/{id}` shows a grantee the other grantees; public-folder shares are
   visible to and editable by everyone (this matches the ownership model).
6. **New finding (test rewrite, not fixed):** `/api/download` sends a weak ETag, so `If-Range` with
   it never gives a 206 (the full file is sent again). `If-Range` with `Last-Modified` works. Not
   proposed for change.
2. Should root `CLAUDE.md` be committed when WP8 updates it?
3. Where does `webui` go at the end (merge into `folder`/`main`)? The owner's call.
4. Should `docs/web-ui-plan/progress.md` (this file) be committed on `webui`? Currently uncommitted.
