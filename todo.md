# FamilyPhotosServer - Improvement Ideas

## Medium-Effort Improvements

- [ ] Add rate limiting middleware - protect against brute force login and upload abuse
- [ ] Add user storage quotas - no per-user disk limits currently
- [ ] Add Prometheus metrics for observability
- [ ] Add photo search capability (names, folders, EXIF data)
- [ ] Make event log retention configurable (currently hardcoded to 512 rows)

## Larger Features

- [ ] Albums/Sharing - allow sharing with specific users (currently only private vs public)
- [ ] Tags - add tagging system for better organization (currently only flat folders)
- [ ] Bulk export/backup API
- [ ] Video transcoding support
- [ ] Timeline view API - auto-group photos by date/location from EXIF
- [ ] Photo rotation/basic editing

## Technical Debt

- [ ] Extract business logic from repository to service layer (`src/repo/photos_repo.rs:103,128`)
- [ ] Add test coverage (unit and integration tests)
- [ ] Improve CLI error handling - uses `stdin.read_line()` which can panic
- [ ] Replace remaining `unwrap()` calls with proper error handling