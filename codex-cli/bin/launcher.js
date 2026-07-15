const TARGET_BY_PLATFORM_ARCH = {
  "android-arm64": "aarch64-unknown-linux-musl",
  "android-x64": "x86_64-unknown-linux-musl",
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-arm64": "aarch64-unknown-linux-musl",
  "linux-x64": "x86_64-unknown-linux-musl",
  "win32-arm64": "aarch64-pc-windows-msvc",
  "win32-x64": "x86_64-pc-windows-msvc",
};

const PACKAGE_SUFFIX_BY_TARGET = {
  "aarch64-apple-darwin": "darwin-arm64",
  "aarch64-pc-windows-msvc": "win32-arm64",
  "aarch64-unknown-linux-musl": "linux-arm64",
  "x86_64-apple-darwin": "darwin-x64",
  "x86_64-pc-windows-msvc": "win32-x64",
  "x86_64-unknown-linux-musl": "linux-x64",
};

export function targetTripleForPlatform(platform, arch) {
  const targetTriple = TARGET_BY_PLATFORM_ARCH[`${platform}-${arch}`];
  if (!targetTriple) {
    throw new Error(`Unsupported platform: ${platform} (${arch})`);
  }
  return targetTriple;
}

export function platformPackageForTarget(packageJson, targetTriple) {
  const suffix = PACKAGE_SUFFIX_BY_TARGET[targetTriple];
  if (!suffix) {
    throw new Error(`Unsupported target triple: ${targetTriple}`);
  }

  const platformPackage = `${packageJson.name}-${suffix}`;
  const optionalDependencies = packageJson.optionalDependencies ?? {};
  const supportedTargets = Object.keys(PACKAGE_SUFFIX_BY_TARGET).filter(
    (target) =>
      `${packageJson.name}-${PACKAGE_SUFFIX_BY_TARGET[target]}` in
      optionalDependencies,
  );
  if (supportedTargets.length && !supportedTargets.includes(targetTriple)) {
    throw new Error(
      `${packageJson.name} does not support target ${targetTriple}. ` +
        `Supported targets: ${supportedTargets.join(", ")}.`,
    );
  }
  return platformPackage;
}

export function missingDependencyError(platformPackage, manager, packageName) {
  const command =
    manager === "bun"
      ? `bun install -g ${packageName}@latest`
      : manager === "pnpm"
        ? `pnpm add -g ${packageName}@latest`
        : `npm install -g ${packageName}@latest`;
  return (
    `Missing optional dependency ${platformPackage}. Reinstall the CLI: ` +
    command
  );
}
