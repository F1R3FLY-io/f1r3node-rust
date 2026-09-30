#!/usr/bin/env bash

##### ELF UTILS #####
 
# Returns Nix store location of a program
function program-file()
{
    local PROGRAM=$1
    echo $(readlink -f $(command -v ${PROGRAM}))
}

# Returns list of shared libraries for the specific program or library file
function shared-libraries()
{
    local FILE=$1
    echo $(ldd ${FILE} 2>/dev/null | sed -n 's/^.*=> \(.*\) (.*)/\1/p')
}

# Returns linker used by the executable
function linker()
{
  local PROGRAM=$1
  # Try readelf first (reliable for all ELF binaries)
  # readelf output: [Requesting program interpreter: /lib64/ld-linux-x86-64.so.2]
  local interp=$(readelf -l "$PROGRAM" 2>/dev/null | sed -n 's/.*\[Requesting program interpreter: \(.*\)\]/\1/p')
  if [ -n "$interp" ]; then
    echo "$interp"
  else
    # Fallback: parse ldd output (may fail for some binaries like swipl)
    echo $(shared-libraries ${PROGRAM} | grep -o '[^ ]*ld-linux[^ ]*')
  fi
}

##### ELF PATCHING #####

# Patches a shared library to use /lib for RPATH
function patched-library() {
    local LIBRARY_PATH=$1
    local LIBRARY_HASH=$(sha256sum "${LIBRARY_PATH}" | cut -d' ' -f1)
    local LIBRARY_NAME=$(basename "${LIBRARY_PATH}")
    local CACHED_LIBRARY="${CACHE_DIR}/${LIBRARY_NAME}-${LIBRARY_HASH}"
    
    # Check if already processed in this session
    if [ ! -f "$CACHED_LIBRARY" ]; then
        mkdir -p "${CACHE_DIR}"
        cp "${LIBRARY_PATH}" "${CACHED_LIBRARY}"
        chmod +w "${CACHED_LIBRARY}"
        # Patch the RPATH/RUNPATH to /lib
        patchelf --set-rpath /lib "${CACHED_LIBRARY}" 2>/dev/null || true
        chmod -w "${CACHED_LIBRARY}"
    fi
    echo "$CACHED_LIBRARY"
}

# Patches an executable binary to use /lib for interpreter and RPATH
function patched-binary() {
    local PROGRAM=$1
    local CACHE_DIR="${HOME}/.cache/bwrap-patched"
    local PROGRAM_FILE=$(program-file "${PROGRAM}")
    local PROGRAM_HASH=$(sha256sum ${PROGRAM_FILE} | cut -d' ' -f1)
    local CACHED_BINARY="${CACHE_DIR}/${PROGRAM}-${PROGRAM_HASH}"
    local LINKER=$(linker ${PROGRAM_FILE})
    
    if [ ! -f "$CACHED_BINARY" ]; then
        mkdir -p "${CACHE_DIR}"
        cp "${PROGRAM_FILE}" "${CACHED_BINARY}"
        chmod +w "${CACHED_BINARY}"
        patchelf --set-interpreter /lib/$(basename ${LINKER}) "${CACHED_BINARY}"
        patchelf --set-rpath /lib "${CACHED_BINARY}"
        chmod -w "${CACHED_BINARY}"
    fi
    echo "$CACHED_BINARY"
}

##### RECURSIVE DEPENDENCY COLLECTION #####

# Recursively collects all library dependencies and their transitive
# dependencies
function collect-all-libraries() {
    local FILE=$1
    local LIBS=$(shared-libraries "${FILE}")

    # We store visited libraries in this map
    declare -A LIBS_MAP
    for LIB in $LIBS; do
        LIBS_MAP["${LIB}"]=1
        # Recursively process this library's dependencies
        if [ -f "$LIB" ]; then
            CHILDREN=$(collect-all-libraries "${LIB}")
            for CHILD in $CHILDREN; do
                LIBS_MAP["${CHILD}"]=1
            done
        fi
    done
    echo ${!LIBS_MAP[*]}
}

# Returns all unique libraries needed by a program
function all-libraries() {
    local PROGRAM=$1
    local PROGRAM_FILE=$(program-file "${PROGRAM}")
    
    # Recursively collect all libraries
    collect-all-libraries "${PROGRAM_FILE}"
}

##### BWRAP OPTIONS GENERATORS #####

# Generates --ro-bind option for the given executable file
function generate-executable-bind() {
    local PROGRAM=$1
    local PATCHED_PROGRAM=$(patched-binary ${PROGRAM})
    echo "--ro-bind ${PATCHED_PROGRAM} /usr/bin/${PROGRAM}"
}

