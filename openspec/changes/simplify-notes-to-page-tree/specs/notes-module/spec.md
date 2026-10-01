## REMOVED Requirements

### Requirement: 笔记本与分区层级组织
**Reason**: Spaces contain editable pages with optional children; users no longer create container types.
**Migration**: Existing notebooks, groups and sections become ordinary parent pages. Existing page IDs, relationships, content, attachments, versions and protected collections are preserved.

## MODIFIED Requirements

### Requirement: 页面树管理
The system SHALL support root pages directly in a space and arbitrarily nested child pages. Every node SHALL be editable content, displayed in one expandable navigation tree beside the editor. Users SHALL create, rename, reorder, move, recycle and restore page subtrees. Moves SHALL reject cycles and preserve content and protection. Canonical page links SHALL depend only on space and page IDs; old links SHALL redirect.

#### Scenario: 创建多级子页面
- **WHEN** a user creates a root page and children within it
- **THEN** all nodes can hold independent content and appear in a single tree

#### Scenario: Move and restore a subtree
- **WHEN** a user moves or recycles a parent page
- **THEN** its descendants follow it and can be restored without losing content

#### Scenario: Upgrade an existing space
- **WHEN** an existing space opens after upgrading
- **THEN** its containers appear as editable parent pages and existing pages retain their IDs and protected content

### Requirement: 空间管理

系统 SHALL 支持创建多个笔记空间，每个空间对应独立的库文件与附件目录（沿用 data-storage 的空间注册机制）；空间 SHALL 支持新建、重命名、归档与删除。归档的空间不再出现在常规导航中但数据保留、可恢复；删除空间 SHALL 先进入确认流程，删除后该空间的库文件与附件目录一并移除。应用启动时 SHALL 自动创建默认空间（当不存在任何空间时）。

#### Scenario: 首次启动创建默认空间

- **WHEN** 应用首次启动且空间注册表为空
- **THEN** 系统自动创建一个名为默认名称的空间，登记到注册表并初始化其库文件与附件目录

#### Scenario: 归档与恢复空间

- **WHEN** 用户归档一个空间后再次请求恢复
- **THEN** 该空间重新出现在导航中，其全部页面树完整可见

#### Scenario: 删除空间的确认与清理

- **WHEN** 用户确认删除某空间
- **THEN** 系统从注册表移除该空间并删除其库文件与附件目录，其他空间不受影响

### Requirement: 文件附件

系统 SHALL 支持将任意文件作为附件嵌入页面；附件内容 SHALL 以内容哈希命名存储于所属空间的附件目录，数据库中登记原始文件名、MIME 类型、大小与哈希。受保护页面树中的附件 SHALL 以密文形式写入附件目录。附件 SHALL 可从页面中打开与删除；删除附件 SHALL 同时清理附件目录中的文件。

#### Scenario: 普通分区附件落盘

- **WHEN** 用户向普通页面树页面附加一个文件
- **THEN** 附件以哈希命名存入该空间的附件目录，页面中可看到附件并可打开

#### Scenario: 加密分区附件密文存储

- **WHEN** 用户向已解锁的受保护页面树页面附加一个文件
- **THEN** 附件内容以 AES-256-GCM 密文写入附件目录，锁定页面树后该附件不可读

### Requirement: 加密分区设置与确认

系统 SHALL 允许用户为任意普通页面树设置独立加密密码：设置时 SHALL 经 KDF 派生密钥并生成随机 DSK、写入 salt/参数/验证器与 wrapped DSK（沿用 crypto-core），既有页面正文、历史版本及附件 SHALL 通过事务化迁移纳入加密保护。设置密码时系统 SHALL 强制用户确认「忘记密码则数据不可恢复」的提示，未确认不得完成设置。系统 SHALL 提供内置密码生成器入口（沿用 crypto-core 的生成能力），供设置密码时一键生成高强度密码。

#### Scenario: 设置密码需确认不可恢复

- **WHEN** 用户为页面树设置密码但未勾选/确认不可恢复提示
- **THEN** 设置流程被阻止，页面树保持未加密状态

#### Scenario: 已有页面的分区加密

- **WHEN** 用户为一个已含多个页面的页面树设置密码
- **THEN** 设置完成后该页面树在当前会话内保持解锁，全部既有页面正常可读；锁定后须通过密码解锁

### Requirement: 分区解锁与锁定

系统 SHALL 支持输入页面树密码解锁受保护页面树：解锁时 SHALL 先以验证器校验密码，密码错误 SHALL 给出明确错误且不建立任何解密能力。解锁状态 SHALL 在会话内有效，并 SHALL 在以下任一条件满足时自动锁定：闲置超时（默认 5 分钟，可配置）、用户手动锁定、应用退出。锁定时内存中的密钥材料 SHALL 被清零。锁定后，该页面树下页面正文、版本历史与附件 SHALL 不可读。

#### Scenario: 错误密码解锁失败

- **WHEN** 用户输入错误密码尝试解锁页面树
- **THEN** 系统提示密码错误，页面树保持锁定，页面内容不可见

#### Scenario: 闲置自动锁定

- **WHEN** 受保护页面树已解锁且超过配置的闲置时长无任何操作
- **THEN** 页面树自动锁定，正在查看的加密页面内容被隐藏/关闭

#### Scenario: 手动与退出锁定

- **WHEN** 用户点击锁定全部受保护页面树，或直接退出应用
- **THEN** 所有解锁中的页面树被锁定，内存密钥清零

### Requirement: 锁定状态下的可见性与搜索隔离

受保护页面树锁定时，系统 SHALL 保证：页面正文、绘图与附件内容不可见；该页面树内容 SHALL NOT 出现在任何搜索结果中；该页面树 SHALL NOT 建立或保留任何持久化搜索索引；导航中页面树的存在性可见，是否显示页面树下页面标题 SHALL 遵循用户设置（默认可见，可设置为隐藏）。

