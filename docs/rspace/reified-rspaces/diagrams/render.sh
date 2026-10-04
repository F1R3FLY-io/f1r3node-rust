#!/usr/bin/env bash
# ============================================================================
# Reified RSpaces: diagram renderer and drift check
# ----------------------------------------------------------------------------
# Renders every PlantUML source (*.puml) of one directory to a paired .svg.
# Each source includes the shared house palette and the campaign aliases:
#     !include ../../../casper/theory/diagrams/palette.iuml
#     !include concept-colors.iuml
# The script never renders the *.iuml include files.
#
# Usage:
#   ./render.sh                         render every *.puml of this directory
#   ./render.sh 02-namespace-bytes.puml render only the named sources
#   ./render.sh --check                 render into scratch and compare with
#                                       the committed SVGs (no source change)
#   ./render.sh --dir DIR [--check] ... operate on DIR, not on this directory
#
# Render mode writes into scratch first. It replaces a committed SVG only
# after a clean render, so a failed render never overwrites a good SVG.
#
# Check mode fails when any of these conditions occurs:
#   - PlantUML exits with a non-zero status (a syntax or include error);
#   - PlantUML writes to stdout or stderr (a warning or a LaTeX error, which
#     PlantUML reports with exit status 0);
#   - the fresh SVG shows a PlantUML error banner;
#   - a committed SVG is missing or differs from the fresh SVG by one byte;
#   - an SVG of the directory has no .puml source (full-directory runs only).
# Each failure prints one line "FAIL <file>: <reason>". Each pass prints one
# line "ok <file>". scripts/verify-reified/doc_lint.py parses these lines.
#
# Scratch root: ${RRS_SCRATCH:-<repo>/target/verification/reified-rspaces}.
# The script refuses a scratch root under /tmp, because /tmp is RAM-backed on
# the development hosts. The verifier sandbox sets RRS_SANDBOX=1, because its
# /tmp is a private tmpfs of the job. Java writes its temporary font files to
# the scratch directory and writes no performance data:
#     -Djava.io.tmpdir=<scratch> -XX:-UsePerfData
#
# Toolchain: PlantUML 1.2026.8 with its bundled JLaTeXMath for LaTeX labels,
# run as "java -jar plantuml.jar". Set PLANTUML_JAR or JAVA to override the
# lookup. Each SVG records the PlantUML version, so a different version fails
# the check. The failure message then names both versions.
#
# Hermetic fonts: text metrics set the size and the layout of each SVG, and
# Java builds its fallback font chain from every font that fontconfig lists.
# So the script gives Java a fixed font set:
#   - fonts.lock pins each font file by its SHA-256 digest. The script stops
#     before it renders when a digest differs.
#   - A generated fontconfig file lists only a scratch directory of links to
#     the pinned fonts. Fontconfig applies only this file. It still scans
#     the system directory conf.avail for file descriptions, but it applies
#     none of those files.
#   - Java and its Graphviz child processes get a scratch home and scratch
#     XDG directories, so they read no font cache or file of the user, and
#     a fixed locale (C.UTF-8, en_US).
# The same sources then give the same SVG bytes on a host and in the
# verifier sandbox.
#
# Exit status: 0 success, 1 render or check failure, 2 usage or tool error.
# ============================================================================
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"

MODE=render
DIR="$SCRIPT_DIR"
declare -a TARGETS=()

usage() {
  sed -n '/^# Usage:/,/^# Render mode/p' "${BASH_SOURCE[0]}" | sed -e 's/^# \{0,1\}//' -e '$d'
}

while (($# > 0)); do
  case "$1" in
    --check) MODE=check ;;
    --dir)
      if (($# < 2)); then
        echo "render.sh: --dir needs a directory" >&2
        exit 2
      fi
      DIR="$2"
      shift ;;
    --dir=*) DIR="${1#--dir=}" ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "render.sh: unknown option: $1" >&2; usage >&2; exit 2 ;;
    *) TARGETS+=("$1") ;;
  esac
  shift
