import assert from "node:assert/strict";
import test from "node:test";

import { binaryRelativePath } from "../plugins/openbaud/launcher.mjs";

const supportedHosts = [
  ["darwin", "arm64", "bin/darwin-arm64/openbaud"],
  ["darwin", "x64", "bin/darwin-x64/openbaud"],
  ["linux", "arm64", "bin/linux-arm64/openbaud"],
  ["linux", "x64", "bin/linux-x64/openbaud"],
  ["win32", "x64", "bin/windows-x64/openbaud.exe"],
];

test("maps every release host to its bundled runtime", () => {
  for (const [platform, arch, expected] of supportedHosts) {
    assert.equal(binaryRelativePath(platform, arch), expected);
  }
});

test("rejects hosts without a published runtime", () => {
  assert.throws(
    () => binaryRelativePath("freebsd", "x64"),
    /no bundled runtime for freebsd\/x64/i,
  );
  assert.throws(
    () => binaryRelativePath("win32", "arm64"),
    /no bundled runtime for win32\/arm64/i,
  );
});
