# Brain CLI development

Use mise tasks from this directory. Rust commands run through pinned Rust and mr-boxington; Usage
owns command parsing, help, and completions. Keep dependency versions exact and commit Cargo.lock.

Keep this repository independently buildable when mirrored. Use `src/` for Rust, `tests/` for
integration tests, and `scripts/` for installation tooling. Portable agent skills are maintained in
a separate repository; never depend on its checkout or the private service repository to build,
test, or install this CLI. Keep code free of comments; put rationale in Markdown. Use Oxfmt for
Markdown and configuration and rustfmt for Rust.

This is a local evaluation release. Cargo publishing is disabled; keep binary, crate, plugin,
marketplace, and website publication out of these tasks. Existing MCP packages remain unchanged. Run
`mise run test`, `mise run fmt`, and `mise run fmt-check` before committing. The service repository
separately tests its CLI adapter and OAuth authorization.

CLI operations use the service's shared OAuth, role checks, and product handlers. Keep Brain
selection verified, credentials outside projects, retry IDs stable, and saved claims tied to durable
receipts. The private operator boundary remains separate.
