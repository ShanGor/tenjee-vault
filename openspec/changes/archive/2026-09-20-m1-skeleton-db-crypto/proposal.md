# Proposal: M1 项目骨架、多库架构与加密原语

## Why

Tenjee Vault 是一个纯本地离线的个人生产力工具（日程 + 任务 + 对标 OneNote 的笔记，含加密分区）。目前只有产品/技术规格草案（根目录 `spec.md`），没有任何代码。M1 的目标是先建立整个产品的技术地基：可运行的 Tauri 2 + React 19 骨架、多库分离的 SQLite 存储架构与迁移体系、以及加密分区所依赖的全部密码学原语。没有这层地基，后续 M2 笔记模块、M3 任务/日程模块都无从落地。

## What Changes

- 新建 Tauri 2.x 项目骨架：React 19 + TypeScript 前端、`src-tauri` Rust 后端、建议的目录结构（`src/modules/*`、`src-tauri/src/{db,crypto,search,commands}`、`src-tauri/migrations`）
- 实现多库分离存储架构：主库 `meta.db`（配置、空间注册表、标签字典）+ 领域分库 `tasks.db` / `calendar.db` + 笔记空间库 `spaces/<space_id>.db` 及独立附件目录
- 实现 SQL 迁移体系：版本化迁移文件、启动时自动执行、记录已应用版本
- 实现启动完整性检查：对每个库执行 `PRAGMA integrity_check` 快检，异常库隔离并提示（不阻断其他库）
- 实现加密原语（RustCrypto）：Argon2id 密钥派生（m=64MB, t=3, p=4）、AES-256-GCM 字段加密、两层密钥结构（分区数据密钥 DSK + 密钥加密密钥 KEK 包裹）、密码验证器（verifier）、`zeroize` 内存清零保护
- 数据库统一启用 WAL 模式、写入事务化
- 建立 Rust 核心逻辑的单元测试体系；加密模块要求 100% 测试覆盖

非目标（本变更不做）：笔记/任务/日程任何业务功能、UI（骨架仅含最小可验证界面）、加密分区的解锁/锁定交互流程（M2 实现，M1 只提供底层原语）。

## Capabilities

### New Capabilities

- `app-skeleton`: Tauri 2.x 桌面应用骨架的创建、组织与启动要求（前端 React 19 + TypeScript，后端 Rust，目录结构，跨平台桌面目标）
- `data-storage`: 多库分离 SQLite 存储架构（主库 + 领域分库 + 空间分库）、SQL 迁移体系、启动完整性检查与 WAL/事务可靠性要求
- `crypto-core`: 加密分区底层密码学能力——Argon2id 派生、AES-256-GCM 加密、DSK/KEK 两层密钥包裹、密码验证器、密钥不落盘与内存清零

### Modified Capabilities

（无——项目尚无任何既有 spec）

## Impact

- **代码**：新建 `src/`（React 前端骨架）与 `src-tauri/`（Rust 后端骨架）两个顶层目录；本项目当前为空仓库，无既有代码受影响
- **依赖**：Tauri 2.x、React 19、TypeScript、Vite、Rust crate：`rusqlite`（SQLite 访问）、`aes-gcm`、`argon2`、`zeroize`、`rand`（或 `rand_core`）
- **数据**：首次运行时在应用数据目录创建 `tenjee-vault/` 数据布局（`meta.db`、`tasks.db`、`calendar.db`、`spaces/`、`backups/`）
- **风险**：Argon2id 参数（m=64MB, t=3, p=4）在低端设备上的派生耗时需在 M1 验证；加密原语的接口设计需为 M2 的解锁/锁定流程预留正确的抽象
