# Cortex

Your Brain from the terminal, Codex, and Claude Code.

Search shared knowledge, record what matters, switch Brains, and manage access through a small Rust
CLI. Portable skills teach an agent how to choose the right Brain, cite sources, finish onboarding,
and recover from an uncertain save.

**Local evaluation preview.** Build and install from the files in this directory. No crate, binary
release, marketplace, or npm package is published. The existing MCP and plugin integrations remain
available.

## Quick start

Install [mise](https://mise.jdx.dev/) 2026.9.2 or newer, then run these commands from this
directory:

```sh
mise trust
mise install
mise run test
mise run install
export PATH="$HOME/.local/bin:$PATH"
cortex --help
```

`mise run install` builds the files in this existing repository directory, including saved,
uncommitted changes, in release mode and installs `cortex` at `~/.local/bin/cortex`. Rerun it after
editing the CLI. It copies the built executable, so changes take effect after reinstalling. Cargo
downloads the locked third-party dependencies on the first build. Subsequent builds use
[mr-boxington](https://github.com/jdx/mr-boxington) through mise's Rust integration. The tool
versions are pinned in `mise.toml`.

`~/.local/bin/cortex` is the shared installation location for development builds and future
production releases. Either installer replaces the current binary there; no uninstall or Cargo
registration is needed. The installer verifies the replacement before moving it into place.
Authentication and project configuration live elsewhere and are preserved. Set `BIN_DIR` to use a
different directory:

```sh
BIN_DIR="$PWD/.local/bin" mise run install
```

Use the same `BIN_DIR` for both installers when switching versions. If an earlier installation is
first on PATH, the installer reports it. Keep the shared installation directory first on the PATH
used by your terminal and agent. From the full Cortex repository root, `mise run install-cli` runs
this same development installation.

After `mise trust` and `mise install`, you can also run `bash scripts/install.sh` from this
directory. The script belongs to this repository. The future public `/install-cli` entry point will
redirect to its raw GitHub URL once the CLI mirror and public distribution are configured. During
local evaluation, piping the script requires `CORTEX_CLI_SOURCE_DIR` to point at an existing CLI
checkout:

```sh
cat scripts/install.sh | CORTEX_CLI_SOURCE_DIR="$PWD" bash
```

To install an executable already built on disk, run
`bash scripts/install.sh --from-file /absolute/path/to/cortex`. This uses the same replacement step
without rebuilding. Public release downloads remain deferred during local evaluation.

Start a Cortex service with CLI support. In the full Cortex checkout, run `mise run dev` from its
root, following its existing local service setup. The CLI defaults to
`http://second-brain.localhost:1355`. Use `--origin https://your-service.example` or `CORTEX_ORIGIN`
to select another compatible service. This preview's backend adapter has not been deployed publicly.

```sh
cortex login
cortex brains list
cortex use org_1234567890abcdef
cortex whoami
cortex search "release decisions"
```

Use an ID returned by `brains list`. Login opens email sign-in and browser consent. It requests the
existing knowledge, Brain, account, and management permissions; the browser shows the consent and
the server checks your current role on each operation. For read-only use:

```sh
cortex login --scopes "knowledge:read brains:access"
```

For an agent terminal, `cortex login --no-browser` prints an authorization URL to stderr and waits
for the browser callback. Keep that process alive. Credentials are stored privately by the CLI;
there is no token copy/paste step. See [local testing](docs/testing.md) for setup and test
boundaries.

## Add the skill

The agent skills live in an independent Cortex skills repository with its own installer. From that
repository directory, install them globally for your user:

```sh
mise trust
mise install
mise run install
```

This copies the current skill, references, and logo to `~/.agents/skills/cortex` for Codex and
`~/.claude/skills/cortex` for Claude Code. It works from any directory without project instruction
files. Update both copies with `mise run install -- --replace` after editing the skill. From the
full Cortex repository root, use `mise run install-cli-skills -- --replace`.

Project-only skill installation remains available with `--project /path/to/your/project`; see the
skills repository's README for that fallback and supported environments. Both repositories build,
install, and test independently.

Make `~/.local/bin` (or your `BIN_DIR`) available on the agent process's PATH, or give it the
absolute path to `cortex`. Then ask:

> Use the Cortex CLI to find our release decisions and cite the sources.

> Create a Brain called Field Notes, select it, and help me onboard it.

> Remember that launch approval belongs to the product lead.

## Brain selection

The CLI and skills are installed globally for your user. Brain selection stays in the current
directory's `.cortex/config.toml`. Plain `cortex ...` commands automatically read and write this
file:

```toml
brain_id = "org_1234567890abcdef"
```

`cortex use <id>` verifies access, atomically writes the file, reads it back, and verifies again. A
failure restores the previous selection. `cortex config` shows the local configuration.

- `--project` targets a different directory; the default is the current directory. There is no
  global default Brain. Parent directories are not searched, so a neighboring project cannot
  silently select a Brain.
- `--brain <id>` overrides the selection for one command without changing the file.
- Login selects the account's only Brain when the project has no configuration and access verifies
  successfully. Use `cortex login --no-select` to opt out. Existing selections are preserved; zero
  or multiple memberships require a choice with `cortex use`. An explicit `--brain` on login also
  leaves saved configuration unchanged.
- Login reports its selection outcome in `data.selection`. A discovery or verification failure
  leaves you signed in and reports `selected: false` with an error; select a Brain later with
  `cortex use`. Existing invalid TOML or JSON-only configuration is preserved for explicit recovery.
- Creation returns a new Brain but leaves selection unchanged. Run `cortex use <new-id>` after
  creation succeeds. This keeps creation retries attached to their original request context.
- JSON selection is deferred. If only `.cortex/config.json` exists, select its intended Brain with
  `cortex --project /path/to/project use <id>`; the JSON file is preserved. When both exist, this
  CLI uses TOML and MCP uses JSON. Keep them aligned intentionally while evaluating both transports.

Account credentials live in `$CORTEX_HOME`, `$XDG_CONFIG_HOME/cortex`, or `~/.config/cortex`, in
that order. Set `CORTEX_HOME` to a private directory outside projects for isolated testing.
Credentials are separated by normalized service origin, written atomically with mode `0600` on Unix,
and protected by a lock during refresh. `cortex logout` revokes the connection before removing the
local file. It does not sign the browser out.

## Knowledge

```sh
cortex knowledge list
cortex search "pricing approval" --limit 20
cortex read pricing.md --start-line 1 --limit 200
cortex knowledge grep "approved" --path decisions
cortex knowledge document ORGANIZATION.md
cortex knowledge show
cortex request-id
cortex record --file facts.md --request-id chosen-stable-id
cortex record --file - --request-id another-stable-id < facts.md
cortex record --file facts.md --attachment notes.csv --target finance --request-id import-stable-id
cortex runs get run_1234567890abcdef --wait 25
```

Results include revisions, pagination, and source URLs. Pin related reads with `--revision <sha>`.
Search is lexical. Follow `nextOffset` and `nextLine`, and inspect per-file errors before concluding
that a result is complete. For grep, carry both `nextOffset` and `nextMatchOffset` when present. The
service supports Sandbox and storage API retrieval; local development works without a Sandbox
binding. `knowledge read-many --file ranges.json` accepts up to eight
`{"path":"file.md","startLine":1,"limit":200}` objects in an array.

Recording invokes the service's writing model, which selects files and commits validated knowledge.
It waits up to 25 seconds by default. Only `status: saved` confirms a commit. Continue a running
result with `runs get`, using the same Brain ID. Preserve the request ID, original Brain, content,
and attachment bytes on retries. `not_saved`, failed, and cancelled runs return a nonzero exit.
`--wait 0` returns the submitted run immediately.

For exact requested edits, `knowledge replace --file change.json` accepts `content`, `baseRevision`,
and optional `documents` and `summary`. Read current content first and merge after a revision
conflict. Normal fact recording needs no manual placement or preflight read. Attachments are
retained in domain-local `_attachments/` before optional extraction. Specify `--target` when
recording local uploads, or `--sources sources.json` for saved `[{path, revision}]` references.
Backend PDF and Office extraction remains unsupported; client document tools can process those
formats.

## People and access

```sh
cortex people list
cortex people invite colleague@example.com --role read
cortex people role usr_1234567890abcdef --role write
cortex people revoke inv_1234567890abcdef
cortex people transfer usr_1234567890abcdef --expected-owner usr_fedcba0987654321
cortex repository access list
cortex repository access create "Local checkout" --secret-file /private/path/git-access.json
cortex repository access revoke access_1234567890abcdef
cortex repository retry
cortex connections list
cortex account show
cortex account profile "Your name"
cortex connections disconnect ogr_1234567890abcdef
cortex account disconnect ogr_1234567890abcdef
cortex runs cancel run_1234567890abcdef
```

Invitations default to Read only and send email outside local development. For a local invitation,
add `--secret-file /private/path/invitation.json` to preserve its one-time acceptance link. Without
that option the CLI discards the development link. One-time links and Git tokens never appear on
stdout. Secret destinations must be new absolute paths outside the working directory (or the
explicit `--project` directory); their parent directories must already exist. Only the current Owner
can transfer ownership. Operator suspension, limits, deletion, and recovery remain in the private
administration console.

## Built for agents

Ordinary commands write one JSON object to stdout. Progress and the browser authorization URL go to
stderr. Help, version, specification, and completion commands return their native text.

```json
{
  "ok": true,
  "data": { "status": "running", "runId": "run_1234567890abcdef" },
  "context": {
    "origin": "http://second-brain.localhost:1355",
    "brain_id": "org_1234567890abcdef",
    "request_id": "chosen-stable-id"
  }
}
```

Errors return `ok: false` with `error.code`, `message`, optional HTTP `status`, and operation
context. A write followed by an unsuccessful wait retains the submitted run ID in error details. The
CLI does not automatically replay writes. Exit codes are `0` for an accepted operation (including a
running recording), `2` for input/configuration errors, `3` for authentication, `4` for permission
denial, `5` for conflicts, and `1` for other failures. Use `--pretty` for indented JSON.

An `io`, `network`, or `protocol` error means the command failed before returning usable knowledge.
Resolve the technical failure before drawing conclusions from search. Sandboxed hosts must allow
authenticated commands to access the global credential directory and its lock as well as the service
connection. Use the host's permission flow while preserving private credential permissions.

[Usage](https://github.com/jdx/usage) is the source for commands, arguments, help, and completions:

```sh
cortex __usage_spec__ > /tmp/cortex.usage.kdl
mise exec -- usage generate json --file /tmp/cortex.usage.kdl
cortex completions zsh
```

## Development

```sh
mise run build
mise run test
mise run fmt
mise run fmt-check
```

`src/` contains the Rust binary, `tests/` contains integration tests, and `scripts/install.sh` is
the installation entry point. This repository contains its own build, test, and installation
tooling. Rust tests run the compiled binary against a synthetic HTTP/OAuth service; the Cortex
service has separate real OAuth, role, and recording integration tests. See
[architecture](docs/architecture.md) and [local testing](docs/testing.md).

The design takes inspiration from [Basecamp for agents](https://basecamp.com/agents) and
[Basecamp's skills](https://github.com/basecamp/skills), adapted to verified Brain selection,
durable knowledge recording, and Cortex's current access model.

## Scoped access

Paths start at the Brain repository root. `access show` and `access explain <path>` inspect current
permissions. Management commands include `access scopes`, `access activate`, `access promote`,
`access operations`, and `people access --file <json> [--preview]`. JSON mutations preserve their
request ID, reason, and expected generation. Invitations require `--role` or `--policy-file` and
`--request-id`.

`knowledge patch --file <json>` accepts `{files, baseRevision, summary}`. Restricted recording uses
`record --target <directory-or-file>`. Follow `nextCursor` with `--cursor` on search/grep. Moves use
`knowledge move --file <json> --preview` and then the exact reviewed plan without `--preview`;
`knowledge discard-move --file <json>` abandons an unapplied plan. Applied uncertain operations
require private recovery. These commands add no authority beyond the service's current membership,
file policy, and OAuth consent. This CLI remains a local evaluation release.

## Retained files and templates

```sh
cortex knowledge upload deliverables/_attachments/report.docx --file report.docx --base-revision REVISION --request-id save-template --summary 'Save report template'
cortex knowledge copy --file copy.json
cortex knowledge download projects/atlas/_attachments/report.docx --revision REVISION --output report-local.docx
cortex knowledge upload projects/atlas/_attachments/report.docx --file report-local.docx --replace --base-revision REVISION --request-id fill-report --summary 'Fill Atlas report' --sources sources.json
```

`copy.json` supplies `source: {path, revision}`, destination `path`, `baseRevision`, `requestId`,
and `summary`. Use actual full revisions. Copy supports attachments and Markdown templates. Uploads
preserve bytes and allow 5 MiB per file. Downloads use one authenticated request, verify byte count
and SHA-256, and refuse to overwrite existing local files. Replacement preserves known source
restrictions and adds references from `sources.json`. Reuse identical arguments and the request ID
after uncertainty; private receipt recovery resolves an ambiguous commit.

CLI and skills remain local evaluation distributions. File operations use shared service access
checks. Binary editing depends on the client's document tools.
