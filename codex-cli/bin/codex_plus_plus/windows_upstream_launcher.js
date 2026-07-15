import { copyFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

export function stageWindowsUpstreamExecutable(
  binaryPath,
  args,
  platform = process.platform,
) {
  if (
    platform !== "win32" ||
    args.length !== 2 ||
    args[0] !== "update" ||
    args[1] !== "upstream"
  ) {
    return { binaryPath };
  }

  const temporaryDirectory = mkdtempSync(
    path.join(tmpdir(), "codex-upstream-"),
  );
  const stagedBinaryPath = path.join(
    temporaryDirectory,
    path.basename(binaryPath),
  );
  const cleanup = () => {
    try {
      rmSync(temporaryDirectory, {
        recursive: true,
        force: true,
        maxRetries: 3,
        retryDelay: 100,
      });
    } catch {
      // Preserve the native child's result if Windows still holds the copy.
    }
  };

  try {
    copyFileSync(binaryPath, stagedBinaryPath);
  } catch (error) {
    cleanup();
    throw error;
  }

  return { binaryPath: stagedBinaryPath, cleanup };
}
