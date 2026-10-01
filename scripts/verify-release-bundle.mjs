import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

function files(path) { return existsSync(path) ? readdirSync(path, { withFileTypes: true }).flatMap((entry) => entry.isDirectory() ? [entry.name, ...files(join(path, entry.name)).map((name) => join(entry.name, name))] : [entry.name]) : []; }
export function verifyRelease(platform,root="src-tauri/target/release/bundle",project=".") {
  const expected = { win32: [".msi", ".exe"], darwin: [".app", ".dmg"], linux: [".AppImage", ".deb"] }[platform];
  if (!expected) throw new Error(`unsupported platform ${platform}`);
  const built=files(root);
  const packages=built.filter(file=>expected.some(extension=>file.endsWith(extension)));
  for(const extension of expected) if(!packages.some(file=>file.endsWith(extension))) throw new Error(`missing required ${extension} bundle`);
  const config=JSON.parse(readFileSync(join(project,"src-tauri/tauri.conf.json"),"utf8"));
  const npm=JSON.parse(readFileSync(join(project,"package.json"),"utf8"));
  const cargo=readFileSync(join(project,"src-tauri/Cargo.toml"),"utf8").match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if(config.productName!=="Tenjee Vault"||config.version!=="1.0.0"||config.identifier!=="com.sam.tenjee-vault"||npm.version!==config.version||cargo!==config.version) throw new Error("release metadata is inconsistent");
  for(const file of packages.filter(file=>!file.endsWith('.app')&&!file.includes('.app/'))) if(!file.includes(config.version)) throw new Error(`bundle name lacks version: ${file}`);
  for(const file of files(join(project,"dist")).filter(file=>/\.(html|js)$/.test(file))) if(config.build?.devUrl&&readFileSync(join(project,"dist",file),"utf8").includes(config.build.devUrl)) throw new Error("release frontend references development server");
  return {platform,product:config.productName,version:config.version,identifier:config.identifier,bundles:packages};
}
if(process.argv[1] && import.meta.url===pathToFileURL(resolve(process.argv[1])).href) console.log(JSON.stringify(verifyRelease(process.argv[2],process.argv[3])));
