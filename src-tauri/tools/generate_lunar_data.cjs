#!/usr/bin/env node
/**
 * 农历/节气查表数据生成脚本（design D4：内置 1900–2100 查表，纯索引计算）。
 *
 * 用法：
 *   npm install lunar-javascript   # 在任意目录
 *   node generate_lunar_data.js > ../src/calendar/lunar_data.rs
 *   或：LUNAR_JS=/path/to/lunar.js node generate_lunar_data.js > lunar_data.rs
 *
 * 数据源：lunar-javascript（寿星天文历算法，精确到日）。
 * 输出：Rust 常量表——
 *   LUNAR_INFO[u32; 201]         经典位掩码编码：bit0-3 闰月月份(0=无)，bit16 闰月天数(1=30,0=29)，
 *                                bit(16-m) 第 m 个平月天数(1=30,0=29)，m=1..12
 *   SOLAR_TERM_DAYS[[u8;24];201] 每年 24 节气的公历「日」（月由节气序号固定推导）
 * 节气顺序（索引 0..23）：小寒 大寒 立春 雨水 惊蛰 春分 清明 谷雨 立夏 小满 芒种 夏至
 *                        小暑 大暑 立秋 处暑 白露 秋分 寒露 霜降 立冬 小雪 大雪 冬至
 */
const L = require(process.env.LUNAR_JS || 'lunar-javascript');

const START_YEAR = 1900;
const END_YEAR = 2100;

const TERM_ORDER = [
  '小寒', '大寒', '立春', '雨水', '惊蛰', '春分', '清明', '谷雨',
  '立夏', '小满', '芒种', '夏至', '小暑', '大暑', '立秋', '处暑',
  '白露', '秋分', '寒露', '霜降', '立冬', '小雪', '大雪', '冬至',
];

function lunarInfo(year) {
  const y = L.LunarYear.fromYear(year);
  const leapMonth = y.getLeapMonth(); // 0 = 无闰月
  let info = leapMonth & 0xf;
  // 平月天数：bit(16 - m)，m = 1..12
  for (let m = 1; m <= 12; m++) {
    const month = y.getMonth(m);
    if (!month) throw new Error(`缺少 ${year} 年 ${m} 月数据`);
    if (month.getDayCount() === 30) {
      info |= 0x10000 >> m;
    }
  }
  if (leapMonth > 0) {
    const leap = y.getMonth(-leapMonth);
    if (!leap) throw new Error(`缺少 ${year} 年闰 ${leapMonth} 月数据`);
    if (leap.getDayCount() === 30) {
      info |= 0x10000;
    }
  }
  return info >>> 0;
}

function solarTermDays(year) {
  const found = {};
  // 遍历全年公历日，收集节气落点
  for (let m = 1; m <= 12; m++) {
    const daysInMonth = new Date(year, m, 0).getDate();
    for (let d = 1; d <= daysInMonth; d++) {
      const jq = L.Solar.fromYmd(year, m, d).getLunar().getJieQi();
      if (jq) found[jq] = d;
    }
  }
  return TERM_ORDER.map((name) => {
    const day = found[name];
    if (!day) throw new Error(`${year} 年缺少节气 ${name}`);
    return day;
  });
}

const infos = [];
const terms = [];
for (let y = START_YEAR; y <= END_YEAR; y++) {
  infos.push(lunarInfo(y));
  terms.push(solarTermDays(y));
}

const out = [];
out.push('//! 农历与二十四节气查表数据（1900–2100）。');
out.push('//!');
out.push('//! 本文件由 `src-tauri/tools/generate_lunar_data.cjs` 生成，请勿手改。');
out.push('//! 数据源：lunar-javascript（寿星天文历算法）。改动历法数据请重新运行生成脚本。');
out.push('');
out.push('/// 农历年信息位掩码表（1900–2100，索引 = 年份 - 1900）。');
out.push('/// 编码：bit0-3 闰月月份（0=无闰月）；bit16 闰月天数（1=30 天，0=29 天）；');
out.push('/// bit(16-m) 第 m 个平月天数（1=30，0=29），m=1..12。');
out.push('pub const LUNAR_INFO: [u32; 201] = [');
for (let i = 0; i < infos.length; i += 8) {
  out.push(
    '    ' + infos.slice(i, i + 8).map((v) => `0x${v.toString(16).padStart(8, '0')}`).join(', ') + ',',
  );
}
out.push('];');
out.push('');
out.push('/// 每年 24 节气的公历「日」（1900–2100，第一维索引 = 年份 - 1900，第二维为节气序号）。');
out.push('/// 节气序号 0..23 对应：小寒 大寒 立春 雨水 惊蛰 春分 清明 谷雨 立夏 小满 芒种 夏至');
out.push('/// 小暑 大暑 立秋 处暑 白露 秋分 寒露 霜降 立冬 小雪 大雪 冬至。');
out.push('/// 节气序号 n 的公历月 = n/2 + 1（小寒/大寒在 1 月，立春/雨水在 2 月，依此类推）。');
out.push('pub const SOLAR_TERM_DAYS: [[u8; 24]; 201] = [');
for (let i = 0; i < terms.length; i++) {
  out.push('    [' + terms[i].join(', ') + '], // ' + (START_YEAR + i));
}
out.push('];');
out.push('');
process.stdout.write(out.join('\n'));
console.error(`生成完成：${START_YEAR}-${END_YEAR}，${infos.length} 年`);
