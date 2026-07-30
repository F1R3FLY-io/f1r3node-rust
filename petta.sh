#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

### BEFORE RUNNING ###
# 
# Make sure the following programs are available in the PATH:
# * swipl
# * python3
# Make sure the following variables are defined:
# * PETTA_DIR: path to PeTTa (the top folder in the repository)
# * SANDBOX_LIB_PATH: path to the libsandbox.so library.
# * CACHE_DIR: this location is where patched libraries will be placed in the
#   host system before binding them inside the bubblewrap sandbox.
#
# Optionally, the following variable may be defined:
# * PLN_DIR: path to the PLN library source (tested with dylon/PLN,
#   feature/mettatron branch). When set, the PLN library is bound read-only
#   inside the sandbox and registered as a MeTTaTron library_path, so that
#   `!(import! &self (library PLN lib_pln))` resolves without `git-import!`
#   (which this script disables unconditionally; see below). When PLN_DIR is
#   unset or empty, the sandbox is unchanged and the PLN library is unavailable.
#
if [[ ! -v PETTA_DIR ]]; then
  echo "The PETTA_DIR variable is not defined. Please set it to the PeTTa \
directory."
  exit 1
fi

if [[ ! -v CACHE_DIR ]]; then
  echo "The CACHE_DIR variable is not defined. Please set it to a directory."
  exit 1
fi

if [[ ! SANDBOX_LIB_PATH ]]; then
  echo "The SANDBOX_LIB_PATH variable is not defined. Please set it to the path\
 of the libsandbox.so library. See https://github.com/cloudflare/sandbox."
  exit 1
fi

# Operating mode: NORMAL (default) or NODE
# NORMAL  — print MeTTa println!/trace! output directly to stdout, emit a single {results:[...]} JSON envelope
# NODE    — emit NDJSON frames: {"channel":"...","arguments":[...]} for each println!/trace!,
#           then a final {"type":"result","value":[...]} line
PETTA_MODE=${PETTA_MODE:-NORMAL}

source "${SCRIPT_DIR}/functions.sh"

### BUBBLEWRAP OPTIONS ###
# Here we calculate the --ro-bind, --dir and --symlink options required to
# create a Prolog compatible filesystem inside the bublewrap sandbox.
#
# Specifically, we need:
# * The Prolog executable and all supporting libraries
# * The PeTTa project directory, which contains the PeTTa interpreter itself as
#   well as other MeTTa libraries provided in it.
# * The libsandbox.so library, which lets us conveniently apply seccomp filters.

# Programs to run in the sandbox. python3 is intentionally excluded: the MeTTa
# libraries have been de-pythonified (random/time/math now come from PeTTa
# builtins backed by lib/pyrand.pl), so no py-call is ever invoked and the
# Python interpreter is not needed inside the sandbox.
PROGRAMS="swipl bash ls"

if [ ! -f ${CACHE_DIR}/cached_programs_binds ]; then
    PROGRAMS_BINDS=$(generate-binds-exes ${PROGRAMS})
else
    PROGRAMS_BINDS=$(<${CACHE_DIR}/cached_programs_binds)
    echo "${PROGRAMS_BINDS}" >${CACHE_DIR}/cached_programs_binds
fi

# Options for re-creating the swipl home inside the  sandbox. We can't just
# bind the whole directory because we need to create the symlink
# `/lib/swipl/lib/x86_64-linux -> /lib`.
SWIPL_HOME_DIR=$(swipl --home)
if [ ! -f ${CACHE_DIR}/cached_swipl_home_binds ]; then
  SWIPL_HOME_BINDS="
    --dir /lib/swipl
    --ro-bind ${SWIPL_HOME_DIR}/ABI /lib/swipl/ABI
    --ro-bind ${SWIPL_HOME_DIR}/app /lib/swipl/app
    --ro-bind ${SWIPL_HOME_DIR}/boot /lib/swipl/boot
    --ro-bind ${SWIPL_HOME_DIR}/boot.prc /lib/swipl/boot.prc
    --ro-bind ${SWIPL_HOME_DIR}/cmake /lib/swipl/cmake
    --ro-bind ${SWIPL_HOME_DIR}/customize /lib/swipl/customize
    --ro-bind ${SWIPL_HOME_DIR}/demo /lib/swipl/demo
    --ro-bind ${SWIPL_HOME_DIR}/doc /lib/swipl/doc
    --ro-bind ${SWIPL_HOME_DIR}/include /lib/swipl/include
    --dir /lib/swipl/lib
    --ro-bind ${SWIPL_HOME_DIR}/library /lib/swipl/library
    --ro-bind ${SWIPL_HOME_DIR}/swipl.home /lib/swipl/swipl.home
  "
  echo "${SWIPL_HOME_BINDS}" >${CACHE_DIR}/cached_swipl_home_binds
