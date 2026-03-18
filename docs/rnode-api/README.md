# RNode API Documentation Notes

This directory contains generated reference material for the node's gRPC and HTTP-facing schemas.

## Source Of `index.md`

`index.md` is derived from the protobuf definitions in:

- `models/src/main/protobuf/`
- `node/src/main/protobuf/`

Those files define the deploy, propose, diagnostics, and term schemas used by the Rust node.

## Regeneration

If you update the protobuf files, regenerate the derived API reference using the local documentation tooling or the same workflow that produced the checked-in `index.md`.

## OpenAPI

Any OpenAPI or Swagger material in this directory should be treated as generated documentation and kept aligned with the protobuf and HTTP route definitions in the Rust workspace.
