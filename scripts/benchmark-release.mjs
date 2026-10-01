import { mkdtempSync, existsSync, readFileSync, writeFileSync, unlinkSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { performance } from "node:perf_hooks";
import { checkThresholds } from "./benchmark-thresholds.mjs";

const executable=resolve(process.argv[2] ?? `src-tauri/target/release/tenjee-vault${process.platform==='win32'?'.exe':''}`);
const searchExecutable=resolve(process.argv[3] ?? `src-tauri/target/release/examples/search_bench${process.platform==='win32'?'.exe':''}`);
const directory=mkdtempSync(join(tmpdir(),"tenjee-release-probe-"));
async function launch() {
  const marker=join(directory,"ready.json");
  if(existsSync(marker)) unlinkSync(marker);
  const start=performance.now();
  const child=spawn(executable,[],{env:{...process.env,TENJEE_RELEASE_PROBE_DIR:directory},stdio:["ignore","pipe","pipe"]});
  let logs="";
  child.stdout.on("data",data=>logs+=data); child.stderr.on("data",data=>logs+=data);
  return new Promise((resolve,reject)=>{
    let readyAt=null;
    const interval=setInterval(()=>{ if(readyAt===null&&existsSync(marker)) readyAt=performance.now()-start; },5);
    const timeout=setTimeout(()=>{child.kill();},15000);
    child.on("error",error=>{clearInterval(interval);clearTimeout(timeout);reject(error);});
    child.on("exit",code=>{
      clearInterval(interval);clearTimeout(timeout);
      if(code!==0 || !existsSync(marker)) return reject(new Error(`Startup probe failed (${code}): ${logs}`));
      const result=JSON.parse(readFileSync(marker,"utf8"));
      if(result.version!=="1.0.0"||result.alerts!==0) return reject(new Error("Startup probe reported invalid metadata or database alerts"));
      resolve(readyAt ?? performance.now()-start);
    });
  });
}
await launch(); // First launch initializes the isolated data set; it is not timed.
const samples=[];
for(let index=0;index<3;index++) samples.push(await launch());
const search=spawnSync(searchExecutable,[],{encoding:"utf8",timeout:120000});
if(search.error) throw search.error;
const metrics=JSON.parse(search.stdout.trim().split('\n').at(-1));
const report={platform:process.platform,architecture:process.arch,cold_start_ms:Math.max(...samples),cold_start_samples_ms:samples,search_ms:metrics.maximum_ms,search:metrics,probe_directory:directory};
mkdirSync("artifacts",{recursive:true});
writeFileSync("artifacts/benchmark.json",JSON.stringify(report,null,2));
if(search.status!==0) throw new Error(`Search benchmark failed: ${search.stderr}`);
console.log(JSON.stringify(checkThresholds(report)));
