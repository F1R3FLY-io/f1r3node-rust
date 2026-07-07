#!/usr/bin/env bash
set -euo pipefail

if [ -d /etc/apt/sources.list.d ]; then
  while IFS= read -r source; do
    if grep -qs 'packages.microsoft.com' "$source"; then
      sudo rm -f "$source"
    fi
  done < <(find /etc/apt/sources.list.d -maxdepth 1 -type f)
fi

sudo apt-get update
xargs -r -a .github/apt-dependencies.txt sudo apt-get install -y
