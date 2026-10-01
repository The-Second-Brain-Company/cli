# Cortex CLI development

Use the pinned mise tasks from this directory. Keep the Rust package independently buildable when
mirrored, with exact dependencies and Cargo.lock. Keep code free of comments; put rationale in
Markdown. Run test, fmt, and fmt-check before committing.

The CLI is the preferred local agent interface and is distributed alongside complete Codex and
Claude plugins. Preserve surface parity through shared service handlers and authorization. Read
[architecture](docs/architecture.md) when changing authentication, project selection, retry
handling, or installation. The CLI reads the working directory's .cortex/config.toml itself and
supplies the Brain ID. TOML is preferred; JSON is read compatibility. Keep credentials per user and
origin, outside projects. Verify switches with rollback and preserve original Brain/request IDs for
retries.

Development installation uses current working files. Public releases use the service repository's
pinned build and website publication tasks after tests and a commit; Cargo publishing stays
disabled. Portable skills remain an independent mirror and complete plugins bundle their CLI
workflow. Keep private operator recovery separate.
