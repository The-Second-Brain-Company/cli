# Cortex CLI development

Develop only in the main `repo/` checkout under `cli/cli`. Run the root's `build-cli`,
`install-cli`, and `test-cli` mise tasks from `repo/`. The sibling `cli` repository is a public
mirror for release discovery and raw GitHub installer links; never edit or commit there. Keep the
Rust package independently buildable when mirrored, with exact dependencies and Cargo.lock. Keep
code free of comments; put rationale in Markdown. Run root test, fmt, and fmt-check tasks before
committing.

Every commit changing `repo/cli/cli` or `repo/cli/skills` MUST synchronize and push both sibling
mirrors and verify both remote `master` heads. Install the main repository's hooks with
`mise run hooks-install`. Its post-commit hook publishes automatically. If hooks are unavailable,
bypassed, or fail, run `mise run publish-cli-mirrors` from `repo/` and resolve failures before
reporting completion. A failed publication leaves the source commit intact.

The CLI is the preferred local agent interface and is distributed alongside complete Codex and
Claude plugins. Preserve surface parity through shared service handlers and authorization. Read
[architecture](docs/architecture.md) when changing authentication, project selection, retry
handling, or installation. The CLI reads the working directory's .cortex/config.toml itself and
supplies the Brain ID. TOML is preferred; JSON is read compatibility. Keep credentials per user and
origin, outside projects. Verify switches with rollback and preserve original Brain/request IDs for
retries.

Development installation uses current working files through `mise run install-cli` or the explicit
`scripts/install.sh --source` mode. The installer defaults to the latest published binary and must
work through stdin without a checkout, Rust, or mise. Public releases use the main repository's
pinned build and website publication tasks after tests and a commit; Cargo publishing stays
disabled. Publish committed source to both public mirrors after every relevant commit. Complete
plugins bundle the portable CLI workflow maintained under `repo/cli/skills`. Keep private operator
recovery separate.
