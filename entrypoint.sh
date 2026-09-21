#!/bin/sh
# `docker run … serve [flags]` starts the HTTP service; anything else is the CLI.
if [ "$1" = "serve" ]; then
  shift
  exec a4norm-serve "$@"
fi
exec a4norm "$@"
