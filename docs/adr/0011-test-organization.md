---
type: adr
status: accepted
summary: "集成测试按契约/行为/红线语义分层，单 binary 组织"
tags: [trans-epub, adr]
contains: [decision]
created: 2026-10-25
updated: 2026-10-25
related: ["../architecture/20-core.md", "../architecture/40-fetch.md", "../architecture/60-cli.md"]
authoring_mode: ai_generated
---

# 0011. 集成测试按语义分层（单 binary 组织）

- 状态：accepted
- 日期：2026-10-25
- 相关：architecture/20-core.md（测试策略）、architecture/40-fetch.md、architecture/60-cli.md

## 背景

集成测试按来源文件堆放：core 的 golden.rs（677 行）混装结构断言与字节回归，fetch 顶层的 mock_server.rs 被自动编成 0-test 空 binary，cli 单文件混接口契约与端到端。语义混装时无法回答"改这个功能时哪些测试爆红是保护、哪些是噪音"。三类语义实际不同命：

- **契约**（golden 字节、确定性、epubcheck 门、CLI 退出码/stdout 形态）：锁对外承诺，改这里=改承诺；
- **行为**（结构断言、预设选择器链）：随产品演进调整；
- **红线**（依赖方向）：永不动。

重组过程中发现 **epubcheck 门假绿**：jar 以运行时相对路径 `target/epubcheck/...` 判存在性，而测试进程 cwd 是 crate 根 → 永远 miss → 门恒 skip；skip 走 eprintln 被 cargo test 吞掉，测试"绿"从未校验过产物（真实验收一直由 scripts/epubcheck.sh 兜底承担）。

## 决策

1. 每个 crate 的 `tests/` 采用**单 binary + 语义子目录**：顶层只放 `main.rs`（mod 声明），实体在 `contract/`（契约）、`behavior/`（行为，仅 core）、`arch/`（红线，仅 core）、`e2e/`（端到端，仅 cli）、`support/`（helper）；`Cargo.toml` 显式 `autotests = false` + `[[test]] name = "integration"`。
2. 归层判据成文："改这个功能时测试爆红是保护还是噪音"——保护进 contract，噪音降级为 behavior，架构约束进 arch。
3. 测试引用仓库资源（epubcheck jar 等）一律 `concat!(env!("CARGO_MANIFEST_DIR"), ...)` 编译期常量，**不得依赖运行时相对路径/cwd**——假绿的根因即此。

否决的备选：

- **每语义一个 test binary**（顶层平铺 contract.rs / behavior.rs…）：cargo 多 test binary 默认串行执行，并行度不升反降；autotest 对顶层 .rs 各自成 binary，"0-test 空 binary"问题换形态复现。
- **纯命名分层**（contract_bytes.rs / behavior_md.rs 前缀）：层级归属不可一目了然，分组与检索都靠约定维持。

## 后果

- 测试名即语义坐标：`contract::bytes::golden_bytes`、`behavior::web::*`、`arch::deps::core_never_depends_on_fetch`。
- **新增测试文件必须挂进对应 mod.rs——漏挂不报错但静默不执行**。防回归靠对账：`cargo test -p <crate> --test integration -- --list`（core 11 / fetch 20 / cli 9）。
- support/ 等子目录天然不被 autotest 扫描，fetch 的 0-test 空 binary 随之消失。
- epubcheck 门修好后 core 集成测试真跑 5 次 JVM（约 +10s），这是设计意图而非回归。
- 纯移动纪律保证 golden/ 与 fixtures/ 零字节变化（本次重组已验证）。
