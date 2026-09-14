# Gemini CLI Instructions

<!-- ste-policy: required -->

This file gives Gemini CLI the project-specific source of instructions.

**Primary instructions:** Read [CLAUDE.md](./CLAUDE.md) for the complete project guidelines.

All AI coding assistants in this project use `CLAUDE.md` as the source for:

- Project context and overview
- Git interaction policies
- Code style guidelines
- Security requirements
- Testing approach
- Architecture documentation

## Why `CLAUDE.md` Is Canonical

Smart Assets maintains vendor-neutral documentation. The other assistant files refer to `CLAUDE.md` to prevent duplicate or inconsistent instructions.

## Configuration and Metadata Files

Prefer JSON with Comments (JSONC) files for human-edited configuration and metadata, including claim inventories.

Follow the [configuration file conventions](./CLAUDE.md#configuration-file-conventions) for the `.jsonc` extension, comment support, parser requirements, migrations, and strict JSON exceptions.

## Gemini-Specific Notes

No Gemini-specific exceptions are defined.

## Related Files

- `CLAUDE.md` - Complete project instructions
- `AGENTS.md` - Condensed instructions for compatible coding assistants
