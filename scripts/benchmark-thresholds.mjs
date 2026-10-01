export function checkThresholds(report) {
  for (const [key,limit] of [["cold_start_ms",2000],["search_ms",300]]) {
    if (!Number.isFinite(report[key]) || report[key]<0 || report[key]>=limit) throw new Error(`${key} must be below ${limit} ms; received ${report[key]}`);
  }
  return { ...report, passed:true };
}