#### Scenario: 锁定分区不出现在搜索结果

- **WHEN** 用户执行全局搜索，且某受保护页面树处于锁定状态
- **THEN** 搜索结果不包含该页面树任何页面的标题或正文匹配项

#### Scenario: 锁定后无持久索引残留

- **WHEN** 受保护页面树从解锁转为锁定
- **THEN** 其会话内搜索索引被销毁，磁盘与数据库中不存在该页面树内容的索引数据

### Requirement: 修改与移除分区密码

系统 SHALL 支持修改受保护页面树密码：验证旧密码正确后，SHALL 仅用新密码派生的 KEK 重新包裹既有 DSK，SHALL NOT 重受保护页面树数据。系统 SHALL 支持移除页面树密码：验证密码后将页面树数据解密恢复为明文存储。两类操作失败（旧密码错误）时 SHALL 保持页面树原有加密状态不变。

#### Scenario: 修改密码不重加密数据

- **WHEN** 用户输入正确旧密码与新密码执行修改
- **THEN** 仅 wrapped DSK 与验证器被更新，页面树页面数据未被重写，新密码可解锁、旧密码不可解锁

### Requirement: 加密分区剪贴板保护

系统 SHALL 提供可选设置：开启后，从受保护页面树复制的内容 SHALL 在 30 秒后自动从剪贴板清空。默认设置状态 SHALL 在用户偏好中持久化。

#### Scenario: 复制内容自动清空

- **WHEN** 设置开启后用户从受保护页面树页面复制一段文本，30 秒后
- **THEN** 系统剪贴板内容被清空（替换为空）

### Requirement: 页面版本历史

系统 SHALL 为页面保存自动版本快照：每次内容保存时生成一个历史版本（沿用 `page_versions` 表），用户 SHALL 可查看历史版本列表并回滚到任一版本。回滚 SHALL 生成新的当前内容而不是删除历史版本。受保护页面树的版本快照 SHALL 以密文存储。

#### Scenario: 回滚页面版本

- **WHEN** 用户查看某页面的版本历史并选择回滚到较早版本
- **THEN** 页面当前内容变为该版本内容，版本历史中保留原当前版本与被选版本两条记录

### Requirement: 回收站

系统 SHALL 提供页面回收站：删除的页面及其子页面进入回收站，可浏览、恢复（回到原父页面）或彻底删除。彻底删除 SHALL 物理移除页面子树、版本数据与附件引用。受保护页面树锁定时，其回收站中的页面 SHALL 不可见且不可恢复。

#### Scenario: 删除后恢复

- **WHEN** 用户删除一个页面后从回收站执行恢复
- **THEN** 页面回到原父页面且内容完整

### Requirement: 最近使用页面

系统 SHALL 维护最近使用页面列表，按最后打开时间排序，供用户快速返回。列表 SHALL 不显示锁定受保护页面树中的页面。

#### Scenario: 最近列表排除锁定分区

- **WHEN** 用户打开过某受保护页面树页面，随后页面树被锁定
- **THEN** 最近使用列表中该页面被隐藏或标记为不可打开

### Requirement: 笔记全文搜索

系统 SHALL 提供笔记全文搜索：对每个空间库的页面标题与正文建立 FTS5 索引，编辑器内容变更时增量更新索引；用户 SHALL 可按关键词搜索并获结果列表（含所属空间与祖先页面路径上下文），支持按页面子树过滤；结果中关键词 SHALL 高亮。受保护页面树解锁时，系统 SHALL 将其解密内容写入内存临时 FTS 表参与搜索，锁定即销毁（见锁定隔离需求）。搜索接口 SHALL 返回结果不晚于 300ms（10 万页面规模的设计目标）。

#### Scenario: 普通分区全文搜索

- **WHEN** 用户输入关键词执行全局笔记搜索
- **THEN** 返回所有匹配页面（标题或正文命中），显示所属层级上下文并高亮关键词

#### Scenario: 加密分区解锁后可搜、锁定后不可搜

- **WHEN** 受保护页面树处于解锁状态时用户搜索其内容中的关键词
- **THEN** 结果包含该页面树页面；页面树锁定后再执行同一搜索，结果不再包含该页面树页面

### Requirement: 解锁会话状态一致性

同一应用会话内，受保护页面树的解锁状态 SHALL 对所有界面与命令保持一致：任何命令尝试读写锁定的受保护页面树 SHALL 被拒绝并返回明确的「页面树已锁定」错误，而不是返回乱码或空数据。前端 SHALL 在页面树状态变化（锁定/解锁）时同步更新导航、编辑器与搜索视图。

#### Scenario: 读写锁定分区被拒绝

- **WHEN** 前端或其他命令在页面树锁定时请求读取该页面树页面内容
- **THEN** 后端返回明确的锁定错误，不返回任何密文或解密失败的数据

## ADDED Requirements

### Requirement: Page subtree protection
The system SHALL allow password protection on a page and its descendants. Children SHALL inherit their parent's protection. Locked content, versions, attachments and search results SHALL remain inaccessible. Existing independent protected collections SHALL retain their keys. Password changes and removal SHALL operate on the protected subtree. Moving a protected collection SHALL preserve its protection; moves that would introduce conflicting independent protection SHALL fail with a clear explanation.

#### Scenario: Protect existing content
- **WHEN** a user protects a page with children, histories and attachments
- **THEN** the whole subtree is protected while siblings remain unchanged

#### Scenario: Create within a locked subtree
- **WHEN** a user tries to create or modify a child of a locked page
- **THEN** the operation requires unlocking first