# Generates --ro-bind options for all executables and their dependencies
function generate-binds-exes() {
    local BINDS=""
    
    # First, generate executable binds
    for PROGRAM in $*; do
        BINDS="${BINDS}$(generate-executable-bind ${PROGRAM}) "
    done
    
    # Then, generate library binds (with deduplication across programs)
    local ALL_LIBS_COLLECTED=()
    declare -A UNIQUE_LIBS
    
    for PROGRAM in $*; do
        for LIB in $(all-libraries ${PROGRAM}); do
            UNIQUE_LIBS[$LIB]=1
        done
    done

    # Generate binds for all unique libraries
    for LIB in "${!UNIQUE_LIBS[@]}"; do
        local LIB_BASENAME=$(basename "${LIB}")

        if [[ "$LIB_BASENAME" == ld-linux* ]]; then
            # Bind the original unpatched linker
            BINDS="${BINDS} --ro-bind ${LIB} /lib/${LIB_BASENAME}"
        else
            # Patch and bind other libraries
            local PATCHED_LIB=$(patched-library "${LIB}")
            BINDS="${BINDS} --ro-bind ${PATCHED_LIB} /lib/${LIB_BASENAME}"
        fi
    done

    # Add the dynamic linker for each program (ldd may not show it for all binaries)
    for PROGRAM in $*; do
        local LNK=$(linker $(program-file "${PROGRAM}") 2>/dev/null)
        if [ -n "$LNK" ] && [ -f "$LNK" ]; then
            local LNK_BASENAME=$(basename "$LNK")
            if [[ "$LNK_BASENAME" == ld-linux* ]]; then
                [[ " ${UNIQUE_LIBS[@]} " =~ " ${LNK} " ]] || BINDS="${BINDS} --ro-bind ${LNK} /lib/${LNK_BASENAME}"
            fi
        fi
    done
    
    echo ${BINDS}
}

# Generates --ro-bind options for all libraries and their dependencies
function generate-binds-libs() {
    local BINDS=""
    
    # Generate library binds (with deduplication across libraries)
    local ALL_LIBS_COLLECTED=()
    declare -A UNIQUE_LIBS
    
    for LIB in $*; do
        UNIQUE_LIBS[$LIB]=1
        for LIBDEP in $(collect-all-libraries ${LIB}); do
            UNIQUE_LIBS[$LIBDEP]=1
        done
    done

    # Generate binds for all unique libraries
    for LIB in "${!UNIQUE_LIBS[@]}"; do
        local LIB_BASENAME=$(basename "${LIB}")

        if [[ "$LIB_BASENAME" == ld-linux* ]]; then
            # Bind the original unpatched linker
            BINDS="${BINDS} --ro-bind ${LIB} /lib/${LIB_BASENAME}"
        else
            # Patch and bind other libraries
            local PATCHED_LIB=$(patched-library "${LIB}")
            BINDS="${BINDS} --ro-bind ${PATCHED_LIB} /lib/${LIB_BASENAME}"
        fi
    done
    
    echo ${BINDS}
}

##### BLOCKED PREDICATE SUPPORT #####

# Quote a predicate name as a Prolog single-quoted atom.
metta_quote_atom() {
    local NAME=$1
    local ESCAPED="${NAME//\'/\'\'}"
    echo "'${ESCAPED}'"
}

# Emit two Prolog goal fragments:
#   1. assertz((block_predicate(N) :- ...))
#   2. forall(member(P, [LIST]), block_predicate(P))
#
# Reads PETTA_BLOCKED_PREDS (default: "readln!") as a space-separated list.
petta_block_preds_goal() {
    local pred_list="${PETTA_BLOCKED_PREDS:-readln!}"
    local prolog_items=""
    local first_item=true

    if [[ -n "$pred_list" ]]; then
        for p in $pred_list; do
            local quoted
            quoted=$(metta_quote_atom "$p")
            if $first_item; then
                prolog_items="$quoted"
                first_item=false
            else
                prolog_items="$prolog_items, $quoted"
            fi
        done
    fi

    local prolog_list="[${prolog_items}]"

    # block_predicate clause (compacted to one line; -g does not handle newlines well)
    cat <<'PROLOG_GOAL' | tr -d '\n'
assertz((block_predicate(N) :-
    findall(A, current_predicate(N/A), As),
    forall(member(A, As), catch(abolish(N,A), _, true)),
    retractall(fun(N)),
    retractall(arity(N, _)),
    retractall(ho_specialization(N, _)),
    retractall(ho_specialization(_, N)),
    retractall('&self'('=', [N|_], _)),
    retractall('&self'(':', N, _)),
    findall(Ref, ( current_predicate('&self'/Ar),
                   functor(H, '&self', Ar), Ar >= 2,
                   arg(1, H, N), clause(H, _, Ref) ), Refs),
    forall(member(Ref, Refs), catch(erase(Ref), _, true)),
    catch(nb_delete(N), _, true))),
PROLOG_GOAL
    printf 'forall(member(P, %s), block_predicate(P))' "${prolog_list}"
}
