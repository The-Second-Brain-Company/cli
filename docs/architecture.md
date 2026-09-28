# Architecture

The CLI sends ordinary HTTP requests to `/app/api/cli`. It does not initialize MCP, discover tools,
or execute remote JavaScript. The service adapter dispatches registered product operations through
the same `organizationRequest` authorization and handlers used by browser and MCP requests.

## Authentication and authority

Login uses dynamic client registration, browser email sign-in and consent, authorization code with
S256 PKCE, a random state, issuer verification, and an ephemeral IPv4 loopback callback. Requests
use the existing account grant with explicit Brain IDs. The OAuth resource remains
`<origin>/app/api/mcp`: it identifies the existing Second Brain authorization audience shared by
these transports. The CLI is a separately consented, inspectable, revocable connected app.

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

## Selection and retries

The selected Brain is a project-local immutable Organization ID. TOML is read from exactly the
chosen project directory. An explicit flag overrides it for one invocation. A missing, malformed, or
JSON-only selection stops knowledge operations. No existing project files are migrated or rewritten
on discovery. `use` performs verified atomic replacement and rollback under a project lock.

Creation and recording require caller-owned retry IDs. Creating a Brain does not switch the project
automatically. The agent skill performs the subsequent verified switch after creation is confirmed.
This preserves the original assertion and request identity across lost responses. Running records
continue through bounded waits, and successful process execution is distinct from a saved receipt.
Exact edits retain optimistic revision checks; the CLI never silently merges or retries them.

After successful authentication, login selects the sole account membership if the project is
unconfigured. `login --no-select` or a per-command `--brain` skips this step. Existing selections
are preserved, including a selection saved during membership discovery. Automatic selection uses the
same project lock, verification, readback, and rollback as `use`. Discovery and selection errors are
returned in `data.selection` while `signed_in` remains true. Zero or multiple memberships leave the
project unconfigured. Invalid or JSON-only configuration requires explicit recovery with `use`.

## Local evaluation boundary

The service adapter adds transport aliases for current operations. It adds no role, management
scope, data store, migration, or public operator API. Browser and MCP results keep their existing
contracts. WebSocket streaming is intentionally outside this command/response CLI; status and runs
provide inspection and waiting. Public waitlist signup and email/invitation acceptance stay in the
browser bootstrap flow. Retired conversations and Virtual operations remain unavailable.

This preview is explicitly local. Cargo has `publish = false`; there are no release/upload tasks.
The existing Codex and Claude plugin manifests, skill packages, public website, and production
infrastructure are unchanged. Removing public MCP access and running CLI-versus-MCP model evals are
separate future decisions.
