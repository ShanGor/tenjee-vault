# Tasks

## 1. 项目骨架搭建

- [x] 1.1 初始化 Tauri 2.x 项目（Vite + React 19 + TypeScript 前端，`src-tauri/` Rust 后端），并按 spec.md §2.1 建立 `src/modules/{calendar,tasks,notes}`、`src/shared/`、`src-tauri/src/{db,crypto,search,commands}`、`src-tauri/migrations/` 目录；验证 `npm run tauri dev` 能启动应用窗口
- [x] 1.2 配置 Rust crate 依赖（`rusqlite` bundled、`aes-gcm`、`argon2`、`zeroize`、`rand_core`、`tempfile`[dev]）并验证 `cargo check` 通过
- [x] 1.3 实现最小验证界面：前端一个按钮调用 Rust command（如 `db_status`）并展示返回的 JSON；验证点击后能看到后端响应，确认前后端链路打通

## 2. 加密原语（crypto-core）

- [x] 2.1 实现 `KdfParams`（默认 Argon2id m=64MB, t=3, p=4）与 `derive_kek(password, salt, params)`，salt 从 `OsRng` 生成；验证同一密码+salt+参数派生出相同 KEK 的单元测试通过
- [x] 2.2 实现 `wrap_dsk` / `unwrap_dsk`（DSK 32 字节随机生成，KEK 包裹，verifier = KEK 加密的固定已知明文）；验证「初始化 → unwrap 成功」和「错误密码 → verifier 校验失败返回 UnlockError」的测试通过
- [x] 2.3 实现 `seal` / `open`（AES-256-GCM，每单元随机 12 字节 nonce，输出格式 `format_tag || ciphertext || nonce` 并带版本字节 v1）；验证加解密往返一致、篡改密文/标签返回解密失败错误的测试通过
- [x] 2.4 密钥材料全部改用 `zeroize::Zeroizing` 持有（`SectionKeys` 等结构实现 Drop 清零）；验证加密模块测试覆盖率达到 100%（`cargo tarpaulin` 或 `cargo llvm-cov` 输出确认）
- [x] 2.5 实现密码生成器（默认 ≥16 位，含大小写字母、数字、符号，`OsRng`）；验证默认设置下生成密码满足字符集与长度要求的测试通过
- [x] 2.6 在低端可用内存条件下测量 Argon2id 默认参数派生耗时并记录基准结果；验证基准输出落盘（如 `docs/benchmarks/kdf.md`），若 >1s 则调参并在任务备注中说明

## 3. 多库数据架构与迁移（data-storage）

- [x] 3.1 实现数据布局初始化：创建 `tenjee-vault/{meta.db, tasks.db, calendar.db, spaces/, backups/}`，缺失时自动补齐；验证首次运行于空目录后布局完整的测试通过
- [x] 3.2 实现迁移 runner：嵌入式 `migrations/<db>/NNNN_*.sql`，`schema_migrations` 表记录版本，单事务执行缺失迁移、拒绝乱序/重复；验证 runner 单测通过（新建库全量迁移、增量升级、重复启动幂等、部分失败回滚）
- [x] 3.3 编写 meta.db 迁移 v1（`app_config`、`spaces`、`tags` 表）、tasks.db 迁移 v1（`task_lists`、`tasks` 骨架）、calendar.db 迁移 v1（`events`、`event_exceptions` 骨架）、空间库迁移 v1（`notebooks`、`section_groups`、`sections` 含加密字段、`pages`、`page_versions`、`attachments`、`taggings` 骨架），schema 以 spec.md §3.2 为准；验证对空库应用全部迁移后 `PRAGMA table_info` 与预期一致的测试通过
- [x] 3.4 实现空间注册表操作（创建/重命名/归档空间，登记 meta.db 注册项 + 创建 `spaces/<space_id>.db` 与 `.files/` 目录）与注册项指向缺失文件时的检测；验证注册-创建一致性和缺失文件告警路径的测试通过
- [x] 3.5 实现启动流程：打开 meta.db → 迁移 → `PRAGMA integrity_check` → 加载空间注册表 → 逐个打开空间库，单库失败仅隔离该库并向前端返回结构化告警列表；验证「calendar.db 损坏文件场景下其余库正常加载且收到隔离告警」的集成测试通过
- [x] 3.6 统一数据库配置：全部库启用 WAL 模式，多记录写入包裹事务；验证「事务中途注入失败 → 重启后无部分写入」的测试通过
- [x] 3.7 实现 `VaultError` 统一错误枚举（含 `DbIntegrity`、`Migration`、`Crypto`、`Io`、`NotFound`），实现 `serde::Serialize` 供 Tauri command 返回；验证前端能收到结构化错误 JSON 的测试通过

## 4. 联调与验收

- [x] 4.1 实现 `db_status` command 返回各库状态（正常/隔离/缺失重建 + 版本号），接入最小界面展示；验证三态（全正常、单库隔离、缺库自愈）下界面显示正确的手工/自动测试通过
- [x] 4.2 运行全部测试套件（`cargo test` 全绿）并测量冷启动时间 < 2s；验证基准记录落盘，超标则在任务备注说明原因与后续措施
- [x] 4.3 验证三平台构建配置可用（至少本机平台 `npm run tauri build` 成功产出安装包；Windows/macOS 走 CI 交叉构建配置）；验证构建产物生成
