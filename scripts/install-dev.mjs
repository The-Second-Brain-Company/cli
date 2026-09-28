import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const source = dirname(dirname(fileURLToPath(import.meta.url)));
if (process.argv.length !== 2) {
  process.stderr.write(
    "Usage: mise run install (set BIN_DIR to override the installation directory)\n",
  );
  process.exit(2);
}
const build = spawnSync(
  "cargo",
  ["build", "--release", "--locked", "--bin", "brain", "--message-format=json-render-diagnostics"],
  {
    cwd: source,
    encoding: "utf8",
    stdio: ["inherit", "pipe", "inherit"],
    maxBuffer: 32 * 1024 * 1024,
  },
);
if (build.error) {
  process.stderr.write(`Could not build Brain: ${build.error.message}\n`);
  process.exit(1);
}
if (build.status !== 0) process.exit(build.status ?? 1);
const artifact = build.stdout
  .split("\n")
  .filter(Boolean)
  .map((line) => JSON.parse(line))
  .find(
    (event) =>
      event.reason === "compiler-artifact" &&
      event.target?.name === "brain" &&
      event.target.kind.includes("bin") &&
      event.executable,
  );
if (!artifact) {
  process.stderr.write(
    "Cargo did not report a Brain executable; the installed version was preserved.\n",
  );
  process.exit(1);
}
const install = spawnSync(
  "bash",
  [join(source, "scripts/install.sh"), "--from-file", artifact.executable],
  {
    stdio: "inherit",
  },
);
if (install.error) process.stderr.write(`Could not install Brain: ${install.error.message}\n`);
process.exit(install.status ?? 1);
