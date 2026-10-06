import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const installer = fileURLToPath(new URL("./install.sh", import.meta.url));
const installerText = await readFile(installer, "utf8");
const latestUrl =
  "https://github.com/The-Second-Brain-Company/cli/releases/latest/download/latest.txt";

async function temporaryRoot(t) {
  const root = await mkdtemp(join(tmpdir(), "cortex-install-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  return root;
}

async function binary(path, version) {
  await writeFile(path, `#!/bin/sh\nprintf '%s\\n' 'cortex ${version}'\n`, { mode: 0o755 });
}

function install(source, destination) {
  return spawnSync("bash", [installer, "--from-file", source], {
    env: { ...process.env, BIN_DIR: destination },
    encoding: "utf8",
  });
}

async function releaseFixture(
  t,
  {
    version = "0.2.1",
    latest = version,
    system = "Darwin",
    arch = "arm64",
    platform = "aarch64-apple-darwin",
  } = {},
) {
  const root = await temporaryRoot(t);
  const tools = join(root, "tools");
  const destination = join(root, "installed");
  const downloads = join(root, "downloads");
  const requests = join(root, "requests");
  const artifact = join(root, "artifact");
  const binaryUrl = `https://github.com/The-Second-Brain-Company/cli/releases/download/v${version}/cortex-${platform}`;
  await mkdir(tools);
  await mkdir(downloads);
  await writeFile(requests, "");
  await binary(artifact, version);
  await writeFile(
    join(tools, "uname"),
    '#!/bin/sh\nif [ "$1" = -s ]; then printf "%s" "$RELEASE_SYSTEM"; else printf "%s" "$RELEASE_ARCH"; fi\n',
    { mode: 0o755 },
  );
  await writeFile(
    join(tools, "curl"),
    `#!/bin/sh
set -eu
url=""
output=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) shift; output="$1";;
    --*) ;;
    *) url="$1";;
  esac
  shift
done
printf '%s\\n' "$url" >> "$RELEASE_REQUESTS"
[ "$url" != "\${FAIL_URL:-}" ] || exit 22
case "$url" in
  "$LATEST_URL") printf '%s\\n' "$RELEASE_LATEST";;
  "$BINARY_URL") cp "$RELEASE_BINARY" "$output";;
  "$BINARY_URL.sha256") printf '%s  %s\\n' "$RELEASE_DIGEST" "\${RELEASE_CHECKSUM_NAME:-\${BINARY_URL##*/}}" > "$output";;
  *) exit 23;;
esac
`,
    { mode: 0o755 },
  );
  const env = {
    ...process.env,
    PATH: `${tools}:${process.env.PATH}`,
    BIN_DIR: destination,
    TMPDIR: downloads,
    LATEST_URL: latestUrl,
    BINARY_URL: binaryUrl,
    RELEASE_BINARY: artifact,
    RELEASE_DIGEST: createHash("sha256")
      .update(await readFile(artifact))
      .digest("hex"),
    RELEASE_LATEST: latest,
    RELEASE_SYSTEM: system,
    RELEASE_ARCH: arch,
    RELEASE_REQUESTS: requests,
    CORTEX_CLI_SOURCE_DIR: join(root, "missing-source"),
  };
  delete env.CORTEX_VERSION;
  return {
    root,
    tools,
    destination,
    downloads,
    artifact,
    binaryUrl,
    requests: async () => (await readFile(requests, "utf8")).split("\n").filter(Boolean),
    run: (args = [], { stdin = false, ...overrides } = {}) =>
      spawnSync("bash", stdin ? ["-s", "--", ...args] : [installer, ...args], {
        env: { ...env, ...overrides },
        input: stdin ? installerText : undefined,
        encoding: "utf8",
      }),
  };
}

test("default and explicit release installs work from a saved file and stdin", async (t) => {
  for (const stdin of [false, true]) {
    for (const args of [[], ["--release"]]) {
      await t.test(`${stdin ? "stdin" : "saved"} ${args[0] ?? "default"}`, async (t) => {
        const fixture = await releaseFixture(t);
        const result = fixture.run(args, { stdin });
        assert.equal(result.status, 0, result.stderr);
        assert.deepEqual(await fixture.requests(), [
          latestUrl,
          fixture.binaryUrl,
          `${fixture.binaryUrl}.sha256`,
        ]);
        assert.equal(
          spawnSync(join(fixture.destination, "cortex"), ["--version"], { encoding: "utf8" })
            .stdout,
          "cortex 0.2.1\n",
        );
        assert.deepEqual(await readdir(fixture.destination), ["cortex"]);
        assert.deepEqual(await readdir(fixture.downloads), []);
      });
    }
  }
});

test("release downloads match every supported OS and architecture", async (t) => {
  for (const [system, arch, platform] of [
    ["Darwin", "arm64", "aarch64-apple-darwin"],
    ["Darwin", "x86_64", "x86_64-apple-darwin"],
    ["Linux", "aarch64", "aarch64-unknown-linux-gnu"],
    ["Linux", "x86_64", "x86_64-unknown-linux-gnu"],
    ["Linux", "arm64", "aarch64-unknown-linux-gnu"],
    ["Linux", "amd64", "x86_64-unknown-linux-gnu"],
  ]) {
    await t.test(`${system} ${arch}`, async (t) => {
      const fixture = await releaseFixture(t, { system, arch, platform });
      const result = fixture.run();
      assert.equal(result.status, 0, result.stderr);
      assert.deepEqual(await fixture.requests(), [
        latestUrl,
        fixture.binaryUrl,
        `${fixture.binaryUrl}.sha256`,
      ]);
    });
  }
});

test("CORTEX_VERSION installs that version without fetching latest.txt", async (t) => {
  const version = "1.2.3-rc.1+build.5";
  const fixture = await releaseFixture(t, { version, latest: "9.9.9" });
  const result = fixture.run([], { stdin: true, CORTEX_VERSION: version });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(await fixture.requests(), [fixture.binaryUrl, `${fixture.binaryUrl}.sha256`]);
  assert.equal(
    spawnSync(join(fixture.destination, "cortex"), ["--version"], { encoding: "utf8" }).stdout,
    `cortex ${version}\n`,
  );
});

test("invalid pinned or advertised versions never request a release artifact", async (t) => {
  for (const version of [
    "",
    "v1.2.3",
    "01.2.3",
    "1.2",
    "1.2.3-01",
    "1.2.3-",
    "1.2.3+",
    "../1.2.3",
    "1.2.3/file",
    "1.2.3\n4.5.6",
    "1.2.3 ",
  ]) {
    await t.test(JSON.stringify(version), async (t) => {
      const fixture = await releaseFixture(t, { latest: version });
      assert.notEqual(fixture.run([], { CORTEX_VERSION: version }).status, 0);
      assert.deepEqual(await fixture.requests(), []);
      assert.notEqual(fixture.run().status, 0);
      assert.deepEqual(await fixture.requests(), [latestUrl]);
      assert.deepEqual(await readdir(fixture.downloads), []);
    });
  }
});

test("checksum, executable version, and download failures preserve the installed binary", async (t) => {
  for (const failure of [
    "checksum",
    "checksum filename",
    "version",
    "latest",
    "binary",
    "manifest",
  ]) {
    await t.test(failure, async (t) => {
      const fixture = await releaseFixture(t);
      await mkdir(fixture.destination);
      const installed = join(fixture.destination, "cortex");
      await binary(installed, "0.1.0");
      const original = await readFile(installed);
      const overrides = {};
      if (failure === "checksum") overrides.RELEASE_DIGEST = "0".repeat(64);
      if (failure === "checksum filename") {
        overrides.RELEASE_CHECKSUM_NAME = "/bin/sh";
        overrides.RELEASE_DIGEST = createHash("sha256")
          .update(await readFile("/bin/sh"))
          .digest("hex");
      }
      if (failure === "version") {
        await binary(fixture.artifact, "0.2.0");
        overrides.RELEASE_DIGEST = createHash("sha256")
          .update(await readFile(fixture.artifact))
          .digest("hex");
      }
      if (failure === "latest") overrides.FAIL_URL = latestUrl;
      if (failure === "binary") overrides.FAIL_URL = fixture.binaryUrl;
      if (failure === "manifest") overrides.FAIL_URL = `${fixture.binaryUrl}.sha256`;
      assert.notEqual(fixture.run([], overrides).status, 0);
      assert.deepEqual(await readFile(installed), original);
      assert.deepEqual(await readdir(fixture.destination), ["cortex"]);
      assert.deepEqual(await readdir(fixture.downloads), []);
    });
  }
});

test("source installation delegates to the pinned mise task only with explicit source mode", async (t) => {
  const fixture = await releaseFixture(t);
  const source = join(fixture.root, "source with spaces");
  const log = join(fixture.root, "mise-arguments");
  await mkdir(source);
  await writeFile(join(source, "Cargo.toml"), "");
  await writeFile(join(source, "mise.toml"), "");
  await writeFile(join(fixture.tools, "mise"), '#!/bin/sh\nprintf "%s\\n" "$@" > "$MISE_LOG"\n', {
    mode: 0o755,
  });
  const result = fixture.run(["--source"], {
    stdin: true,
    CORTEX_CLI_SOURCE_DIR: source,
    MISE_LOG: log,
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(await readFile(log, "utf8"), `-C\n${source}\nrun\ninstall\n`);
  assert.deepEqual(await fixture.requests(), []);
  const local = fixture.run(["--source"], { CORTEX_CLI_SOURCE_DIR: "", MISE_LOG: log });
  assert.equal(local.status, 0, local.stderr);
  assert.equal(await readFile(log, "utf8"), `-C\n${dirname(dirname(installer))}\nrun\ninstall\n`);
  assert.notEqual(fixture.run(["--source"], { stdin: true, CORTEX_CLI_SOURCE_DIR: "" }).status, 0);
});

test("unknown arguments and unsupported platforms do not download or install", async (t) => {
  const fixture = await releaseFixture(t, { system: "FreeBSD", arch: "x86_64" });
  for (const args of [
    ["--unknown"],
    ["--from-file"],
    ["--release", "extra"],
    ["--source", "extra"],
    ["--help", "extra"],
  ]) {
    assert.equal(fixture.run(args).status, 2);
  }
  assert.notEqual(fixture.run().status, 0);
  assert.deepEqual(await fixture.requests(), []);
  assert.deepEqual(await readdir(fixture.downloads), []);
});

test("local and release artifacts replace each other at the same executable path", async (t) => {
  const root = await temporaryRoot(t);
  const destination = join(root, "bin with spaces");
  const dev = join(root, "local build");
  const release = join(root, "release build");
  await binary(dev, "0.1.0-dev");
  await binary(release, "0.1.0");
  for (const source of [dev, release, dev]) {
    const result = install(source, destination);
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(await readFile(join(destination, "cortex")), await readFile(source));
    assert.equal((await lstat(join(destination, "cortex"))).mode & 0o777, 0o755);
    assert.deepEqual(await readdir(destination), ["cortex"]);
  }
});

test("an unusable replacement preserves the installed binary and cleans up temporary files", async (t) => {
  const root = await temporaryRoot(t);
  const destination = join(root, "bin");
  const good = join(root, "good");
  const bad = join(root, "bad");
  await binary(good, "0.1.0");
  assert.equal(install(good, destination).status, 0);
  const original = await readFile(join(destination, "cortex"));
  for (const content of [
    "#!/bin/sh\nexit 1\n",
    "#!/bin/sh\nprintf '%s\\n' 'unrelated program'\n",
  ]) {
    await writeFile(bad, content, { mode: 0o755 });
    assert.notEqual(install(bad, destination).status, 0);
    assert.deepEqual(await readFile(join(destination, "cortex")), original);
    assert.deepEqual(await readdir(destination), ["cortex"]);
  }
  assert.notEqual(install(join(root, "missing"), destination).status, 0);
  assert.deepEqual(await readFile(join(destination, "cortex")), original);
});

test("replacement replaces an old executable symlink without changing its target", async (t) => {
  const root = await temporaryRoot(t);
  const destination = join(root, "bin");
  await mkdir(destination);
  const old = join(root, "old");
  const next = join(root, "next");
  await binary(old, "0.0.1");
  await binary(next, "0.1.0");
  const original = await readFile(old);
  await symlink(old, join(destination, "cortex"));
  const result = install(next, destination);
  assert.equal(result.status, 0, result.stderr);
  assert.equal((await lstat(join(destination, "cortex"))).isSymbolicLink(), false);
  assert.deepEqual(await readFile(old), original);
  assert.deepEqual(await readFile(join(destination, "cortex")), await readFile(next));
});

test("a directory at the destination is preserved", async (t) => {
  const root = await temporaryRoot(t);
  const destination = join(root, "bin");
  await mkdir(join(destination, "cortex"), { recursive: true });
  const source = join(root, "artifact");
  await binary(source, "0.1.0");
  assert.notEqual(install(source, destination).status, 0);
  assert.deepEqual(await readdir(join(destination, "cortex")), []);
});
