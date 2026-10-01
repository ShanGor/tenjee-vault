# KDF 派生耗时基准（Argon2id 默认参数）

- 日期：2026-09-20
- 测量对象：`crypto::kdf::derive_kek`（KEK 派生）
- 参数：m=64MB, t=3, p=4（spec.md §6 默认参数）
- 构建：`cargo run --release --example kdf_bench`
- 硬件：Intel Core i9-14900K（32 线程），91 GiB 内存（测量时可用 75 GiB）

## 结果

| 指标 | 值 |
|---|---|
| 5 次运行（ms） | 71, 70, 71, 70, 70 |
| 最小 | **70 ms** |
| 平均 | **70 ms** |

## 结论

默认参数派生耗时 ~70 ms，远低于 1s 的关注阈值，**无需调参**。
低端设备上若超标，`KdfParams` 从第一天即可配置（`wrap_dsk_with_params`），
届时再实测调参并更新本文件与 spec。

## 复现

```bash
cd src-tauri
cargo run --release --example kdf_bench
```
