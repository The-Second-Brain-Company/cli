# Architecture

The CLI sends ordinary HTTP requests to `/app/api/cli`. It does not initialize MCP, discover tools,
or execute remote JavaScript. The service adapter dispatches registered product operations through
the same `organizationRequest` authorization and handlers used by browser and MCP requests.

OAuth and API requests share an HTTP client with Brotli and gzip response decoding. Local proxies
can compress JSON responses; parsing the compressed bytes as JSON would turn successful requests and
service errors alike into protocol failures.

Search, grep, and ranged reads use the shared retrieval route. The service selects its configured
backend, including the storage API used by local development without Sandbox. The CLI follows the
returned cursors and revision without choosing a backend itself.

## Authentication and authority

Login uses dynamic client registration, browser email sign-in and consent, authorization code with
S256 PKCE, a random state, issuer verification, and an ephemeral IPv4 loopback callback. Requests
use the existing account grant with explicit Brain IDs. The OAuth resource remains
`<origin>/app/api/mcp`: it identifies the existing Cortex authorization audience shared by these
transports. The CLI is a separately consented, inspectable, revocable connected app.

The adapter intersects consented scopes with current roles and enforces the existing public service
and capacity controls. It rejects unregistered and retired operations and accepts no operator
credentials. Long waits recheck the grant before returning. Private administration retains its
existing inspection, revocation, suspension, run cancellation, repository retry, and cleanup paths.
CLI retrievals appear as `cli` tool observations alongside their HTTP retrieval ledger entries.

A local credential lock serializes refresh and login for an origin. HTTP redirects are disabled to
keep authorization and tokens at the configured origin. HTTP origins are allowed only for localhost;
other origins require HTTPS. Tokens never appear in command arguments, project configuration, or
normal output. Unix credential directories are `0700` and files are `0600`. Native OS keychain
storage is a future option; Windows credential permissions are not yet a tested target.

Authenticated commands need local permission to access the credential directory and its lock,
including read-only searches. A sandbox denial returns an `io` error with a permission hint; the
host's permission flow grants access without changing credential storage or file permissions.

## Selection and retries

The selected Brain is a project-local immutable Organization ID. Commands automatically use the
current directory's `.cortex/config.toml`; `--project` targets another directory. There is no global
selection. An explicit `--brain` overrides the selection for one invocation. A missing, malformed,
selection stops knowledge operations. No existing project files are migrated or rewritten on
discovery. `use` performs verified atomic replacement and rollback under a project lock.

Creation and recording require caller-owned retry IDs. Creating a Brain does not switch the
selection automatically. The agent skill performs the subsequent verified switch after creation is
confirmed. This preserves the original assertion and request identity across lost responses. Running
records continue through bounded waits, and successful process execution is distinct from a saved
receipt. Exact edits retain optimistic revision checks; the CLI never silently merges or retries
them.

After successful authentication, login selects the sole account membership if the project is
unconfigured. `login --no-select` or a per-command `--brain` skips this step. Existing selections
are preserved, including a selection saved during membership discovery. Automatic selection uses the
same project lock, verification, readback, and rollback as `use`. Discovery and selection errors are
returned in `data.selection` while `signed_in` remains true. Zero or multiple memberships leave the
project unconfigured. Invalid configuration requires explicit recovery with `use`.

## Installation and release

Development installation builds the current working files with the pinned Cargo toolchain, reads the
executable path from Cargo's build output, and passes it to `scripts/install.sh --from-file`. That
shared installation step validates a temporary executable and atomically replaces
`~/.local/bin/cortex`, or `BIN_DIR/cortex` when configured. The production download path uses the
same destination and replacement step so either version can replace the other. Binary installation
keeps credentials and project selections intact.

The service adapter adds transport aliases for current operations. It adds no role, management
scope, data store, migration, or public operator API. Browser and MCP results keep their existing
contracts. WebSocket streaming is intentionally outside this command/response CLI; status and runs
provide inspection and waiting. Public waitlist signup and email/invitation acceptance stay in the
browser bootstrap flow. Retired conversations and Virtual operations remain unavailable.

The service repository builds official macOS/Linux arm64 and x86_64 binaries with pinned Rust and a
digest-pinned Linux container, then publishes them through the Public Website after checks and a
commit. The installer verifies checksums before atomic replacement. The CLI default origin is the
canonical production service; local evaluation chooses its origin explicitly. Cargo publishing stays
disabled. Complete Codex and Claude plugins bundle the portable CLI workflow, so normal installs
need no separate global skill. The fallback MCP audience selects one Brain during OAuth consent; CLI
authentication remains account-wide for per-directory selection.

## Naming compatibility

Cortex reads legacy `.brain/config.toml` selections with `brain_id` when no `.cortex` selection
exists. New selections write `.cortex/config.toml` with `brain_id`. Existing `cortex_id` fields
remain readable. An invalid `.cortex` file stops the operation instead of falling back. Existing
`BRAIN_HOME`, `BRAIN_ORIGIN`, and private `second-brain` credential directories remain usable; no
files are automatically copied or rewritten. The service keeps existing OAuth scope strings and
accepts `/cortexes` as an alias for `/brains`.

The preferred project format is TOML. When only JSON exists it remains readable; TOML takes
precedence within the selected .cortex or .brain directory. Conflicting brain_id/cortex_id aliases
or invalid preferred configuration stop access. cortex use always writes verified TOML.
