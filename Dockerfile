# a4norm — photos of paper documents -> one scanner-grade A4 PDF
#
# Alpine is the base because its ImageMagick delegates are separate packages, so
# HEIC support is a line in the install list rather than a hope about how a
# distro happened to build its binary. (PNG needs no delegate package — it is in
# the base imagemagick build.)
FROM alpine:3.21

RUN apk add --no-cache \
      python3 \
      imagemagick \
      imagemagick-heic \
      imagemagick-jpeg \
      imagemagick-tiff \
      imagemagick-webp \
      poppler-utils

# No ghostscript on purpose. Alpine's ImageMagick has no PDF coder without it,
# and a4norm writes its own PDF — a JPEG per page plus a few hundred bytes of
# boilerplate — so the image stays at ImageMagick + poppler. ImageMagick never
# READS a PDF here either; poppler does that.

COPY a4norm a4norm-serve /usr/local/bin/
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/a4norm /usr/local/bin/a4norm-serve \
             /usr/local/bin/entrypoint.sh

# Build-time smoke test: a synthetic page must survive the whole pipeline and
# come out as an A4 PDF, two of them must combine into one two-page PDF, HEIC
# must decode, and the HTTP service must answer. A missing delegate is then a
# failed build instead of a surprise on someone's first real document. Kept
# small because this also runs under emulation for the arm64 image.
RUN set -e; \
    magick -size 600x850 xc:white -fill '#111111' \
      -draw "rectangle 100,150 500,172" -draw "rectangle 100,260 430,282" \
      -draw "rectangle 100,370 470,392" \
      -background '#8b8b8b' -rotate 0.6 -bordercolor '#8b8b8b' -border 20 \
      /tmp/smoke.jpg; \
    a4norm -o /tmp/smoke.pdf /tmp/smoke.jpg; \
    pdfinfo /tmp/smoke.pdf | grep -q 'A4'; \
    a4norm -o /tmp/two.pdf /tmp/smoke.jpg /tmp/smoke.jpg; \
    test "$(pdfinfo /tmp/two.pdf | awk '/^Pages:/ {print $2}')" = "2"; \
    magick -list format | grep -qiE '^ +HEIC' || (echo "no HEIC delegate"; exit 1); \
    a4norm-serve --port 8099 & \
    SRV=$!; \
    sleep 3; \
    python3 -c "import urllib.request,json,sys; \
d=json.load(urllib.request.urlopen('http://127.0.0.1:8099/health')); \
sys.exit(0 if d.get('status')=='ok' else 1)"; \
    kill $SRV; \
    rm -f /tmp/smoke.jpg /tmp/smoke.pdf /tmp/two.pdf

WORKDIR /work
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD ["--help"]
