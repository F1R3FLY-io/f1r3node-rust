#!/usr/bin/env bash
# Prints the resolved dependency closure of the casper-soak harness from
# Cargo.lock, one "name version source checksum" line for each package. The
# roots are the non-optional [dependencies] of scripts/casper-soak/Cargo.toml.
# With --digest it prints the sha256 of that list and of the workspace manifest
# parts that reach the harness build. With --manifest it prints those parts.
# The cleanup gate binds this digest instead of the workspace Cargo.lock and
# Cargo.toml, so an unrelated dependency change does not invalidate the cleanup
# acceptance. The lockfile walk follows every recorded edge, including optional
# edges that the harness features do not activate. It can therefore include a
# package that the harness does not build, which only fails closed.
set -euo pipefail
root=.
mode=list
while [ "$#" -gt 0 ]; do
    case "$1" in
        --root) root="$2"; shift 2 ;;
        --digest) mode=digest; shift ;;
        --manifest) mode=manifest; shift ;;
        *) printf 'Unknown lock closure option: %s\n' "$1" >&2; exit 2 ;;
    esac
done
manifest="$root/scripts/casper-soak/Cargo.toml"
lock="$root/Cargo.lock"
[ -f "$manifest" ] || { printf 'The harness manifest is missing.\n' >&2; exit 2; }
[ -f "$lock" ] || { printf 'Cargo.lock is missing.\n' >&2; exit 2; }
roots="$(awk '
    /^\[/ { section = ($0 == "[dependencies]"); next }
    section && /^[A-Za-z0-9_-]+[[:space:]]*=/ && $0 !~ /optional[[:space:]]*=[[:space:]]*true/ {
        sub(/[[:space:]]*=.*/, ""); printf "%s ", $0
    }' "$manifest")"
[ -n "$roots" ] || { printf 'The harness manifest declares no dependencies.\n' >&2; exit 2; }
closure="$(awk -v roots="$roots" '
    function flush() {
        if (name != "") {
            key = name " " ver
            info[key] = name " " ver " " src " " sum
            deps[key] = list
            count[name]++
            only[name] = key
        }
        name = ""; ver = ""; src = "-"; sum = "-"; list = ""; inlist = 0
    }
    function resolve(ref,    parts, n) {
        sub(/ \(.*\)$/, "", ref)
        n = split(ref, parts, " ")
        if (n == 2) return (parts[1] " " parts[2]) in info ? parts[1] " " parts[2] : ""
        return count[ref] == 1 ? only[ref] : ""
    }
    /^\[\[package\]\]$/ { flush(); next }
    inlist && /^\]$/ { inlist = 0; next }
    inlist { ref = $0; gsub(/^[[:space:]]*"|",?[[:space:]]*$/, "", ref); list = list ref "\n"; next }
    /^name = "/ { name = $3; gsub(/"/, "", name); next }
    /^version = "/ { ver = $3; gsub(/"/, "", ver); next }
    /^source = "/ { src = $3; gsub(/"/, "", src); next }
    /^checksum = "/ { sum = $3; gsub(/"/, "", sum); next }
    /^dependencies = \[$/ { inlist = 1; next }
    END {
        flush()
        harness = resolve("casper-soak")
        if (harness == "") { print "casper-soak is not in Cargo.lock" > "/dev/stderr"; exit 2 }
        n = split(roots, wanted, " ")
        for (i = 1; i <= n; i++) if (wanted[i] != "") want[wanted[i]] = 1
        m = split(deps[harness], refs, "\n")
        for (i = 1; i <= m; i++) {
            if (refs[i] == "") continue
            ref = refs[i]; sub(/ .*/, "", ref)
            if (!(ref in want)) continue
            key = resolve(refs[i])
            if (key == "") { print "unresolved root: " refs[i] > "/dev/stderr"; exit 2 }
            queue[++tail] = key; seen[key] = 1; found[ref] = 1
        }
        for (r in want) if (!(r in found)) { print "root missing from Cargo.lock: " r > "/dev/stderr"; exit 2 }
        while (head < tail) {
            key = queue[++head]
            print info[key]
            m = split(deps[key], refs, "\n")
            for (i = 1; i <= m; i++) {
                if (refs[i] == "") continue
                next_key = resolve(refs[i])
                if (next_key == "") { print "unresolved dependency: " refs[i] > "/dev/stderr"; exit 2 }
                if (!(next_key in seen)) { seen[next_key] = 1; queue[++tail] = next_key }
            }
        }
    }' "$lock" | LC_ALL=C sort)"
[ -n "$closure" ] || { printf 'The harness dependency closure is empty.\n' >&2; exit 2; }
# The workspace manifest also changes the harness build: the inherited
# [workspace.dependencies] entries of the workspace = true roots set their
# features, and [patch.*] and [profile.*] apply to every crate. Bind these
# parts verbatim. Members and the entries of other crates stay unbound.
workspace_manifest="$root/Cargo.toml"
[ -f "$workspace_manifest" ] || { printf 'The workspace manifest is missing.\n' >&2; exit 2; }
inherited="$(awk '
    /^\[/ { section = ($0 == "[dependencies]"); next }
    section && /^[A-Za-z0-9_-]+[[:space:]]*=/ && /workspace[[:space:]]*=[[:space:]]*true/ && $0 !~ /optional[[:space:]]*=[[:space:]]*true/ {
        sub(/[[:space:]]*=.*/, ""); printf "%s ", $0
    }' "$manifest")"
workspace_parts="$(awk -v names="$inherited" '
    function depth(text,    opened, closed) {
        opened = gsub(/[[{]/, "&", text); closed = gsub(/[]}]/, "&", text)
        return opened - closed
    }
    BEGIN { n = split(names, list, " "); for (i = 1; i <= n; i++) if (list[i] != "") want[list[i]] = 1 }
    /^\[/ {
        open = 0
        header = $0
        keep = (header ~ /^\[(profile|patch)([.]|\])/)
        member = header; sub(/^\[workspace\.dependencies\./, "", member); sub(/\]$/, "", member)
        if (header ~ /^\[workspace\.dependencies\./ && (member in want)) keep = 1
        deps = (header == "[workspace.dependencies]")
        if (keep) print
        next
    }
    keep { print; next }
    open > 0 { print; open += depth($0); next }
    deps && /^[A-Za-z0-9_-]+[[:space:]]*=/ {
        name = $0; sub(/[[:space:]]*=.*/, "", name)
        if (name in want) { print; open = depth($0) }
    }' "$workspace_manifest")"
for name in $inherited; do
    printf '%s\n' "$workspace_parts" | grep -Eq "^(${name}[[:space:]]*=|\[workspace\.dependencies\.${name}\])" ||
        { printf 'The workspace manifest does not declare the inherited dependency %s.\n' "$name" >&2; exit 2; }
done
if [ "$mode" = digest ]; then
    printf '%s\n# workspace manifest\n%s\n' "$closure" "$workspace_parts" | shasum -a 256 | awk '{print $1}'
elif [ "$mode" = manifest ]; then
    printf '%s\n' "$workspace_parts"
else
    printf '%s\n' "$closure"
fi
