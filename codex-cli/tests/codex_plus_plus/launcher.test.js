import assert from "node:assert/strict";
import test from "node:test";

import {
  missingDependencyError,
  platformPackageForTarget,
  targetTripleForPlatform,
} from "../../bin/launcher.js";

const cases = [
  {
    platform: "linux",
    arch: "x64",
    target: "x86_64-unknown-linux-musl",
    suffix: "linux-x64",
  },
  {
    platform: "darwin",
    arch: "arm64",
    target: "aarch64-apple-darwin",
    suffix: "darwin-arm64",
  },
  {
    platform: "win32",
    arch: "x64",
    target: "x86_64-pc-windows-msvc",
    suffix: "win32-x64",
  },
];

const packageName = "@jjliebig/codex-plus-plus";
const packageJson = {
  name: packageName,
  optionalDependencies: Object.fromEntries(
    cases.map(({ suffix }) => [
      `${packageName}-${suffix}`,
      `npm:${packageName}@0.144.4-fork.1-${suffix}`,
    ]),
  ),
};

test("selects each supported Codex++ package and binary", () => {
  for (const { platform, arch, target, suffix } of cases) {
    assert.equal(targetTripleForPlatform(platform, arch), target);
    assert.equal(
      platformPackageForTarget(packageJson, target),
      `${packageName}-${suffix}`,
    );
  }
});

test("rejects a platform omitted from the fork manifest", () => {
  assert.throws(
    () => platformPackageForTarget(packageJson, "aarch64-unknown-linux-musl"),
    /does not support target aarch64-unknown-linux-musl.*Supported targets:/,
  );
});

test("fork reinstall guidance never switches to upstream", () => {
  const guidance = [
    missingDependencyError("fork-platform", "npm", packageName),
    missingDependencyError("fork-platform", "pnpm", packageName),
    missingDependencyError("fork-platform", "bun", packageName),
  ];
  assert.deepEqual(guidance, [
    "Missing optional dependency fork-platform. Reinstall the CLI: npm install -g @jjliebig/codex-plus-plus@latest",
    "Missing optional dependency fork-platform. Reinstall the CLI: pnpm add -g @jjliebig/codex-plus-plus@latest",
    "Missing optional dependency fork-platform. Reinstall the CLI: bun install -g @jjliebig/codex-plus-plus@latest",
  ]);
  assert.equal(guidance.some((line) => line.includes("@openai/codex")), false);
});
