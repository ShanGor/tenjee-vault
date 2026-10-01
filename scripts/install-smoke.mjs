// Runs only on disposable native CI runners. Every application launch uses a fresh,
// isolated data directory via benchmark-release.mjs; uninstall must preserve that data.
import { mkdtempSync,mkdirSync,readdirSync,existsSync,readFileSync,writeFileSync,cpSync,rmSync } from "node:fs";
import { join,resolve,basename } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { verifyRelease } from "./verify-release-bundle.mjs";
if (!process.env.CI) throw new Error("Installer smoke checks require a disposable CI runner");
const root=resolve("src-tauri/target/release/bundle");
const release=verifyRelease(process.platform,root);
const staging=mkdtempSync(join(tmpdir(),"tenjee-install-smoke-"));
mkdirSync("artifacts",{recursive:true});
const reports=[];
function run(command,args,options={}) {
  const result=spawnSync(command,args,{encoding:"utf8",timeout:180000,...options});
  if(result.error||result.status!==0) throw new Error(`${command} failed: ${result.error ?? result.stdout+result.stderr}`);
  return result.stdout;
}
function find(directory,predicate) {
  for(const entry of readdirSync(directory,{withFileTypes:true})) {
    const path=join(directory,entry.name);
    if(predicate(path)) return path;
    if(entry.isDirectory()) { const nested=find(path,predicate); if(nested)return nested; }
  }
}
function benchmark(executable,format) {
  run(process.execPath,["scripts/benchmark-release.mjs",executable]);
  const report=JSON.parse(readFileSync("artifacts/benchmark.json","utf8"));
  writeFileSync(`artifacts/benchmark-${format}.json`,JSON.stringify(report,null,2));
  reports.push({format,...report});
  return report.probe_directory;
}
function retained(directory) {
  if(!existsSync(join(directory,"tenjee-vault","meta.db"))) throw new Error("Uninstall removed application data");
}
if(process.platform==="linux") {
  const deb=join(root,release.bundles.find(file=>file.endsWith(".deb")));
  run("sudo",["dpkg","-i",deb]);
  const data=benchmark("/usr/bin/tenjee-vault","deb");
  run("sudo",["dpkg","-r","tenjee-vault"]); retained(data);
  const appimage=join(root,release.bundles.find(file=>file.endsWith(".AppImage")));
  run(appimage,["--appimage-extract"],{cwd:staging});
  const unpacked=join(staging,"squashfs-root");
  const imageData=benchmark(join(unpacked,"AppRun"),"appimage");
  rmSync(unpacked,{recursive:true}); retained(imageData);
} else if(process.platform==="darwin") {
  const app=join(root,release.bundles.find(file=>file.endsWith(".app")));
  const installed=join(staging,"Tenjee Vault.app");
  cpSync(app,installed,{recursive:true});
  const data=benchmark(join(installed,"Contents/MacOS/tenjee-vault"),"app");
  rmSync(installed,{recursive:true});retained(data);
  const mount=join(staging,"mounted");mkdirSync(mount);
  run("hdiutil",["attach",join(root,release.bundles.find(file=>file.endsWith(".dmg"))),"-nobrowse","-readonly","-mountpoint",mount]);
  try { cpSync(find(mount,path=>path.endsWith(".app")),installed,{recursive:true}); }
  finally { run("hdiutil",["detach",mount]); }
  const dmgData=benchmark(join(installed,"Contents/MacOS/tenjee-vault"),"dmg");
  rmSync(installed,{recursive:true});retained(dmgData);
} else if(process.platform==="win32") {
  for(const extension of [".msi",".exe"]) {
    const installer=join(root,release.bundles.find(file=>file.endsWith(extension)));
    const installed=join(staging,extension===".msi"?"msi":"nsis");
    if(extension===".msi")run("msiexec.exe",["/i",installer,"/qn","/norestart",`INSTALLDIR=${installed}`]);
    else run(installer,["/S",`/D=${installed}`]);
    const binary=find(installed,path=>basename(path).toLowerCase()==="tenjee-vault.exe");
    if(!binary)throw new Error(`Installed executable missing: ${installed}`);
    const data=benchmark(binary,extension===".msi"?"msi":"nsis");
    if(extension===".msi")run("msiexec.exe",["/x",installer,"/qn","/norestart"]);
    else {
      const uninstaller=find(installed,path=>/uninstall.*\.exe$/i.test(basename(path)));
      if(!uninstaller)throw new Error("NSIS uninstaller missing");
      run(uninstaller,["/S"]);
    }
    retained(data);
  }
} else throw new Error(`Unsupported platform: ${process.platform}`);
writeFileSync("artifacts/install-smoke.json",JSON.stringify({release,reports,passed:true},null,2));