else
  SWIPL_HOME_BINDS=$(<${CACHE_DIR}/cached_swipl_home_binds)
fi

# Shared libraries that come with the SWI-Prolog distribution. We ignore
# libjpl because it is not needed and it has cyclic dependencies, which halts
# dependency discovery code. libpython/janus remain here as SWI-Prolog's own
# linkage (janus.pl fails to initialize if its shared object is missing); they
# are never exercised because the MeTTa libraries invoke no py-call, and the
# Python interpreter and standard library are not bound into the sandbox.
if [ ! -f ${CACHE_DIR}/cached_swipl_libs_binds ]; then
    SWIPL_LIBS=$(find ${SWIPL_HOME_DIR}/lib/x86_64-linux -type f | grep -v libjpl)
    SWIPL_LIBS_BINDS=$(generate-binds-libs ${SWIPL_LIBS})
    echo "${SWIPL_LIBS_BINDS}" >${CACHE_DIR}/cached_swipl_libs_binds
else
    SWIPL_LIBS_BINDS=$(<${CACHE_DIR}/cached_swipl_libs_binds)
fi

# Metta program to run
PROGRAM_FILE=$1
# Session path (simply the CWD of swipl)
SESSION_PATH="/tmp/session"

### WORKSPACE MODE (OPTIONAL) ###
# A single .metta file with no imports is bound directly as program.metta.
# Test suites (metta-moses, metta-attention) instead use repo-relative imports
# (../../utilities/...) and PeTTa-lib imports (lib/lib_spaces). To run those, set
# PETTA_WORKSPACE_DIR to the repo root: the whole repo is bound read-only at
# ${SESSION_PATH}/ws, swipl's cwd stays ${SESSION_PATH} (so `lib/x` resolves via
# the ${SESSION_PATH}/lib symlink to the PeTTa library), and working_dir is set
# to the test's own directory inside the repo (so `../../x` resolves within it).
WS_PARENT="${SESSION_PATH}/ws"
if [[ -v PETTA_WORKSPACE_DIR && -n "${PETTA_WORKSPACE_DIR}" ]]; then
  WS_ROOT="${PETTA_WORKSPACE_DIR%/}"
  REL="${PROGRAM_FILE#${WS_ROOT}/}"
  # Mount the repo one level deep, using its canonical project name as the
  # mount's final path component, inside a parent dir (${WS_PARENT}). This
  # replicates the real on-disk layout (e.g. .../metta-moses): several suite
  # files import with paths that walk up out of the repo and back down through a
  # directory literally named after the repo (`../../../../metta-moses/x`).
  # Binding the repo flat left no such directory above the test, so those imports
  # resolved to a non-existent path, the imported function stayed undefined, and
  # the heavy-compute assertions returned empty (or leaked the function name as
  # an atom into arithmetic). Nesting the mount makes them resolve exactly as
  # they do unsandboxed.
  #
  # The mount name must be the repo's canonical project name, which is NOT always
  # the basename of ${WS_ROOT}: when the repo comes from a Nix store input the
  # basename is a hash (e.g. `<hash>-source`), so imports keyed on `metta-moses`
  # would not resolve. Callers that know the canonical name (the Rust harness)
  # pass it in PETTA_WORKSPACE_NAME; otherwise fall back to the basename, which
  # is correct for a normally-named local checkout.
  WS_BASENAME="${PETTA_WORKSPACE_NAME:-$(basename "${WS_ROOT}")}"
  WS_MOUNT="${WS_PARENT}/${WS_BASENAME}"
  PROGRAM_BIND="--ro-bind ${WS_ROOT} ${WS_MOUNT}"
  # swipl's cwd is the repo root, matching how the suites' own runners invoke
  # PeTTa: repo-root-relative imports (feature-selection/x) and repo-relative
  # `consult` of .pl helpers resolve against it. working_dir is the test's own
  # directory so its `../../x` imports resolve within the repo. PeTTa-library
  # imports use `(library x)` (resolved via the standard library path), so no
  # cwd-local `lib` directory is required.
  CHDIR="${WS_MOUNT}"
  WORKDIR="${WS_MOUNT}/$(dirname "${REL}")"
  LOAD_PATH="${WS_MOUNT}/${REL}"
