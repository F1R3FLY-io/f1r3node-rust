// Library interface for the file-read-service sidecar.  Exposes
// the primitives the binary and integration tests both use.
//
// The sidecar's job is narrow: serve bytes from operator-declared
// static files via HTTP, coordinating with the node's writes via
// POSIX advisory locks.  Everything that's not that lives
// elsewhere (rate limiting, auth, TLS → external reverse proxy;
// consensus-mode path resolution → the node).
//
// # Module layout
//
//   * `config` — HOCON schema parse.
//   * `index`  — resolved logical→(file,dir) index.
//   * `fcntl`  — F_RDLCK bracket for cross-process coordination
//     with the node's F_WRLCK bracket.
//   * `handler` — axum HTTP handler for GET /files/{logical}.
//   * `server` — axum router + startup helper.

pub mod config;
pub mod fcntl;
pub mod handler;
pub mod index;
pub mod server;
