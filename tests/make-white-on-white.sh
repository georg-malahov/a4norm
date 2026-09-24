#!/bin/sh
# Build examples/white-on-white.jpg: a synthetic photo of a white sheet on a
# near-white desk, the case brightness cannot see and only the edges find.
#
#   tests/make-white-on-white.sh [a4norm]
#
# The sheet is a4norm's own clean page of examples/landing-invoice.webp (a
# synthetic invoice, no real data), laid in perspective on a background a few
# levels darker than the paper, with a soft shadow, uneven light, sensor
# noise and a phone's JPEG. Deterministic: +noise and the blur are seeded.
set -e
cd "$(dirname "$0")/.."
A4NORM="${1:-./a4norm}"
T=$(mktemp -d)
"$A4NORM" --format jpg --dpi 100 -o "$T/page.jpg" examples/landing-invoice.webp >/dev/null
# the page at 560x793 -> placed into a 1600x1200 frame, tilted, in perspective
magick -seed 7 "$T/page.jpg" -resize '560x793!' \
  -alpha set -virtual-pixel transparent -background none \
  -define distort:viewport=1600x1200+0+0 \
  -distort Perspective '0,0 520,170  560,0 1040,110  560,793 1110,960  0,793 480,1010' \
  +repage "$T/sheet.png"
magick -seed 7 -size 1600x1200 gradient:'#dddbd6'-'#e4e2de' \
  \( "$T/sheet.png" -alpha extract -blur 0x14 -level 0,60% \
     -background black -alpha shape -channel A -evaluate multiply 0.28 +channel \
     -geometry +10+14 \) -compose Over -composite \
  "$T/sheet.png" -compose Over -composite \
  \( -size 1600x1200 radial-gradient:white-'#cfcfcf' \) -compose Multiply -composite \
  -attenuate 0.35 +noise Gaussian \
  -sampling-factor 2x2 -quality 82 -strip examples/white-on-white.jpg
rm -rf "$T"
echo "examples/white-on-white.jpg"