else
  PROGRAM_BIND="--ro-bind ${PROGRAM_FILE} ${SESSION_PATH}/program.metta"
  CHDIR="${SESSION_PATH}"
  WORKDIR="${SESSION_PATH}"
  LOAD_PATH="program.metta"
fi

PETTA_BLOCK_GOAL=$(petta_block_preds_goal)

### DISABLE git-import! (network isolation + import hygiene) ###
# We shadow lib_import.metta with a copy that drops git-import! from its
# exported-function list, so `!(git-import! ...)` becomes an
# inert, unregistered symbol. Generated from the pinned PeTTa so it tracks
# upstream automatically; the check fails the run loudly if the strip no-ops.
PATCHED_LIB_IMPORT="${CACHE_DIR}/lib_import.no-git-import.metta"
sed 's/git-import! //g' "${PETTA_DIR}/lib/lib_import.metta" >"${PATCHED_LIB_IMPORT}"
if grep -q 'git-import!' "${PATCHED_LIB_IMPORT}"; then
  echo "petta.sh: could not strip git-import! from lib_import.metta (upstream format changed)" >&2
  exit 1
fi
LIB_IMPORT_BIND="--ro-bind ${PATCHED_LIB_IMPORT} /lib/PeTTa/lib/lib_import.metta"

### PLN LIBRARY (OPTIONAL) ###
# When PLN_DIR is set, bind the PLN source read-only inside the sandbox and
# register it as the single MeTTaTron library_path for PLN. The mount point's
# final path component must be `PLN` because PeTTa resolves `(library PLN lib_pln)`
# via
#   library(X, Y, Path) :- library_path(Base), atom_concat(_, X, Base),
#                          atomic_list_concat([Base, '/', Y], Path).
# (see PeTTa src/metta.pl). 
# When PLN_DIR is unset, both variables stay empty and the sandbox is unchanged.
PLN_BINDS=""
PLN_LIBRARY_GOAL=""
if [[ -v PLN_DIR && -n "${PLN_DIR}" ]]; then
  PLN_MOUNT="${SESSION_PATH}/repos/PLN"
  PLN_BINDS="--ro-bind ${PLN_DIR} ${PLN_MOUNT}"
  PLN_LIBRARY_GOAL="asserta(library_path('${PLN_MOUNT}')), "
fi

if [ "${PETTA_MODE}" = "NODE" ]; then
  # NODE mode: emit NDJSON frames for println!/trace!, final result as {"type":"result","value":[...]}
  # Override println! by making it dynamic, abolishing the old clause, and asserting the new frame-emitting clause.
  NODE_BINDS=""
  GOAL="${PETTA_BLOCK_GOAL}, ${PLN_LIBRARY_GOAL}assertz(silent(true)), assertz(working_dir('${WORKDIR}')), catch(use_module(library(json)), _, use_module(library(http/json))), dynamic('println!'/2), abolish('println!'/2), asserta(('println!'(Arg,true) :- swrite(Arg,RArg), json_write_dict(current_output, _{channel:'rho:io:stdout',arguments:[RArg]}), nl(current_output))), load_metta_file('${LOAD_PATH}', Results), json_write_dict(current_output, _{type:'result', value:Results}), nl(current_output)."
else
  # NORMAL mode (default): emit single {results:[...]} JSON envelope, raw println!/trace! to stdout
  NODE_BINDS=""
  GOAL="${PETTA_BLOCK_GOAL}, ${PLN_LIBRARY_GOAL}assertz(silent(true)), assertz(working_dir('${WORKDIR}')), load_metta_file('${LOAD_PATH}', Results), catch(use_module(library(json)), _, use_module(library(http/json))), json_write_dict(current_output, #{results:Results})."
fi

### SECCOMP FILTERS ###

if [ ! $SANDBOX_LIB_PATH ]; then
  echo "SANDBOX_LIB_PATH is not defined. Please set it to the path of the
  libsandbox.so library"
  exit 1
