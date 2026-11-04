# WP10 — iOS

**After WP9.** May be delayed: the owner has no real iPhone, and touch, scrolling performance and memory can't
be judged without one (simulators and desktop Safari say little about them). Start only when a real iPhone is
available for testing (Safari remote Web Inspector from a Mac, or a deployed test instance).

There is no iOS app, so iPhone users use the web app installed on the home screen. WP9 already gives it the
native feel on other platforms; WP10 adds what only iOS needs.

## Tasks
- `apple-mobile-web-app-capable`, `apple-mobile-web-app-status-bar-style` (`black-translucent`, content
  under the status bar with safe-area padding), `apple-mobile-web-app-title`.
- An "Install this app" hint on iOS Safari (not standalone): a dismissible card explaining Share →
  Add to Home Screen, shown once, remembered in `fp_prefs` or localStorage.
- 120 Hz ProMotion profiling, iOS memory limits, HEVC playback, `-apple-system` font, iOS edge-swipe
  conflicts, Web Share file limits on iOS.
- WebKit + Mobile Safari e2e runs (`check.sh --webkit` in the Playwright Docker image; the host lacks
  WebKit's libraries) and a test on a real iPhone (Safari remote Web Inspector or a deployed instance).
- Document the platform limits below for iPhone users.
- iOS-specific parts of WP9's 9.4 (edge-swipe back where it doesn't fight Safari's gesture) and 9.6 (Web Share
  of several photos/videos within iOS limits, the status bar colour).

## Known platform limits (document them for users; don't try to work around them)

- **No automatic camera-roll backup.** A web app can't read the photo library in the background, so
  uploads are always "pick photos, keep the app open". This is the main gap versus the Android app.
- No background upload: leaving the app pauses uploads (the queue must resume cleanly on return).
- No Vibration API (no haptics), no Web Share *Target* (can't appear in the iOS share sheet as a
  destination), no Background Sync.
- iOS may evict web storage; installed apps are mostly exempt. Treat every cache as a speed-up only.

## Rules
Same as WP9: no `/api` change, no migration without the owner's OK, CSP stays strict, desktop and Android
behaviour unchanged. Keep `check.sh --firefox` green and add `--webkit`.

## Acceptance criteria
- Installed on an iPhone: opens full-screen with no white flash, status bar and safe areas correct in portrait
  and landscape, back and pull-to-refresh available in-app.
- Viewer gestures and fling scrolling through the 200 000-photo seed without jank on a ProMotion iPhone, no tab
  crash from memory.
- The full suite green on WebKit and Mobile Safari as well.
