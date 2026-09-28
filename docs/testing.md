# Local evaluation

## Tests without accounts or publishing

From the CLI directory:

```sh
mise install
mise run test
mise run install
mise run fmt-check
```

The compiled-binary tests start an ephemeral loopback fixture. They exercise the browser callback,
PKCE, invalid state, private credential storage, refresh, revocation, JSON output, stdin, sole-Brain
login selection, opt-out, existing and concurrent selection preservation, discovery and verification
failures, failed-switch rollback, request identity, and one-time secret files. Fixtures contain
synthetic identities and knowledge and do not call a model or external storage. Skill installation
is tested independently in the Brain skills repository.

From the full service repository root:

```sh
mise run test-cli-api
```

These workerd tests exercise real OAuth registration, consent, exchange and revocation, current-role
changes, creation retries, retrieval, recording receipts, invitation roles, stale ownership, service
suspension, and existing MCP regression coverage. External storage and model boundaries are
substituted. This is separate from the Rust fixture suite; neither claims a live provider test.

## Try your own local Brain

1. In the service checkout, follow its local development setup for Code Storage and the private
   admin password. Use the tracked `.dev.vars.example` files as the variable reference. Credentials
   belong only in the existing ignored secret inputs.
2. Start `mise run dev` at the service root. Reuse an existing instance when present. Its shared
   origin is `http://second-brain.localhost:1355`.
3. Use `/admin/` to invite a test user if needed. Accept the development invitation link in the
   browser and complete or skip the personal name. Operator login remains separate.
4. In a separate evaluation folder, run the locally installed `brain login`. Complete browser
   sign-in and consent. Local delivery returns a masked link in the browser; no email is sent.
5. Login selects your only Brain for an unconfigured project. Use `brain login --no-select` to opt
   out. Check `brain config`; if nothing is selected, run `brain brains list`, then
   `brain use <id>`. To create a Brain, get an ID with `brain request-id`, run
   `brain brains create "Evaluation" --request-id <id>`, and select the returned Organization ID
   with `brain use`.
6. Install the skill into the evaluation folder, restart or reload the harness's instructions if
   needed, and give it the installed binary's PATH. Ask it to record synthetic facts and retrieve
   them with citations. Inspect People and Connected apps in the browser to compare outcomes.
7. Use `brain logout` when finished. Remove test Brains only through tracked admin cleanup. Do not
   wipe local databases or remove provider repositories manually.

Use a new private directory through `BRAIN_HOME` to isolate CLI credentials during evaluation.
`--origin` must match the running service origin exactly. Login listens on `127.0.0.1`; a remote
browser must be able to reach that loopback callback on the CLI's machine. A network sandbox may
need permission for the listener and service connection.

## Mirror check

Everything needed to build, install, and test the CLI is inside this repository. Copy the repository
without `.local/`, `target/`, or credential/configuration directories, then run the same mise tasks
in the copy. It needs no skills checkout or service source. Authenticated operations still need a
compatible Second Brain service.

The skills repository has its own README, mise tasks, and installer and can be copied and tested
separately. Mirror the two repositories independently. The preview has no public distribution
pipeline; `scripts/install.sh` installs CLI source locally and is the future public redirect target.
