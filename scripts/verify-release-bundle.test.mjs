import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync,mkdirSync,writeFileSync,rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyRelease } from "./verify-release-bundle.mjs";
test("release gate requires every platform format and rejects developer-server references",()=>{
  const root=mkdtempSync(join(tmpdir(),"tenjee-bundle-test-"));
  try {
    mkdirSync(join(root,"src-tauri"));mkdirSync(join(root,"dist"));mkdirSync(join(root,"bundle"));
    writeFileSync(join(root,"src-tauri/tauri.conf.json"),JSON.stringify({productName:"Tenjee Vault",version:"1.0.0",identifier:"com.sam.tenjee-vault",build:{devUrl:"http://localhost:1420"}}));
    writeFileSync(join(root,"package.json"),JSON.stringify({version:"1.0.0"}));
    writeFileSync(join(root,"src-tauri/Cargo.toml"),'[package]\nversion = "1.0.0"\n');
    writeFileSync(join(root,"dist/index.html"),"<html>Offline</html>");
    writeFileSync(join(root,"bundle/tenjee_1.0.0.deb"),"");
    assert.throws(()=>verifyRelease("linux",join(root,"bundle"),root),/AppImage/);
    writeFileSync(join(root,"bundle/tenjee_1.0.0.AppImage"),"");
    assert.equal(verifyRelease("linux",join(root,"bundle"),root).bundles.length,2);
    writeFileSync(join(root,"dist/index.html"),'http://localhost:1420');
    assert.throws(()=>verifyRelease("linux",join(root,"bundle"),root),/development server/);
  } finally { rmSync(root,{recursive:true,force:true}); }
});
