import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { createHash } from "node:crypto";

const installer = fileURLToPath(new URL("./install.sh", import.meta.url));

test("release installation verifies downloaded bytes before replacement", async () => {
  const root = await mkdtemp(join(tmpdir(), "cortex-release-install-"));
  try {
    const bin = join(root, "tools");
    const destination = join(root, "installed");
    await mkdir(bin);
    const artifact = join(root, "artifact");
    await binary(artifact, "0.2.1");
    await writeFile(
      join(bin, "uname"),
      '#!/bin/sh\nif [ "$1" = -s ]; then printf Darwin; else printf arm64; fi\n',
      { mode: 0o755 },
    );
    await writeFile(
      join(bin, "curl"),
      '#!/bin/sh\nfor argument in "$@"; do previous="${current:-}"; current="$argument"; done\ncase "$previous" in --output) destination="$current";; *) exit 1;; esac\ncase "$destination" in *.sha256) cp "$RELEASE_CHECKSUM" "$destination";; *) cp "$RELEASE_BINARY" "$destination";; esac\n',
      { mode: 0o755 },
    );
    const checksum = join(root, "checksum");
    const digest = createHash("sha256")
      .update(await readFile(artifact))
      .digest("hex");
    await writeFile(checksum, `${digest}  cortex-aarch64-apple-darwin\n`);
    const run = () =>
      spawnSync("bash", [installer, "--release"], {
        env: {
          ...process.env,
          PATH: `${bin}:${process.env.PATH}`,
          BIN_DIR: destination,
          RELEASE_BINARY: artifact,
          RELEASE_CHECKSUM: checksum,
        },
        encoding: "utf8",
      });
    assert.equal(run().status, 0);
    const original = await readFile(join(destination, "cortex"));
    await binary(artifact, "0.2.1-tampered");
    assert.notEqual(run().status, 0);
    assert.deepEqual(await readFile(join(destination, "cortex")), original);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

async function binary(path, version) {
  await writeFile(path, `#!/bin/sh\nprintf '%s\\n' 'cortex ${version}'\n`, { mode: 0o755 });
}

function install(source, destination) {
  return spawnSync("bash", [installer, "--from-file", source], {
    env: { ...process.env, BIN_DIR: destination },
    encoding: "utf8",
  });
}

test("local and release artifacts replace each other at the same executable path", async () => {
  const root = await mkdtemp(join(tmpdir(), "cortex-install-"));
  try {
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
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("an unusable replacement preserves the installed binary and cleans up temporary files", async () => {
  const root = await mkdtemp(join(tmpdir(), "cortex-install-"));
  try {
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
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("replacement replaces an old executable symlink without changing its target", async () => {
  const root = await mkdtemp(join(tmpdir(), "cortex-install-"));
  try {
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
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("a directory at the destination is preserved", async () => {
  const root = await mkdtemp(join(tmpdir(), "cortex-install-"));
  try {
    const destination = join(root, "bin");
    await mkdir(join(destination, "cortex"), { recursive: true });
    const source = join(root, "artifact");
    await binary(source, "0.1.0");
    assert.notEqual(install(source, destination).status, 0);
    assert.deepEqual(await readdir(join(destination, "cortex")), []);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