else
  if [ ! -f ${CACHE_DIR}/cached_sandbox_lib_binds ]; then
    SANDBOX_LIB_BINDS=$(generate-binds-libs ${SANDBOX_LIB_PATH})
    echo "${SANDBOX_LIB_BINDS}" >${CACHE_DIR}/cached_sandbox_lib_binds
  else
    SANDBOX_LIB_BINDS=$(<${CACHE_DIR}/cached_sandbox_lib_binds)
  fi
fi

# Taken from an audit of syscalls while running the entire PeTTa test suite
# Allow override via environment variable
# nanosleep/clock_nanosleep are required by MeTTa `time-sleep` (Prolog sleep/1),
# used by the attention/rent-collection suites to advance the clock.
DEFAULT_SECCOMP_SYSCALL_ALLOW="read:write:open:lseek:mprotect:munmap:brk:rt_sigaction:rt_sigprocmask:access:madvise:getpid:exit:fcntl:getcwd:readlink:sigaltstack:prctl:futex:sched_getaffinity:getdents64:clock_gettime:exit_group:set_robust_list:prlimit64:getrandom:rseq:clone3:openat:fstat:newfstatat:mmap:close:ioctl:rt_sigreturn:mkdir:getuid:getgid:geteuid:getegid:gettid:tgkill:socket:connect:stat:getdents:clone:uname:arch_prctl:set_tid_address:pselect6:pipe2:dup:dup2:eventfd2:epoll_create1:epoll_ctl:epoll_pwait:writev:readv:sendto:recvfrom:getsockname:getpeername:socketpair:shutdown:setsockopt:getsockopt:bind:listen:accept4:sysinfo:nanosleep:clock_nanosleep"
SECCOMP_SYSCALL_ALLOW=${SECCOMP_SYSCALL_ALLOW:-$DEFAULT_SECCOMP_SYSCALL_ALLOW}

### BUBBLEWRAP ###

(exec bwrap \
      --dir /bin \
      --dir /usr \
      --dir /usr/share \
      --dir /usr/bin \
      --dir /lib \
      --dir /tmp \
      --dir ${SESSION_PATH} \
      --dir ${SESSION_PATH}/program \
      --dir /var \
      ${SWIPL_HOME_BINDS} \
      --ro-bind ${PETTA_DIR} /lib/PeTTa \
      ${LIB_IMPORT_BIND} \
      ${PLN_BINDS} \
      ${PROGRAM_BIND} \
      --ro-bind ${TERMINFO} /lib/terminal/terminfo \
      --ro-bind ${TERMINFO_DIRS} /usr/share/terminfo \
      ${PROGRAMS_BINDS} \
      ${SWIPL_LIBS_BINDS} \
      ${SANDBOX_LIB_BINDS} \
      ${NODE_BINDS} \
      --symlink ../tmp var/tmp \
      --symlink /lib/PeTTa/lib /tmp/lib \
      --symlink /lib/PeTTa/lib /tmp/session/lib \
      --symlink /usr/bin/sh /bin/sh \
      --symlink /lib /lib/swipl/lib/x86_64-linux \
      --proc /proc \
      --dev /dev \
      --chdir ${CHDIR} \
      --unshare-all \
      --unshare-net \
      --die-with-parent \
      --dir /run/user/$(id -u) \
      --clearenv \
      --setenv XDG_RUNTIME_DIR "/run/user/`id -u`" \
      --setenv PATH "/usr/bin" \
      --setenv SWI_HOME_DIR "/lib/swipl" \
      --setenv LANG "en_US.UTF-8" \
      --setenv TERM "${TERM}" \
      --setenv TERMINFO "/lib/terminal/terminfo" \
      --setenv TERMINFO_DIRS "/usr/share/terminfo" \
      --setenv LD_PRELOAD "/lib/libsandbox.so" \
      --setenv SECCOMP_DEFAULT_ACTION "${SECCOMP_DEFAULT_ACTION:-kill}" \
      --setenv SECCOMP_SYSCALL_ALLOW "${SECCOMP_SYSCALL_ALLOW}" \
      --file 11 /etc/passwd \
      --file 12 /etc/group \
    swipl --stack_limit=8g -q -s /lib/PeTTa/src/metta.pl -g "${GOAL}" -t halt \
    11< <(getent passwd $UID 65534) \
    12< <(getent group $(id -g) 65534) \
)
