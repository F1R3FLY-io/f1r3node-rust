#!/usr/bin/env bash

verification_tmpdir_create() {
  local parent="$1"
  mkdir -p "$parent"
  mktemp -d "$parent/tmp.XXXXXX"
}

verification_tmpdir_cleanup() {
  local scratch="${1:-}"
  local parent="${2:-}"
  if [[ -z "$scratch" || -z "$parent" || "$scratch" != "$parent"/tmp.* ]]; then
    return 2
  fi
  rm -rf -- "$scratch"
}

verification_tmpdir_install() {
  mkdir -p "$1"
  VERIFY_TMP_PARENT="$(cd "$1" && pwd -P)"
  VERIFY_TMP="$(verification_tmpdir_create "$VERIFY_TMP_PARENT")"
  export TMPDIR="$VERIFY_TMP"
  trap 'verification_tmpdir_cleanup "$VERIFY_TMP" "$VERIFY_TMP_PARENT"' EXIT
}
