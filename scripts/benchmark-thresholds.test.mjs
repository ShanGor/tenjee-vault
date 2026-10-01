import { test } from "node:test";
import assert from "node:assert/strict";
import { checkThresholds } from "./benchmark-thresholds.mjs";
test("performance gates reject over-limit, boundary, missing and invalid metrics",()=>{
  assert.equal(checkThresholds({cold_start_ms:100,search_ms:40}).passed,true);
  for(const report of [{cold_start_ms:2000,search_ms:40},{cold_start_ms:100,search_ms:300},{cold_start_ms:2500,search_ms:10},{cold_start_ms:100,search_ms:NaN},{search_ms:10}]) assert.throws(()=>checkThresholds(report));
});
