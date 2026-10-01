# Design: M1 项目骨架、多库架构与加密原语

## Context

当前仓库为空（仅有 `spec.md` 草案与 `openspec/` 规划目录），属于从零搭建。需求与约束见 `spec.md`：纯本地离线桌面应用（Tauri 2.x + React 19 + Rust），多库分离 SQLite 存储，加密分区采用 AES-256-GCM + Argon2id 且密钥永不落盘。动机见 proposal.md。本设计只做架构与技术选型层面的决策，模块业务功能（M2+）不在此范围。

## Goals / Non-Goals

**Goals:**
- 建立可构建、可启动、可测试的 Tauri 2 应用骨架（三平台）
- 实现多库数据布局、迁移体系、启动完整性检查、WAL/事务
- 实现加密原语库并达到 100% 测试覆盖，为 M2 的解锁/锁定流程提供干净的 API 抽象

**Non-Goals:**
- 任何业务 UI（骨架只含最小验证界面，如一个调用 Rust command 的按钮）
- 加密分区的解锁会话管理、闲置超时自动锁定、内存 FTS 索引（M2）
- FTS5 搜索实现（M2/M3，search/ 目录仅占位）
- 打包分发流水线（M4）

## Decisions

### D1: SQLite 访问层选 `rusqlite`（bundled），不选 `sqlx`

**理由**：M1 全部数据库访问都在本进程内同步进行，`rusqlite` 直接暴露 `Connection` 与 `PRAGMA` 控制（WAL、integrity_check），无需 async runtime；`bundled` feature 锁定 SQLite 版本，避免不同平台系统 SQLite 差异。`sqlx` 的优势在 async/连接池/编译期校验，对嵌入式单进程场景是过度设计，且 `sqlx` 的 SQLite driver 底层同样是 rusqlite 生态的绑定，不带来实质收益。

**备选**：`sqlx`（否决，见上）；`diesel`（否决，ORM 抽象与手写迁移体系重复，且增加 schema 定义双份维护）。

### D2: 迁移采用嵌入式版本化迁移（内置 runner），不引入迁移框架

每个库一组迁移，`src-tauri/migrations/<db>/NNNN_*.sql` 命名，启动时比较 `schema_migrations` 表（`version INTEGER PRIMARY KEY, applied_at`）并按序应用缺失迁移，全程单个事务。M1 的迁移数量少且需要多库/动态空间库的灵活注册，手写 ~100 行 runner 比引入 `refinery`/`barrel` 更透明、依赖更少。

**备选**：`refinery`（可行但需适配动态空间库注册，收益不大，留待迁移复杂度增长后再评估）。

### D3: 加密模块（`src-tauri/src/crypto/`）的 API 以「密钥材料句柄」为中心，内存安全贯穿接口

核心类型：
- `KdfParams { m_cost, t_cost, p_cost }`（默认 64MB/3/4，可调以便测试提速）
- `SectionKeys { kek: SecretKey, dsk: SecretKey }`：所有密钥材料用 `zeroize::Zeroizing<[u8; 32]>` 持有，Drop 时自动清零
- `WrappedDsk { salt, params, verifier, ciphertext }`：唯一允许落盘的结构
- 操作：`derive_kek(password, salt, params)`、`wrap_dsk(dsk, password) -> WrappedDsk`、`unwrap_dsk(wrapped, password) -> Result<SectionKeys, UnlockError>`（内部先做 verifier 校验）、`seal(plaintext, dsk) -> (ciphertext, nonce)`、`open(ciphertext, nonce, dsk) -> Result<Vec<u8>, CryptoError>`

M2 的会话管理（持有 `SectionKeys`、闲置锁定、内存索引销毁）将消费这些原语；M1 不实现会话层，避免接口被 UI 流程绑架。

**备选**：把密码/PBKDF 逻辑散在 commands 层（否决，无法保证覆盖与内存安全审查的集中性）。

### D4: 随机数与 nonce：`rand_core::OsRng` + 每单元随机 nonce

DSK（32 字节）与每个加密单元（页/版本/附件）的 nonce（12 字节）均从 `OsRng` 生成；nonce 与密文拼接存储（`ciphertext || nonce` 单字段或两列）。AES-256-GCM 在随机 nonce 下生日界内（~2^32 条）对单分区数据量足够安全，且不依赖状态计数器——加密可能跨多个进程生命周期（备份恢复、手动替换库文件），计数器方案易出错。

**备选**：XChaCha20-Poly1305（备选合理，但 spec.md 已明确 AES-256-GCM，遵循已定决策）。

### D5: 错误处理与隔离：后端统一 `VaultError` 枚举，完整性检查失败走「隔离 + 提示」而非 panic

`src-tauri/src/error.rs` 定义 `VaultError`（`DbIntegrity`、`Migration`、`Crypto`、`Io`、`NotFound`…），实现 `serde::Serialize` 直接作为 Tauri command 的 Err 负载返回前端。启动流程：打开 meta.db → 迁移 → integrity_check → 加载空间注册表 → 逐个打开空间库（每步失败仅隔离该空间，前端收到结构化告警列表）。任何单库失败 SHALL NOT 使进程退出。

### D6: 前端骨架：Vite + React 19 + TypeScript，模块目录先行空置

`src/modules/{calendar,tasks,notes}/` 与 `src/shared/` 仅放占位（如各自一个 TODO 路由入口），最小界面放在 `src/App.tsx`：一个按钮调用 Rust command（如 `greet` 或 `db_status`）验证前后端链路。状态管理与路由库待 M2 有真实需求时再引入，避免现在选型过早。

### D7: 测试策略

- `crypto/`：100% 覆盖——KDF 往返、verifier 正反、seal/open 往返、篡改检测、wrap/unwrap、错误密码、清零语义（尽力验证）
- `db/`：每个迁移在临时目录（`tempfile` crate）新建库验证 schema；完整性检查用「损坏文件 + 正常文件」组合测试隔离逻辑
- 命令层：对 command 函数做纯 Rust 单测（不依赖 WebView），Tauri 集成在 M2 随首个真实功能补

## Risks / Trade-offs

- [Argon2id m=64MB,t=3,p=4 在低内存设备上派生耗时可能 >1s] → `KdfParams` 从第一天即可配置；M1 任务中包含在低端硬件上的耗时基准测试，若超标调参并回写 spec
- [手写迁移 runner 早期 bug 可能导致 schema 漂移] → 迁移 runner 自身纳入单测（乱序文件拒绝、部分失败回滚、重复启动幂等）
- [加密格式一旦发布即成为长期兼容负担] → 密文格式带版本字节（v1 = 1 byte format tag || ciphertext || nonce），M1 预留扩展位
- [多库并发写入的锁竞争] → M1 每库单 `Connection` + `Mutex`，WAL 下读写不互斥；性能不足时（M3 后）再评估连接池
- [integrity_check 大库耗时] → 大库上 `PRAGMA integrity_check` 可能秒级；M1 用快检（integrity_check 本身即全检，必要时改 `quick_check`），启动时异步执行不阻塞首屏

## Migration Plan

全新项目，无旧数据迁移。回滚策略：M1 无用户数据，删除数据目录即完全回滚。

## Open Questions

- 应用数据目录的具体路径策略（Tauri `app_data_dir` + `tenjee-vault/` 子目录为默认假设，实现时确认各平台行为）
- 前端 UI 组件库选型（M2 首个真实界面时再定，不影响本变更）
