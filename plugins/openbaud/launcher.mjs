#!/usr/bin/env node

import { constants } from "node:fs";
import { access } from "node:fs/promises";
import { spawn } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const RUNTIMES = new Map([
  ["darwin/arm64", "bin/darwin-arm64/openbaud"],
  ["darwin/x64", "bin/darwin-x64/openbaud"],
  ["linux/arm64", "bin/linux-arm64/openbaud"],
  ["linux/x64", "bin/linux-x64/openbaud"],
  ["win32/x64", "bin/windows-x64/openbaud.exe"],
]);

export function binaryRelativePath(platform = process.platform, arch = process.arch) {
  const host = `${platform}/${arch}`;
  const relativePath = RUNTIMES.get(host);
  if (!relativePath) {
    throw new Error(
      `OpenBaud could not start: no bundled runtime for ${host}. ` +
        "Install a supported OpenBaud plugin build.",
    );
  }
  return relativePath;
}

export async function launch(args = process.argv.slice(2)) {
  const pluginRoot = dirname(fileURLToPath(import.meta.url));
  const runtime = resolve(pluginRoot, binaryRelativePath());
  const accessMode = process.platform === "win32" ? constants.F_OK : constants.X_OK;

  try {
    await access(runtime, accessMode);
  } catch {
    throw new Error(
      `OpenBaud could not start: bundled runtime is missing or not executable: ${runtime}. ` +
        "Reinstall the plugin from the stable marketplace channel.",
    );
  }

  const child = spawn(runtime, args, {
    env: process.env,
    stdio: "inherit",
    windowsHide: true,
  });

  const signals = process.platform === "win32"
    ? ["SIGINT"]
    : ["SIGINT", "SIGTERM", "SIGHUP"];
  const signalHandlers = new Map(
    signals.map((signal) => [
      signal,
      () => {
        if (!child.killed) {
          child.kill(signal);
        }
      },
    ]),
  );
  for (const signal of signals) {
    process.on(signal, signalHandlers.get(signal));
  }

  try {
    const result = await new Promise((resolveExit, rejectExit) => {
      child.once("error", rejectExit);
      child.once("exit", (code, signal) => resolveExit({ code, signal }));
    });
    return result.signal ? 1 : (result.code ?? 1);
  } finally {
    for (const signal of signals) {
      process.off(signal, signalHandlers.get(signal));
    }
  }
}

const entrypoint = process.argv[1]
  ? pathToFileURL(resolve(process.argv[1])).href === import.meta.url
  : false;

if (entrypoint) {
  try {
    process.exitCode = await launch();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 127;
  }
}
