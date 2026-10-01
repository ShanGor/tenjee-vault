# Spec Delta

## Purpose

定义 Tenjee Vault 加密分区的底层密码学能力：以行业标准算法（Argon2id + AES-256-GCM）实现分区密码派生、两层密钥结构包裹、密码验证与数据加解密，并保证密钥与明文密码永不写入磁盘。

## ADDED Requirements

### Requirement: 两层密钥结构
每个加密分区 SHALL 使用随机生成的分区数据密钥（DSK）加密其数据；分区密码 SHALL 经 Argon2id 派生出密钥加密密钥（KEK）用于包裹（加密存储）DSK。修改分区密码时 SHALL 仅需用新密码派生的 KEK 重新包裹既有 DSK，SHALL NOT 要求重加密分区数据。

#### Scenario: 初始化加密分区
- **WHEN** 用户为一个分区设置加密密码
- **THEN** 系统生成随机 DSK，用 Argon2id（密码 + 随机 salt）派生的 KEK 包裹 DSK，库中仅持久化 salt、KDF 参数与包裹后的 DSK

#### Scenario: 修改分区密码
- **WHEN** 用户验证旧密码后设置新密码
- **THEN** 系统用新密码派生的 KEK 重新包裹同一 DSK 并替换持久化的密钥材料，分区内的页面与附件数据保持不变

### Requirement: 密钥派生参数与盐
密钥派生 SHALL 使用 Argon2id，默认参数为 m=64MB、t=3、p=4，每个分区使用独立的随机 salt。KDF 参数与 salt SHALL 随分区持久化，以便后续解锁时复现相同派生过程。

#### Scenario: 派生 KEK
- **WHEN** 输入正确的分区密码与该分区存储的 salt 及 KDF 参数
- **THEN** 派生出与设置密码时一致的 KEK，可成功解包 DSK

#### Scenario: 错误密码无法解包
- **WHEN** 输入错误的分区密码
- **THEN** 解包 DSK 失败或验证器校验失败，系统判定密码错误，不暴露任何密钥材料

### Requirement: 密码验证器
每个加密分区 SHALL 持久化一个验证器（verifier）：由 KEK 加密的固定已知明文。解锁时系统 SHALL 通过解密验证器并比对明文来判定密码正确性，SHALL NOT 以任何形式存储密码或 KEK 本身。

#### Scenario: 验证正确密码
- **WHEN** 用户输入正确密码，系统派生 KEK 并解密验证器
- **THEN** 解密结果与固定明文一致，解锁成功

### Requirement: 数据加密
页面正文、历史版本与附件 SHALL 使用 DSK 经 AES-256-GCM 加密；每个被加密的单元（页、版本、附件）SHALL 使用独立的随机 nonce，密文与 nonce 一并存储。解密失败的密文 SHALL 返回明确的解密失败错误而不是静默返回乱码。

#### Scenario: 加密解密往返
- **WHEN** 使用 DSK 加密一段页面内容后，用同一 DSK 解密
- **THEN** 还原出与原文完全一致的内容

#### Scenario: 密文被篡改
- **WHEN** 存储的密文在 GCM 认证标签之外被改动后尝试解密
- **THEN** 解密失败并返回明确错误，不会输出篡改后的明文

### Requirement: 密钥不落盘
系统 SHALL NOT 将 DSK、KEK、分区密码或任何恢复令牌写入磁盘、钥匙串或任何持久化存储；持久化介质上 SHALL 只允许存在 wrapped DSK、KDF salt、KDF 参数与验证器。内存中的密钥材料 SHALL 在不再需要时（如锁定、退出）被显式清零。

#### Scenario: 磁盘上无密钥残留
- **WHEN** 检查数据目录及系统钥匙串，在设置密码并解锁使用分区之后
- **THEN** 不存在任何明文 DSK、KEK 或密码；仅能找到 wrapped DSK、salt、KDF 参数与验证器

#### Scenario: 锁定后内存清零
- **WHEN** 分区从解锁状态转为锁定（闲置超时、手动锁定或退出应用）
- **THEN** 内存中的 KEK 与 DSK 被清零，此后任何加解密操作必须重新输入密码

### Requirement: 密码生成器
系统 SHALL 提供内置密码生成器，可生成指定长度的高强度随机密码（默认至少 16 位，含大小写字母、数字与符号），供用户用于新分区密码或直接写入笔记。

#### Scenario: 生成高强度密码
- **WHEN** 用户调用密码生成器并采用默认设置
- **THEN** 得到一个至少 16 位、包含大小写字母、数字与符号的随机密码
