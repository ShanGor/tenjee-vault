import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";

const projectDirectory = fileURLToPath(new URL("../", import.meta.url));
const defaultSdk = process.platform === "darwin"
  ? path.join(homedir(), "Library", "Android", "sdk")
  : process.platform === "win32"
    ? path.join(process.env.LOCALAPPDATA ?? path.join(homedir(), "AppData", "Local"), "Android", "Sdk")
    : path.join(homedir(), "Android", "Sdk");
const sdkDirectory = process.env.ANDROID_HOME ?? process.env.ANDROID_SDK_ROOT ?? defaultSdk;
const ndkDirectory = process.env.NDK_HOME ?? path.join(sdkDirectory, "ndk", "28.2.13676358");

if (!existsSync(sdkDirectory) || !existsSync(path.join(ndkDirectory, "source.properties"))) {
  console.error("Android SDK/NDK not found. Set ANDROID_HOME and NDK_HOME, or install NDK 28.2.13676358 in your SDK. See docs/android-development.md.");
  process.exit(1);
}

const args = process.argv.slice(2);
if (!["init", "dev", "build", "open"].includes(args[0])) {
  console.error("Usage: node scripts/android.mjs <init|dev|build|open> [Tauri options]");
  process.exit(1);
}

const debugApkSource = path.join(projectDirectory, "src-tauri", "gen", "android", "app", "build", "outputs", "apk", "universal", "debug", "app-universal-debug.apk");
if (args[0] === "build" && args.includes("--debug") && args.includes("--apk")) {
  // Gradle's incremental ZIP writer can retain large unused regions when
  // switching CPU targets. Regenerate this output; copied artifacts stay intact.
  rmSync(debugApkSource, { force: true });
}

// Java/Gradle does not read the proxy variables used by npm, curl and the SDK
// manager. Forward an existing unauthenticated proxy without saving it in the
// Android project or changing the user's global Gradle settings.
let gradleOptions = process.env.GRADLE_OPTS ?? "";
let javaOptions = process.env.JAVA_TOOL_OPTIONS ?? "";
const proxySetting = process.env.HTTPS_PROXY ?? process.env.https_proxy;
if (proxySetting && !gradleOptions.includes("proxyHost")) {
  const proxy = new URL(proxySetting);
  if (proxy.protocol === "http:" && !proxy.username && !proxy.password) {
    const proxyPort = proxy.port || "80";
    const proxyOptions = ` -Dhttps.proxyHost=${proxy.hostname} -Dhttps.proxyPort=${proxyPort} -Dhttp.proxyHost=${proxy.hostname} -Dhttp.proxyPort=${proxyPort} -Dhttp.nonProxyHosts="localhost|127.*|[::1]" -Dhttps.protocols=TLSv1.2`;
    gradleOptions += proxyOptions;
    if (!javaOptions.includes("proxyHost")) javaOptions += proxyOptions;
  }
}

const child = spawn(process.execPath, [path.join(projectDirectory, "node_modules", "@tauri-apps", "cli", "tauri.js"), "android", ...args], {
  cwd: projectDirectory,
  stdio: "inherit",
  env: {
    ...process.env,
    ANDROID_HOME: sdkDirectory,
    ANDROID_SDK_ROOT: sdkDirectory,
    NDK_HOME: ndkDirectory,
    GRADLE_OPTS: gradleOptions.trim(),
    ...(javaOptions.trim() ? { JAVA_TOOL_OPTIONS: javaOptions.trim() } : {}),
  },
});
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => child.kill(signal));
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
child.on("exit", (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
  if (code !== 0 || args[0] !== "build" || !args.includes("--debug") || !args.includes("--apk")) return;
  const targetIndex = args.indexOf("--target");
  const target = targetIndex < 0 ? undefined : args[targetIndex + 1];
  if (!["aarch64", "x86_64"].includes(target)) return;
  if (!existsSync(debugApkSource)) return;
  const directory = path.join(projectDirectory, "artifacts", "android");
  mkdirSync(directory, { recursive: true });
  const destination = path.join(directory, `tenjee-vault-debug-${target}.apk`);
  copyFileSync(debugApkSource, destination);
  console.log(`Android debug APK: ${destination}`);
});
