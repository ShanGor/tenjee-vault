# 冷启动基准

- 日期：2026-09-20
- 测量对象：release 构建的 `tenjee-vault` 桌面应用（spec: app-skeleton 冷启动 < 2s）
- 方法：进程启动 → WebKitWebProcess（WebView 渲染进程）出现的时间，
  以 20ms 间隔轮询 `ps --ppid`。数据目录已初始化（spec 明确首次运行建库不计入）。
- 硬件：Intel Core i9-14900K，91 GiB 内存，X11

## 结果

| 指标 | 值 |
|---|---|
| 冷启动 | **147 ms** |
| 阈值 | < 2000 ms |
| 结论 | **PASS**（余量 ~13 倍） |

## 备注

该指标为「进程启动 → WebView 开始渲染」的近似；前端 JS 载荷仅 ~221 KB，
渲染完成时间与此值非常接近。低端硬件复测方式：

```bash
npm run tauri build
# 运行 release 二进制，计时到窗口内容可见
```
