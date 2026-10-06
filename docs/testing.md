# Local evaluation

## Tests without accounts or publishing

From the main `repo/` checkout:

```sh
mise install
mise run test-cli
BIN_DIR="$PWD/.local/bin" mise run install-cli
mise run fmt-check
```

The compiled-binary tests start an ephemeral loopback fixture. They exercise the browser callback,
PKCE, invalid state, private credential storage, refresh, revocation, JSON output, stdin, sole-Brain
login selection, opt-out, existing and concurrent selection preservation, discovery and verification
failures, failed-switch rollback, selection from the current working directory, request identity,
and one-time secret files. Fixtures contain synthetic identities and knowledge and do not call a
model or external storage. Compressed-response fixtures cover Brotli and gzip across OAuth login,
refresh, search results, and service permission errors. Skill installation is tested independently
through `mise run test-cli-skills` against `repo/cli/skills`.

Installer tests replace development and release fixtures in the same temporary directory, preserve
the previous executable when a replacement fails validation, and cover existing symlinks and paths
with spaces. The `BIN_DIR` override above keeps this test installation inside the repository. For
normal use, `mise run install-cli` installs the current files, including uncommitted edits, into
`~/.local/bin/cortex`. Repeat it after changes; a separate source copy or commit is unnecessary.

From `repo/`, also run:

```sh
mise run test-cli-api
```

These workerd tests exercise real OAuth registration, consent, exchange and revocation, current-role
changes, creation retries, retrieval, recording receipts, invitation roles, stale ownership, service
suspension, and existing MCP regression coverage. External storage and model boundaries are
substituted. This is separate from the Rust fixture suite; neither claims a live provider test. The
service's repository tests also exercise direct retrieval without a Sandbox binding, including file
and match pagination, partial failures, pinned ranges, and the selected Sandbox path.

## Try your own local Brain

1. In the service checkout, follow its local development setup for Code Storage and the private
   admin password. Use the tracked `.dev.vars.example` files as the variable reference. Credentials
   belong only in the existing ignored secret inputs.
2. Start `mise run dev` at the service root. Reuse an existing instance when present. Its shared
   origin is `http://second-brain.localhost:1355`.
3. Use `/admin/` to invite a test user if needed. Accept the development invitation link in the
   browser and complete or skip the personal name. Operator login remains separate.
4. In an evaluation directory, run `cortex --origin http://second-brain.localhost:1355 login`.
   Complete browser sign-in and consent. Local delivery returns a masked link in the browser; no
   email is sent.
5. Login selects your only Brain when the directory has no saved selection. Use
   `cortex login --no-select` to opt out. Check `cortex config`; if nothing is selected, run
   `cortex brains list`, then `cortex use <id>`. To create a Brain, get an ID with
   `cortex request-id`, run `cortex brains create "Evaluation" --request-id <id>`, and select the
   returned Organization ID with `cortex use`.
6. Install the complete deployed plugin, restart or reload the harness's instructions if needed, and
   give it the installed binary's PATH. Ask it to record synthetic facts and retrieve them with
   citations. Inspect People and Connected apps in the browser to compare outcomes.
7. Use `cortex logout` when finished. Remove test Brains only through tracked admin cleanup. Do not
   wipe local databases or remove provider repositories manually.

Use a new private directory through `CORTEX_HOME` to isolate CLI credentials during evaluation.
Commands automatically respect the current directory's `.cortex/config.toml`. Use
`--project /path/to/project` to target another folder. No global Brain selection is stored.
`--origin` must match the running service origin exactly. Login listens on `127.0.0.1`; a remote
browser must be able to reach that loopback callback on the CLI's machine. A network sandbox may
need permission for the listener and service connection.

## Public mirror boundary

All maintained source and release work stays in `repo/`. The sibling `cli` and `skills` repositories
are public publishing mirrors for source discovery and raw GitHub content links. Never develop or
create commits there. The mirrored packages retain their own README, mise tasks, and installers so
consumers can use them independently. CLI builds need no skills checkout or service source;
authenticated operations still need a compatible Cortex service.

Official releases use `mise run build-cli-release` and `mise run publish-cli-release` from `repo/`.
Run them after checks, a commit, mirror publication, and signoff. The installer defaults to
published binaries and supports stdin, `--release`, `--from-file`, and explicit `--source`
installation. Installer tests exercise all four platform mappings, latest and pinned versions,
checksum and executable-version failures, and preservation of installed files. Run
`mise run test-cli-mirrors` from `repo/` to verify signed synchronization and destination
protection.
