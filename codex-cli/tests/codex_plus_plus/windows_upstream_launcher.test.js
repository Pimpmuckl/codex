import assert from "node:assert/strict";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { stageWindowsUpstreamExecutable } from "../../bin/codex_plus_plus/windows_upstream_launcher.js";

test("leaves every invocation except exact Windows update upstream unchanged", () => {
  const binaryPath = "missing-codex.exe";
  const cases = [
    ["linux", ["update", "upstream"]],
    ["win32", ["update"]],
    ["win32", ["update", "upstream", "--help"]],
    ["win32", ["update", "fork"]],
  ];

  for (const [platform, args] of cases) {
    assert.deepEqual(
      stageWindowsUpstreamExecutable(binaryPath, args, platform),
      {
        binaryPath,
      },
    );
  }
});

test("copies the Windows upstream updater bytes and removes its directory", (t) => {
  const fixtureDirectory = mkdtempSync(
    path.join(tmpdir(), "codex-upstream-fixture-"),
  );
  t.after(() => rmSync(fixtureDirectory, { recursive: true, force: true }));

  const sourcePath = path.join(fixtureDirectory, "codex.exe");
  const sourceBytes = Buffer.from([0, 1, 2, 3, 255]);
  writeFileSync(sourcePath, sourceBytes);

  const stagedExecutable = stageWindowsUpstreamExecutable(
    sourcePath,
    ["update", "upstream"],
    "win32",
  );
  const stagedDirectory = path.dirname(stagedExecutable.binaryPath);

  assert.notEqual(stagedExecutable.binaryPath, sourcePath);
  assert.deepEqual(readFileSync(stagedExecutable.binaryPath), sourceBytes);

  stagedExecutable.cleanup();
  assert.equal(existsSync(stagedDirectory), false);
});
