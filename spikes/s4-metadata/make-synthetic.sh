#!/usr/bin/env bash
# Synthetic test files with "full" panorama metadata (no private data): EXIF incl. GPS with
# made-up coordinates, IFD1 thumbnail, PTGui-style GPano, IPTC, Display P3 / Adobe RGB ICC.
# Usage: make-synthetic.sh <out-dir>
set -euo pipefail
d="$1"; mkdir -p "$d"
bin="$(dirname "$0")/target/release/s4-metadata"
"$bin" synth "$d"
sips -Z 160 -s format jpeg "$d/synthetic.jpg" --out "$d/thumb.jpg" >/dev/null
common=(-overwrite_original -Make=Synthetic -Model="Test Rig" -Software="PTGui Pro 12.27 (www.ptgui.com)"
  -DateTimeOriginal="2026:01:01 12:00:00" -FNumber=8 -ExposureTime=1/250 -ISO=100 -FocalLength=14
  -GPSLatitude=47.123456 -GPSLatitudeRef=N -GPSLongitude=8.654321 -GPSLongitudeRef=E -GPSAltitude=400 -GPSAltitudeRef=0
  -XMP-GPano:ProjectionType=equirectangular -XMP-GPano:UsePanoramaViewer=True
  -XMP-GPano:FullPanoWidthPixels=4096 -XMP-GPano:FullPanoHeightPixels=2048
  -XMP-GPano:CroppedAreaImageWidthPixels=4096 -XMP-GPano:CroppedAreaImageHeightPixels=2048
  -XMP-GPano:CroppedAreaLeftPixels=0 -XMP-GPano:CroppedAreaTopPixels=0
  -XMP-GPano:InitialViewHeadingDegrees=180 -XMP-GPano:PoseHeadingDegrees=42.0
  -XMP-GPano:StitchingSoftware="PTGui Pro 12.27 (www.ptgui.com)"
  -IPTC:Keywords=synthetic -IPTC:Keywords=panorama -XMP-dc:Subject=synthetic -Copyright="CC0 synthetic")
exiftool "${common[@]}" "-ICC_Profile<=/System/Library/ColorSync/Profiles/Display P3.icc" "-ThumbnailImage<=$d/thumb.jpg" "$d/synthetic.jpg" >/dev/null
exiftool "${common[@]}" "-ICC_Profile<=/System/Library/ColorSync/Profiles/AdobeRGB1998.icc" "$d/synthetic.tif" >/dev/null
rm "$d/thumb.jpg"
echo "written: $d/synthetic.jpg $d/synthetic.tif"
