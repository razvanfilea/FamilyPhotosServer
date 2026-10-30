#!/usr/bin/env bash
# Regenerates the committed E2E fixture photos in playwright/fixtures/photos/.
# Needs ImageMagick (`magick`), ffmpeg and node. The bulk and memories fixtures are generated at
# setup time by playwright/harness/serve.mjs instead, because they depend on the current date.
set -euo pipefail

cd "$(dirname "$0")"
root=photos
rm -rf "$root"

# path | EXIF DateTimeOriginal | size
photos=(
    "public/tie-a.jpg|2024:06:08 14:03:29|480x360"
    "public/tie-b.jpg|2024:06:08 14:03:29|480x360"
    "public/lake.jpg|2024:06:20 09:15:00|360x480"
    "public/garden.jpg|2024:05:02 10:00:00|480x360"
    "public/picnic.jpg|2024:05:11 12:30:00|480x360"
    "public/bikes.jpg|2024:05:25 17:45:00|360x480"
    "public/snow.jpg|2024:03:03 08:00:00|480x360"
    "public/kite.jpg|2024:03:17 15:20:00|480x360"
    "public/party.jpg|2023:12:24 20:00:00|480x360"
    "public/tree.jpg|2023:12:25 09:30:00|360x480"
    "public/duplicate-a.jpg|2023:08:14 11:00:00|480x360"
    "public/Family Trip/trip-a.jpg|2023:07:01 09:00:00|480x360"
    "public/Family Trip/trip-b.jpg|2023:07:01 13:00:00|480x360"
    "public/Family Trip/trip-c.jpg|2023:07:02 10:00:00|360x480"
    "public/Family Trip/trip-d.jpg|2023:07:03 18:00:00|480x360"
    "public/Family Trip/trip-e.jpg|2023:07:04 08:30:00|480x360"
    "alice/alice-a.jpg|2024:06:01 10:00:00|480x360"
    "alice/alice-b.jpg|2024:05:15 10:00:00|480x360"
    "alice/alice-c.jpg|2024:04:10 10:00:00|360x480"
    "alice/alice-d.jpg|2024:02:20 10:00:00|480x360"
    "alice/alice-e.jpg|2023:11:05 10:00:00|480x360"
    "alice/alice-f.jpg|2023:09:09 10:00:00|480x360"
    "alice/Holiday/holiday-a.jpg|2024:07:10 09:00:00|480x360"
    "alice/Holiday/holiday-b.jpg|2024:07:10 11:00:00|480x360"
    "alice/Holiday/holiday-c.jpg|2024:07:11 09:00:00|360x480"
    "alice/Holiday/holiday-d.jpg|2024:07:11 16:00:00|480x360"
    "alice/Holiday/holiday-e.jpg|2024:07:12 10:00:00|480x360"
    "alice/Holiday/holiday-f.jpg|2024:07:13 10:00:00|480x360"
    "alice/Holiday/holiday-g.jpg|2024:07:14 10:00:00|360x480"
    "alice/Holiday/holiday-h.jpg|2024:07:15 10:00:00|480x360"
    "alice/Private/private-a.jpg|2024:01:05 10:00:00|480x360"
    "alice/Private/private-b.jpg|2024:01:06 10:00:00|480x360"
    "alice/Private/private-c.jpg|2024:01:07 10:00:00|480x360"
    "bob/bob-a.jpg|2024:06:15 10:00:00|480x360"
    "bob/bob-b.jpg|2024:04:02 10:00:00|480x360"
    "bob/bob-c.jpg|2023:10:10 10:00:00|360x480"
    "carol/dup-a.jpg|2024:03:01 10:00:00|480x360"
)

seed=1
for entry in "${photos[@]}"; do
    IFS='|' read -r path date size <<< "$entry"
    mkdir -p "$root/$(dirname "$path")"
    magick -size "$size" -seed "$seed" plasma:fractal -blur 0x2 -strip -quality 70 "$root/$path"
    node ../harness/exif.mjs "$root/$path" "$date"
    seed=$((seed + 1))
done

# An exact copy, for the duplicates tool
cp "$root/public/duplicate-a.jpg" "$root/public/duplicate-b.jpg"

# carol's duplicates: the upload API refuses to create duplicates of a user's photos
mkdir -p "$root/carol/Copies"
cp "$root/carol/dup-a.jpg" "$root/carol/Copies/dup-b.jpg"

# Videos have no EXIF, the server reads their date from the file name
ffmpeg -loglevel error -f lavfi -i testsrc=duration=2:size=320x240:rate=15 \
    -pix_fmt yuv420p -c:v libx264 -preset veryslow -crf 35 -map_metadata -1 -y \
    "$root/bob/VID_20240610_120000.mp4"

du -sh "$root"
