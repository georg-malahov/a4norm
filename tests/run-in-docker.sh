#!/bin/sh
# Run the regression test inside an a4norm image, against the a4norm baked
# into that image -- the same thing CI does before anything is pushed.
#
#   tests/run-in-docker.sh                                  # builds ./Dockerfile
#   tests/run-in-docker.sh ghcr.io/georg-malahov/a4norm:full
#   tests/run-in-docker.sh a4norm:test --update-goldens     # refresh goldens
#
#   tests/run-in-docker.sh a4norm:test-full                 # full image; picks
#                                                           # tests/golden/full
#
# Goldens are per image flavour (light / full): the two ImageMagick builds
# differ by a sub-pixel shift on the demo page. Refresh BOTH when an
# algorithm change is meant to change the pages, and look at them first.
set -e
cd "$(dirname "$0")/.."
IMAGE="${1:-}"
case "$IMAGE" in -*|"") IMAGE=a4norm:test; docker build -q -t "$IMAGE" . >/dev/null ;; *) shift ;; esac
exec docker run --rm -v "$PWD:/repo" -w /repo --user "$(id -u):$(id -g)" \
  --entrypoint python3 "$IMAGE" tests/regression.py "$@"
