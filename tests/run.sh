#!/bin/sh
# Run every test there is, then build tests/out/report.html.
#
#   tests/run.sh           # the a4norm in this checkout
#   tests/run.sh --open    # ...and open the report in the browser
#
# 1. tests/regression.py -- the public examples in examples/, which CI runs
#    too: structural checks plus a golden render per example.
# 2. tests/corpus.py     -- your own photos in tests/corpus/ (git-ignored),
#    if there are any. See README, Testing, for how to add them.
# 3. tests/report.py     -- one page with every input beside its result.
#
# Exits non-zero when a check failed; the report is built either way, since
# that is exactly when it is needed.
cd "$(dirname "$0")/.."
OPEN=
[ "$1" = "--open" ] && OPEN=1
A4NORM="$PWD/a4norm"
STATUS=0

python3 tests/regression.py --a4norm "$A4NORM" --flavor light || STATUS=1
if [ -f tests/corpus/cases.json ]; then
  python3 tests/corpus.py --a4norm "$A4NORM" || STATUS=1
else
  echo "no local corpus (tests/corpus/cases.json) -- public examples only"
fi
python3 tests/report.py || STATUS=1
if [ -n "$OPEN" ]; then
  if command -v open >/dev/null 2>&1; then open tests/out/report.html
  elif command -v xdg-open >/dev/null 2>&1; then xdg-open tests/out/report.html
  fi
fi
exit $STATUS
