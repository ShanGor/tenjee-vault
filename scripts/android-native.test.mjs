import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { installAndroidNative } from "./android-native.mjs";
test("tracked native services survive clean Android generation and keep generated files", () => {
  const root = mkdtempSync(path.join(tmpdir(), "tenjee-android-native-"));
  try {
    const source = path.join(root, "src-tauri/android/app/src/main/java");
    const generated = path.join(root, "src-tauri/gen/android/app/src/main/java");
    mkdirSync(source, { recursive: true }); writeFileSync(path.join(source, "Native.kt"), "native services");
    assert.equal(installAndroidNative(root), false);
    mkdirSync(generated, { recursive: true }); writeFileSync(path.join(generated, "Generated.kt"), "tauri binding");
    assert.equal(installAndroidNative(root), true);
    assert.equal(readFileSync(path.join(generated, "Native.kt"), "utf8"), "native services");
    assert.equal(readFileSync(path.join(generated, "Generated.kt"), "utf8"), "tauri binding");
  } finally { rmSync(root, { recursive: true }); }
});
