#!/usr/bin/env bash
# S4 round-trip suite on synthetic files (publishable output). Usage: run-synthetic.sh <work-dir>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"; d="$1"
bin="$here/target/release/s4-metadata"; diff="$here/s4-diff.sh"
"$here/make-synthetic.sh" "$d" >/dev/null
j="$d/synthetic.jpg"; t="$d/synthetic.tif"
run() { # name, input, output, args…
  local name="$1" in="$2" out="$3"; shift 3
  echo "### $name"; echo '```'
  "$bin" rt "$in" "$out" "$@" | sed 's/^/# /'
  "$diff" "$in" "$out"
  echo '```'; echo
}
run "1 JPEG → JPEG (XMP via xmp_toolkit)"            "$j" "$d/1.jpg"
run "2 JPEG → JPEG (XMP text patch, heading 90°)"    "$j" "$d/2.jpg" --xmp text --heading 90
run "3 JPEG → JPEG half size (dimensions)"           "$j" "$d/3.jpg" --xmp text --half
run "4 JPEG → JPEG, EXIF via little_exif"            "$j" "$d/4.jpg" --xmp text --exif little
run "5 TIFF 16-bit → TIFF"                           "$t" "$d/5.tif" --xmp text
run "6 TIFF → BigTIFF (heading 12.5°)"               "$t" "$d/6.tif" --xmp text --bigtiff --heading 12.5
run "7 BigTIFF → classic TIFF"                       "$d/6.tif" "$d/7.tif" --xmp text --classic
echo "### 6+7 original TIFF vs. BigTIFF → classic (exiftool can't read GPS from BigTIFF)"; echo '```'
"$diff" "$t" "$d/7.tif"; echo '```'; echo
run "8 TIFF Adobe RGB 16-bit → sRGB JPEG half size (Street View preset)" "$t" "$d/8.jpg" --xmp text --half --srgb mox
echo "### Pixels"; echo '```'
for f in 5 7; do echo -n "$f.tif: "; "$bin" pixeq "$t" "$d/$f.tif"; done
echo '```'
