import { spawnSync } from "node:child_process";
import { mkdirSync, statfsSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const option = (name, fallback) => args.includes(name) ? args[args.indexOf(name) + 1] : fallback;
const dataset = option("--dataset", "smoke");
const datasets = { smoke: ["67108864", "1"], large4: ["4294967296", "1"], large8: ["8589934592", "1"], small: ["4096", "10000"] };
if (!datasets[dataset]) throw new Error("Choose smoke, large4, large8, or small");
const storage = path.resolve(option("--storage-dir", os.tmpdir()));
const destination = path.resolve(option("--out", path.join(project, "artifacts", "file-exchange", `${dataset}.json`)));
const run = (command, parameters) => {
  const result = spawnSync(command, parameters, { cwd: project, encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
  if (result.status !== 0) throw new Error(`${command} failed:\n${result.stderr}\n${result.stdout}`);
  return result.stdout;
};
run("cargo", ["build", "--locked", "--release", "--manifest-path", "tools/file-exchange-harness/Cargo.toml", "--bin", "bench"]);
const binary = path.join(project, "tools/file-exchange-harness/target/release", process.platform === "win32" ? "bench.exe" : "bench");
const [bytes, files] = datasets[dataset];
const sample = mode => JSON.parse(run(binary, ["--mode", mode, "--bytes", bytes, "--files", files, "--storage-dir", storage]));
// Sequential independent processes keep dataset generation and RSS measurements isolated.
const baseline = files === "1" ? sample("baseline") : null;
const exchange = sample("files");
const report = {
  recordedAt: new Date().toISOString(), dataset,
  host: { platform: process.platform, release: os.release(), architecture: process.arch, cpu: os.cpus()[0]?.model, filesystemType: statfsSync(storage).type },
  conditions: "Owned pseudorandom test data; both native peers on IPv4 loopback; no RTT/loss shaping; no GUI or Android provider. This does not establish LAN/VPN release acceptance.",
  baseline, exchange,
  relativeSessionThroughput: baseline ? exchange.bytesPerSecond / baseline.bytesPerSecond : null,
  relativePayloadThroughput: baseline ? baseline.sender.phasesSeconds.exchanging / exchange.sender.phasesSeconds.exchanging : null,
};
mkdirSync(path.dirname(destination), { recursive: true });
writeFileSync(destination, JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify({ report: destination, dataset, relativePayloadThroughput: report.relativePayloadThroughput, sessionSeconds: exchange.sessionSeconds, incrementalRssBytes: exchange.incrementalRssBytes }));
