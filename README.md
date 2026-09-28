# Brain

Your Second Brain from the terminal, Codex, and Claude Code.

Search shared knowledge, record what matters, switch Brains, and manage access through a small Rust
CLI. Portable skills teach an agent how to choose the right Brain, cite sources, finish onboarding,
and recover from an uncertain save.

**Local evaluation preview.** Build and install from this checkout. No crate, binary release,
marketplace, or npm package is published. The existing MCP and plugin integrations remain available.

## Quick start

Install [mise](https://mise.jdx.dev/) 2026.9.2 or newer, then run these commands from this
directory:

```sh
mise trust
mise install
mise run test
mise run install
export PATH="$PWD/.local/bin:$PATH"
brain --help
```

`mise run install` builds the local Rust package and places `brain` in `.local/bin`. It does not
publish or download a released Brain binary. Cargo downloads the locked third-party dependencies on
the first build. Subsequent builds use [mr-boxington](https://github.com/jdx/mr-boxington) through
mise's Rust integration. The tool versions are pinned in `mise.toml`.

After `mise trust` and `mise install`, you can also run `bash scripts/install.sh` from this
checkout. The script belongs to this repository. The future public `/install-cli` entry point will
redirect to its raw GitHub URL once the CLI mirror and public distribution are configured. During
local evaluation, piping the script requires `BRAIN_CLI_SOURCE_DIR` to point at an existing CLI
checkout:

```sh
cat scripts/install.sh | BRAIN_CLI_SOURCE_DIR="$PWD" bash
```

Start a Second Brain service with CLI support. In the full Second Brain checkout, run `mise run dev`
from its root, following its existing local service setup. The CLI defaults to
`http://second-brain.localhost:1355`. Use `--origin https://your-service.example` or `BRAIN_ORIGIN`
to select another compatible service. This preview's backend adapter has not been deployed publicly.

```sh
brain login
brain brains list
brain --project /path/to/your/project use org_1234567890abcdef
brain --project /path/to/your/project whoami
brain --project /path/to/your/project search "release decisions"
```

Use an ID returned by `brains list`. Login opens email sign-in and browser consent. It requests the
existing knowledge, Brain, account, and management permissions; the browser shows the consent and
the server checks your current role on each operation. For read-only use:

```sh
brain login --scopes "knowledge:read brains:access"
```

For an agent terminal, `brain login --no-browser` prints an authorization URL to stderr and waits
for the browser callback. Keep that process alive. Credentials are stored privately by the CLI;
there is no token copy/paste step. See [local testing](docs/testing.md) for setup and test
boundaries.

## Add the skill

The agent skills live in an independent Brain skills repository with its own installer. From a
checkout of that repository, install into your evaluation project:

```sh
mise trust
mise install
mise run install -- --project /path/to/your/project
```

This copies its `brain/` skill to the project's `.agents/skills/brain` and adds a short `AGENTS.md`
pointer while retaining existing instructions. Reinstall with `--replace` after reviewing a skill
change. The CLI repository builds and installs independently of that checkout.

Codex discovers `.agents/skills`. Current Claude Code can follow the `AGENTS.md` pointer with its
built-in AGENTS support enabled. The source package needs no `.claude` folder. A Claude setup that
loads only `CLAUDE.md` can import `AGENTS.md`, or load `.agents/skills/brain/SKILL.md` explicitly.
See the skills repository's README for supported environments.

Make this CLI checkout's `.local/bin` available on the agent process's PATH, or give it the absolute
path to `brain`. Then ask:

> Use the Brain CLI to find our release decisions and cite the sources.

> Create a Brain called Field Notes, select it for this project, and help me onboard it.

> Remember that launch approval belongs to the product lead.

## Project selection

Each project chooses a Brain in `.brain/config.toml`:

```toml
brain_id = "org_1234567890abcdef"
```

`brain use <id>` verifies access, atomically writes the file, reads it back, and verifies again. A
failure restores the previous selection. `brain config` shows the local configuration.

- `--project` chooses the directory; the default is the current directory. Parent directories are
  not searched, so a neighboring project cannot silently select a Brain.
- `--brain <id>` overrides the selection for one command without changing the file.
- Login selects the account's only Brain when the project has no configuration and access verifies
  successfully. Use `brain login --no-select` to opt out. Existing selections are preserved; zero or
  multiple memberships require a choice with `brain use`. An explicit `--brain` on login also leaves
  project configuration unchanged.
- Login reports its selection outcome in `data.selection`. A discovery or verification failure
  leaves you signed in and reports `selected: false` with an error; select a Brain later with
  `brain use`. Existing invalid TOML or JSON-only configuration is preserved for explicit recovery.
- Creation returns a new Brain but leaves selection unchanged. Run `brain use <new-id>` after
  creation succeeds. This keeps creation retries attached to their original request context.
- JSON selection is deferred. If only `.brain/config.json` exists, select its intended Brain with
  `brain use`; the JSON file is preserved. When both exist, this CLI uses TOML and MCP uses JSON.
  Keep them aligned intentionally while evaluating both transports.

Account credentials live in `$BRAIN_HOME`, `$XDG_CONFIG_HOME/second-brain`, or
`~/.config/second-brain`, in that order. Set `BRAIN_HOME` to a private directory outside projects
for isolated testing. Credentials are separated by normalized service origin, written atomically
with mode `0600` on Unix, and protected by a lock during refresh. `brain logout` revokes the
connection before removing the local file. It does not sign the browser out.

## Knowledge

```sh
brain knowledge list
brain search "pricing approval" --limit 20
brain read pricing.md --start-line 1 --limit 200
brain knowledge grep "approved" --path decisions
brain knowledge document ORGANIZATION.md
brain knowledge show
brain request-id
brain record --file facts.md --request-id chosen-stable-id
brain record --file - --request-id another-stable-id < facts.md
brain record --file facts.md --attachment notes.csv --request-id import-stable-id
brain runs get run_1234567890abcdef --wait 25
```

Results include revisions, pagination, and source URLs. Pin related reads with `--revision <sha>`.
Search is lexical. Follow `nextOffset` and `nextLine`, and inspect per-file errors before concluding
that a result is complete. `knowledge read-many --file ranges.json` accepts up to eight
`{"path":"file.md","startLine":1,"limit":200}` objects in an array.

Recording invokes the service's writing model, which selects files and commits validated knowledge.
It waits up to 25 seconds by default. Only `status: saved` confirms a commit. Continue a running
result with `runs get`, using the same Brain ID. Preserve the request ID, original Brain, content,
and attachment bytes on retries. `not_saved`, failed, and cancelled runs return a nonzero exit.
`--wait 0` returns the submitted run immediately.

For exact requested edits, `knowledge replace --file change.json` accepts `content`, `baseRevision`,
and optional `documents` and `summary`. Read current content first and merge after a revision
conflict. Normal fact recording needs no manual placement or preflight read. Attachments are
processed transiently; the service retains extracted knowledge and source receipts. PDF and Office
extraction remains deferred.

## People and access

```sh
brain people list
brain people invite colleague@example.com --role read
brain people role usr_1234567890abcdef --role write
brain people revoke inv_1234567890abcdef
brain people transfer usr_1234567890abcdef --expected-owner usr_fedcba0987654321
brain repository access list
brain repository access create "Local checkout" --secret-file /private/path/git-access.json
brain repository access revoke access_1234567890abcdef
brain repository retry
brain connections list
brain account show
brain account profile "Your name"
brain connections disconnect ogr_1234567890abcdef
brain account disconnect ogr_1234567890abcdef
brain runs cancel run_1234567890abcdef
```

Invitations default to Read only and send email outside local development. For a local invitation,
add `--secret-file /private/path/invitation.json` to preserve its one-time acceptance link. Without
that option the CLI discards the development link. One-time links and Git tokens never appear on
stdout. Secret destinations must be new absolute paths outside the selected project; their parent
directories must already exist. Only the current Owner can transfer ownership. Operator suspension,
limits, deletion, and recovery remain in the private administration console.

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

[Usage](https://github.com/jdx/usage) is the source for commands, arguments, help, and completions:

```sh
brain __usage_spec__ > /tmp/brain.usage.kdl
mise exec -- usage generate json --file /tmp/brain.usage.kdl
brain completions zsh
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
tooling. Rust tests run the compiled binary against a synthetic HTTP/OAuth service; the Second Brain
service has separate real OAuth, role, and recording integration tests. See
[architecture](docs/architecture.md) and [local testing](docs/testing.md).

The design takes inspiration from [Basecamp for agents](https://basecamp.com/agents) and
[Basecamp's skills](https://github.com/basecamp/skills), adapted to verified Brain selection,
durable knowledge recording, and Second Brain's current access model.
