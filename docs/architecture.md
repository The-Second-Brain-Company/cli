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

Manual browser login uses dynamic client registration, email sign-in and consent, authorization code
with S256 PKCE, a random state, issuer verification, and an ephemeral IPv4 loopback callback. CLI
0.5.0 also supports email-code login entirely in chat, without a browser or listener. Requests use
the existing account grant with explicit Brain IDs. The OAuth resource remains
`<origin>/app/api/mcp`: it identifies the existing Cortex authorization audience shared by these
transports. The CLI is a separately consented, inspectable, revocable connected app.

The adapter intersects consented scopes with current roles and enforces the existing public service
and capacity controls. It rejects unregistered and retired operations and accepts no operator
credentials. Long waits recheck the grant before returning. Private administration retains its
existing inspection, revocation, suspension, run cancellation, repository retry, and cleanup paths.
CLI retrievals appear as `cli` tool observations alongside their HTTP retrieval ledger entries.

Email login requires `--email` and explicit `--approve-scopes`. The setup ceiling is knowledge:read,
knowledge:write, brains:access, account:manage, and brains:create. The agent discloses these actions
before requesting the email and preferred name. The code verifies identity and enters only through
bounded stdin. The CLI privately composes POST session, code acceptance, GET/POST consent, and
normal PKCE token exchange. It validates origin/discovery, email, client, resource, permissions,
callback, state, and issuer. It never follows a redirect or prints a challenge, session cookie,
OAuth code, verifier, or token.

The permission preview uses the public clientName, redirectUri, scopes, and connection-type fields.
Internal authorization and User records are not part of that HTTP response. The server validates the
retained client, resource, and PKCE input during consent and exchange.

One tagged pending record per origin stores each successful secret boundary before the next request.
Existing credentials survive until atomic token replacement. A different pending request requires
explicit cancellation. `--resume` continues a saved session or authorization; ambiguous token
exchange requests a fresh OAuth authorization code from retained consent instead of replaying a
consumed code. The ordinary grant remains visible and revocable in Connected apps. Tokens are saved
before temporary-session cleanup. Failed cleanup stays pending, including across logout, and resumes
without repeating consent. Existing sessions can last seven days; offline cancellation cannot
guarantee immediate server revocation. Explicit cancellation can discard corrupt local pending state
while reporting remote cleanup as unknown. No submitted email code is persisted.

Email login changes no project selection. Ordinary profile, Brain list/create/use, and whoami
commands complete chat setup, then invite a fresh chat. A knowledge interview and save are separate
from installation. Hosted MCP retains client-owned OAuth and does not inherit local credentials.

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
provide inspection and waiting. Public waitlist and invitation acceptance retain their bootstrap
contracts. Email-code sign-in is shared by browser and CLI setup. Retired conversations and Virtual
operations remain unavailable.

The main `repo/` checkout owns all CLI development, testing, builds, and release work under
`cli/cli`; the sibling `cli` and `skills` repositories are public mirrors for discovery and raw
GitHub content links. The main repository builds official macOS/Linux arm64 and x86_64 binaries with
pinned Rust and a digest-pinned Linux container, then publishes them in a GitHub release on the
exact signed public CLI mirror commit. Its publisher verifies the committed build inputs, complete
platform set, and all checksums before publishing the release and its `latest.txt` asset.
No-argument installation reads the latest GitHub release's `latest.txt` and works through stdin,
including from the raw GitHub mirror, without a checkout or toolchain. `CORTEX_VERSION` pins an
available release. `--source` explicitly requests a working-file build. The installer verifies
checksums and the executable's version before atomic replacement. The CLI default origin is the
canonical production service; local evaluation chooses its origin explicitly. Cargo publishing stays
disabled. Complete Codex and Claude plugins bundle the portable CLI workflow, so normal installs
need no separate global skill. The fallback MCP audience selects one Brain during OAuth consent; CLI
authentication remains account-wide for per-directory selection.

## CLI release compatibility

`cortex update` runs before origin, project selection, credentials, and API initialization. Its
unauthenticated download client reads the official stable GitHub release and version-pinned assets.
It never uses response hints as executable URLs. The command locks the destination directory, checks
the currently installed version, verifies the platform checksum and candidate version, and
atomically replaces the executable through a sibling temporary file. Failure preserves the installed
binary. A current or newer installed version is a successful no-op.

The API's `cli-release.json` declares independent latest and minimum versions. The CLI adapter
checks its versioned User-Agent before authentication, body parsing, and product dispatch. Missing
or malformed versions fail with `cli_version_required`; versions below the minimum fail with
`upgrade_required`. Shared service availability still applies. Authentication routes, browser and
MCP requests, private administration, and release downloads retain their own boundaries.

CLI responses include latest and minimum version headers. The client validates and collects hints
before decoding the body, including empty responses, then adds `context.update` without changing the
requested data or retry identity. Invalid optional metadata does not fail a successful command. No
command polls releases or installs updates in the background. The explicit update command alone
checks release metadata.

Publish and verify the compatible binary before deploying a policy that advertises it. The API
deployment task checks the public stable release, all four platform assets, their checksums, and
matching release metadata. Operators change the checked-in policy and redeploy to raise a minimum or
recover from an incorrect minimum. A CLI major version does not automatically raise the minimum.

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
