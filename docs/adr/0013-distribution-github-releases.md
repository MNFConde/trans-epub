---
type: adr
status: accepted
summary: "分发渠道 GitHub Releases 预编译二进制（六目标），crates.io 暂缓非放弃、四 crate workspace 结构保留；配套 reqwest TLS 切 rustls（no-provider + ring）实现交叉零 C 依赖"
tags: [trans-epub, adr, distribution, releases, rustls]
contains: [decision]
created: 2026-10-25
updated: 2026-10-25
related: ["../requirements.md", "../architecture/00-overview.md", "../architecture/40-fetch.md", "../guide/install.md", "0012-license-agpl.md"]
authoring_mode: ai_generated
---

# 0013. 分发渠道：GitHub Releases 二进制；crates.io 暂缓

- 状态：accepted
- 日期：2026-10-25
- 相关：`docs/guide/install.md`、`crates/trans-epub-fetch/Cargo.toml`、`polite.rs` provider 安装点

## 背景

需求基线默认的发布形态含"crates.io 名称确认（trans-epub 系列）"。2026-10-25 决策变更：不想为发布把 trans-epub-contract / -core / -fetch / -cli 四个包全部上架占名，但四 crate 拆分（ADR 0002 的依赖红线与可测性）要原样保留。同时确定了面向最终用户的分发形态：GitHub Releases 预编译二进制，平台矩阵 Windows / Linux / macOS × x64 / arm64（Linux 静态链接）——这要求 TLS 栈可纯 Rust 工具链交叉构建。

## 决策

1. **分发 = GitHub Releases 预编译二进制**（六目标矩阵），辅以 `cargo install --git` 与源码构建两路（见 [../guide/install.md](../guide/install.md)）。
2. **crates.io 暂缓、非放弃**；四 crate workspace 结构原样保留——workspace 内多 crate 是本地结构，与是否发布互不影响。
3. **配套子决策：reqwest TLS 从 native-tls 切 rustls**。native-tls 在 Linux 交叉需 OpenSSL；reqwest `rustls` feature 默认绑的 aws-lc-rs 需 CMake/NASM 的 C 构建——两者都违背"纯 Rust 工具链交叉"目标。故走 `rustls-no-provider` + 显式依赖 rustls(ring)，provider 安装点在 `PoliteFetcher::new`（进程级一次，重复安装按幂等容忍）。

## 依据

- GitHub 分发二进制不要求源码包满足 crates.io 结构约束；contract 拆分的架构价值（依赖红线、契约测试、服务化接缝）在本地 workspace 已全额兑现，不因不上架而损失。
- rustls + ring 交叉实测（2026-10-25）：Cargo.lock 中 native-tls / openssl-sys / aws-lc-* 全部退场，六目标无 C 工具链依赖；fetch crate 27 测试全绿（回环 mock 不涉 TLS）。
- 证书验证走 rustls-platform-verifier 的平台证书库（Windows schannel / macOS security-framework 读系统根证书），行为与 native-tls 等价，用户无感。

## 后果

- 用户获取渠道与安装说明集中在 `docs/guide/install.md`；README 链接 Releases 页。
- **将来上架 crates.io 的两条路径**（届时另立 ADR）：整套 workspace 一起发（contract 等全部占名上架）；或只发 binary 包——需把 contract 并回 core（发布面结构变化，牵动 ADR 0002）。
- requirements.md 验收表的"crates.io 名称可用性"条目标记为已替换并指向本 ADR；00-overview 进度行同步暂缓口径。
- CI 发布流水线（tag → 交叉构建 → Releases 资产上传）与首次 Release 的版本基线，发布前另立任务，不在本决策范围内实施。
- 若 fetch crate 将来出现第二个 reqwest Client 构造点，必须复用同一 provider 安装（否则 no-provider 模式下该 Client 运行时 panic）。
