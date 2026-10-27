#!/usr/bin/env bash
# Metadata diff between an original and a re-written file (exiftool, all groups, numeric values).
# Usage: s4-diff.sh <original> <rewritten>
set -euo pipefail
# BigTIFF display quirks of exiftool: sub-IFDs are named after the pointer tag, and tag 700 is
# listed raw (ApplicationNotes) next to the parsed XMP. Normalised so classic and BigTIFF compare.
dump() { exiftool -a -G1 -s -n -u "$1" | grep -v -E '^\[(File|System|Composite|ExifTool)\]' \
  | grep -v -E '^\[IFD0\] +ApplicationNotes' | sed -E 's/^\[ExifOffset\]  /[ExifIFD]     /; s/^\[GPSInfo\]   /[GPS]       /' | sort; }
out=$(diff <(dump "$1") <(dump "$2") | grep -E '^[<>]' || true)
[ -n "$out" ] && echo "$out" || echo "(metadata identical)"
cmpbin() { # tag
  if cmp -s <(exiftool -b -"$1" "$a" 2>/dev/null) <(exiftool -b -"$1" "$b" 2>/dev/null); then echo "$1: byte-identical"; else echo "$1: DIFFERENT"; fi
}
a="$1"; b="$2"
for t in ICC_Profile XMP ThumbnailImage; do
  [ "$(exiftool -b -$t "$a" 2>/dev/null | wc -c)" -gt 0 ] && cmpbin $t
done
exiftool -s -s -s -FileType -ImageSize -BitsPerSample -Compression -YCbCrSubSampling "$b" | tr '\n' ' '; echo
