---
type: testing
status: active
summary: "测试落点与组织操作细则：单元/集成分布判据、镜像映射与命名、单一锚点与反模式、fuzz 位置——ADR 0011 分层决策的操作手册"
tags: [trans-epub, testing, organization]
contains: [checklist]
created: 2026-10-26
updated: 2026-10-26
related: ["../adr/0011-test-organization.md", "manual-checklist.md"]
authoring_mode: ai_generated
---

# 测试组织与落点

[ADR 0011](../adr/0011-test-organization.md) 定了集成测试四层语义分层（contract / behavior / arch / e2e，单 binary 组织）。本篇是它的操作手册：新增或挪动测试时"放哪个文件、怎么命名、哪些不重复"。

## 分布判据：单元 or 集成

- **src 内嵌单测**（`#[cfg(test)] mod tests` 同文件底部）：私有函数、纯函数、解析器内部状态——测试与被测代码同文件，改动时零查找成本。
- **tests/ 集成**：走公共入口的跨模块行为，按 ADR 0011 归层（契约 / 行为 / 红线 / 端到端）。

一句话判据：**测"细节"内嵌，测"承诺与行为"集成**。私有可测就不出 src；需要 Vfs/Fetcher mock、多 pass 协作、产物断言才进 tests/。

## 落点速查表

| 测试内容 | 落点 | 现例 |
|---|---|---|
| 解析器/纯函数畸形输入（ttf、refs、dom） | src 同文件内嵌 | `src/ttf.rs`、`src/refs.rs` |
| 契约语义钉子（robots、重定向、glob、CLI 退出码） | `<crate> tests/contract/` | `contract/polite_http.rs` |
| 前端语义/结构断言 | `core tests/behavior/<前端名>.rs` | `behavior/web.rs` |
| 批量外部语料（CommonMark spec 等） | `behavior/` 独立新文件，勿撑爆既有文件 | `behavior/commonmark_spec.rs`（规划中） |
| 用户路径端到端 | `cli tests/e2e/` | `e2e/convert.rs` |
| 依赖红线 | `core tests/arch/` | `arch/deps.rs` |
| fuzz 目标 | crate 根 `fuzz/`（cargo-fuzz 惯例），不进 tests/ | —— |

## 组织三规则

1. **镜像映射**：tests/ 文件与 src 模块同名或同域（`behavior/web.rs` ↔ `src/frontends/web.rs`；`contract/polite_http.rs` ↔ `src/polite.rs`）。改某 src 文件时测试位置可推断：私有细节看同文件底部，公共行为看 tests/ 同名域文件。
2. **命名含被测对象**：`ttf_name_table_truncated_is_rejected` 而非 `test_bad_input`——grep 模块/函数名直达测试。与 ADR 0011"测试名即语义坐标"互补：坐标 = 层 + 模块 + 行为。
3. **新文件必须挂进对应 mod.rs**——漏挂不报错但静默不执行；对账命令见 [ADR 0011](../adr/0011-test-organization.md)。

## 单一锚点与反模式

- **一个行为只在最小完整层测一次**：细节不在集成层重复断言（脆且稀释语义坐标），behavior 已覆盖的不在 e2e 重测。各层职责互补而非重叠：单元盯细节、behavior 盯语义、contract 钉承诺、e2e 走用户路径——覆盖靠层职责，不靠重复。
- **golden 字节回归是产物兜底网**（`contract/bytes.rs`）：产物级意外改动由它抓，专项测试只管语义正确，不做逐字节重复断言。
- **反模式：按测试类型聚集的杂物文件**——如把所有畸形输入集中进 `malformed.rs`。看似整齐，实际切断与被测模块的映射（改 ttf 时想不到去 malformed.rs 找）；畸形输入散回各自 src 文件内嵌。

## 回归纪律

**每个修过的 bug 必须留一条修复前会红的测试**——没有测试的修复等于没修（下一次回归无报器）。落地口径：

- 修复提交里同时包含触发原缺陷的输入（截断字节 / 越界声明 / 篡改内容 / 异常 href），断言修复后的行为（拒绝 / 降级 / None），不是只测 happy path；
- 测试名或注释指回缺陷背景（如"修复前这里是裸切片 panic"），保留考古线索；
- 现行案例：robots 误拒（`fetch_direct` 三处代码曾零测试，2026-10 补 `fetch_direct_skips_robots_while_fetch_stay_bound`）、ttf name 表越界 panic（2026-10 修复随负例组）。

## 故障注入与已知不可测项

故障注入走 mock 出口：MockFetcher 决定 URL 应答形态（失败 / 篡改 / 超限）、输出路径占位（目录）注入落盘失败。已知不可测项须登记原因而非硬造：

- **字体缓存写失败降级**（`pipeline/fonts.rs` 的 `let _ =` 静默忽略）：写入只发生在 sha256 校验通过后，构造"校验通过且写失败"需要钉版 zip 的 sha 原像——不可行；注入需重构缓存层为 trait，收益不配。代码评审确认静默降级形态即可。
