import { cpSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
export function installAndroidNative(projectDirectory) {
  const source = path.join(projectDirectory, "src-tauri", "android", "app", "src", "main");
  const destination = path.join(projectDirectory, "src-tauri", "gen", "android", "app", "src", "main");
  if (!existsSync(destination)) return false;
  mkdirSync(destination, { recursive: true });
  cpSync(source, destination, { recursive: true });
  return true;
}