done

if [[ ! -d "$DIR" ]]; then
  echo "render.sh: not a directory: $DIR" >&2
  exit 2
fi
DIR="$(cd "$DIR" && pwd)"

# ---- sources ----------------------------------------------------------------
declare -a SOURCES=()
if ((${#TARGETS[@]} > 0)); then
  for t in "${TARGETS[@]}"; do
    case "$t" in
      *.puml) ;;
      *) echo "render.sh: not a .puml source: $t" >&2; exit 2 ;;
    esac
    if [[ -f "$DIR/$(basename "$t")" ]]; then
      SOURCES+=("$DIR/$(basename "$t")")
    else
      echo "render.sh: source not found in $DIR: $t" >&2
      exit 2
    fi
  done
else
  shopt -s nullglob
  for f in "$DIR"/*.puml; do SOURCES+=("$f"); done
  shopt -u nullglob
fi

# ---- toolchain --------------------------------------------------------------
JAVA_BIN="${JAVA:-java}"
if ! command -v "$JAVA_BIN" >/dev/null 2>&1; then
  echo "render.sh: FAIL: the Java runtime '$JAVA_BIN' is not installed" >&2
  exit 2
fi

find_plantuml_jar() {
  local wrapper jar
  if [[ -n "${PLANTUML_JAR:-}" ]]; then
    printf '%s\n' "$PLANTUML_JAR"
    return 0
  fi
  wrapper="$(command -v plantuml 2>/dev/null || true)"
  if [[ -n "$wrapper" && -f "$wrapper" ]] && grep -Iq . "$wrapper" 2>/dev/null; then
    jar="$(grep -oE "[^[:space:]\"']+\\.jar" "$wrapper" | head -n 1)"
    if [[ -n "$jar" && -f "$jar" ]]; then
      printf '%s\n' "$jar"
      return 0
    fi
  fi
  for jar in /usr/share/java/plantuml/plantuml.jar /usr/share/plantuml/plantuml.jar \
             /opt/plantuml/plantuml.jar; do
    if [[ -f "$jar" ]]; then
      printf '%s\n' "$jar"
      return 0
    fi
  done
  return 1
}

if ! JAR="$(find_plantuml_jar)" || [[ ! -f "$JAR" ]]; then
  echo "render.sh: FAIL: plantuml.jar not found (set PLANTUML_JAR)" >&2
  exit 2
fi

# ---- scratch ----------------------------------------------------------------
SCRATCH_ROOT="${RRS_SCRATCH:-$REPO_ROOT/target/verification/reified-rspaces}"
case "$SCRATCH_ROOT" in
  /tmp|/tmp/*)
    if [[ "${RRS_SANDBOX:-0}" != 1 ]]; then
      echo "render.sh: refusing the scratch root $SCRATCH_ROOT under /tmp; set RRS_SCRATCH" >&2
      exit 2
    fi ;;
esac
if ! mkdir -p "$SCRATCH_ROOT/diagram-render"; then
  echo "render.sh: cannot create $SCRATCH_ROOT/diagram-render" >&2
  exit 2
fi
if ! WORK="$(mktemp -d "$SCRATCH_ROOT/diagram-render/$MODE.XXXXXX")"; then
  echo "render.sh: cannot create a scratch directory under $SCRATCH_ROOT/diagram-render" >&2
  exit 2
fi
JTMP="$WORK/java-tmp"
OUT="$WORK/out"
FONT_ENV="$WORK/font-env"
mkdir -p "$JTMP" "$OUT" "$FONT_ENV/fonts" "$FONT_ENV/fontconfig-cache" "$FONT_ENV/home"

# On every exit path, remove the font environment and the scratch
# directories that are empty. A failed check keeps its fresh SVGs, so the
# directory stays for inspection.
cleanup_scratch() {
  rm -rf -- "$FONT_ENV"
  rmdir -- "$JTMP" 2>/dev/null
  rmdir -- "$OUT" 2>/dev/null
  rmdir -- "$WORK" 2>/dev/null
  return 0
}
trap cleanup_scratch EXIT

# ---- hermetic fonts ---------------------------------------------------------
FONT_LOCK="$SCRIPT_DIR/fonts.lock"
if [[ ! -f "$FONT_LOCK" ]]; then
  echo "render.sh: FAIL: the font pin file $FONT_LOCK is missing" >&2
  exit 2
fi
FONT_COUNT=0
while read -r digest path; do
  if [[ -z "$digest" || "$digest" == \#* ]]; then
    continue
  fi
  if [[ ! -f "$path" ]]; then
    echo "render.sh: FAIL: the pinned font $path is not installed" >&2
    exit 2
  fi
  actual="$(sha256sum -- "$path" | sed -E 's/[[:space:]].*//')"
  if [[ "$actual" != "$digest" ]]; then
    echo "render.sh: FAIL: the font $path changed (pinned $digest, found $actual); update fonts.lock and render again" >&2
    exit 2
  fi
  ln -s -- "$path" "$FONT_ENV/fonts/$(basename "$path")"
  FONT_COUNT=$((FONT_COUNT + 1))
done <"$FONT_LOCK"
if ((FONT_COUNT == 0)); then
  echo "render.sh: FAIL: $FONT_LOCK pins no font" >&2
  exit 2
fi
cat >"$FONT_ENV/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>$FONT_ENV/fonts</dir>
  <cachedir>$FONT_ENV/fontconfig-cache</cachedir>
  <alias binding="same"><family>sans-serif</family><prefer><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>sans</family><prefer><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>serif</family><prefer><family>Noto Sans</family></prefer></alias>
  <alias binding="same"><family>monospace</family><prefer><family>Noto Sans Mono</family></prefer></alias>
</fontconfig>
EOF

run_plantuml() {
  # $1 = log path prefix. The other arguments go to PlantUML. Output goes to $1.out and $1.err.
  local log="$1"
  shift
  FONTCONFIG_FILE="$FONT_ENV/fonts.conf" LC_ALL=C.UTF-8 HOME="$FONT_ENV/home" \
    XDG_CONFIG_HOME="$FONT_ENV/home/.config" XDG_DATA_HOME="$FONT_ENV/home/.local/share" \
    XDG_CACHE_HOME="$FONT_ENV/home/.cache" \
    "$JAVA_BIN" -Djava.awt.headless=true -Djava.io.tmpdir="$JTMP" -XX:-UsePerfData \
    -Duser.home="$FONT_ENV/home" -Duser.language=en -Duser.country=US -Dfile.encoding=UTF-8 \
    -jar "$JAR" "$@" >"$log.out" 2>"$log.err"
}

run_plantuml "$WORK/version" -version
LOCAL_VERSION="$(sed -nE 's/^PlantUML version ([^ ]+).*/\1/p' "$WORK/version.out" | head -n 1)"
rm -f -- "$WORK/version.out" "$WORK/version.err"
if [[ -z "$LOCAL_VERSION" ]]; then
  echo "render.sh: FAIL: '$JAR' did not report a PlantUML version" >&2
  exit 2
fi

svg_version() {
  grep -o '<?plantuml [^?]*?>' "$1" 2>/dev/null | head -n 1 | sed -E 's/^<\?plantuml ([^?]*)\?>$/\1/'
}

FAILURES=0
PASSES=0

fail() {
  # $1 = file name; $2... = reasons. Prints one line per reason, counts one file.
  local name="$1" reason
  shift
  for reason in "$@"; do
    printf 'FAIL %s: %s\n' "$name" "$reason"
  done
  FAILURES=$((FAILURES + 1))
}

# render_fresh <source> sets FRESH_SVG and appends reasons to REASONS.
render_fresh() {
  local src="$1" stem log status
  stem="$(basename "${src%.puml}")"
  log="$WORK/$stem"
  REASONS=()
  FRESH_SVG="$OUT/$stem.svg"
  run_plantuml "$log" -charset UTF-8 -tsvg -o "$OUT" "$src"
  status=$?
  if ((status != 0)); then
    REASONS+=("PlantUML exit status $status: $(tr '\n' ' ' <"$log.err" | head -c 400)")
  elif [[ -s "$log.err" ]]; then
    REASONS+=("PlantUML wrote to stderr: $(tr '\n' ' ' <"$log.err" | head -c 400)")
  fi
  if [[ -s "$log.out" ]]; then
    REASONS+=("PlantUML wrote to stdout: $(tr '\n' ' ' <"$log.out" | head -c 400)")
  fi
  if [[ ! -s "$FRESH_SVG" ]]; then
    REASONS+=("PlantUML produced no SVG")
  elif grep -q -E 'Syntax Error\?|contains errors|[Cc]annot include' "$FRESH_SVG"; then
    REASONS+=("the fresh SVG shows a PlantUML error banner")
  fi
  rm -f -- "$log.out" "$log.err"
}

if [[ "$MODE" == render ]]; then
  for src in "${SOURCES[@]}"; do
    name="$(basename "$src")"
    render_fresh "$src"
    if ((${#REASONS[@]} > 0)); then
      fail "$name" "${REASONS[@]}"
      rm -f -- "$FRESH_SVG"
      continue
    fi
    if cp -- "$FRESH_SVG" "$DIR/${name%.puml}.svg"; then
      printf 'ok %s -> %s\n' "$name" "${name%.puml}.svg"
      PASSES=$((PASSES + 1))
    else
      fail "$name" "cannot write ${name%.puml}.svg"
    fi
    rm -f -- "$FRESH_SVG"
  done
else
  for src in "${SOURCES[@]}"; do
    name="$(basename "$src")"
    stem="${name%.puml}"
    committed="$DIR/$stem.svg"
    render_fresh "$src"
    if [[ ! -f "$committed" ]]; then
      REASONS+=("the committed SVG $stem.svg is missing")
    elif [[ -s "$FRESH_SVG" ]] && ! cmp -s -- "$FRESH_SVG" "$committed"; then
      first="$(cmp -- "$FRESH_SVG" "$committed" 2>/dev/null | sed -nE 's/.*differ: (byte|char) ([0-9]+, line [0-9]+).*/byte \2/p')"
      committed_version="$(svg_version "$committed")"
      if [[ -n "$committed_version" && "$committed_version" != "$LOCAL_VERSION" ]]; then
        REASONS+=("$stem.svg was rendered by PlantUML $committed_version, but the local PlantUML is $LOCAL_VERSION")
      fi
      REASONS+=("$stem.svg differs from the fresh render (first difference at ${first:-end of file}); run ./render.sh $name")
    fi
    if ((${#REASONS[@]} > 0)); then
      fail "$name" "${REASONS[@]}"
    else
      printf 'ok %s\n' "$name"
      PASSES=$((PASSES + 1))
      rm -f -- "$FRESH_SVG"
    fi
  done
  if ((${#TARGETS[@]} == 0)); then
    shopt -s nullglob
    for svg in "$DIR"/*.svg; do
      if [[ ! -f "${svg%.svg}.puml" ]]; then
        fail "$(basename "$svg")" "the SVG has no .puml source in this directory"
      fi
    done
    shopt -u nullglob
  fi
fi

# ---- cleanup: remove only empty scratch directories -------------------------
cleanup_scratch
if [[ -d "$WORK" ]]; then
  KEPT=" (scratch kept at $WORK)"
else
  KEPT=""
fi

printf 'render.sh %s: %d ok, %d failed, PlantUML %s, directory %s%s\n' \
  "$MODE" "$PASSES" "$FAILURES" "$LOCAL_VERSION" "$DIR" "$KEPT"
exit $((FAILURES > 0 ? 1 : 0))
