# crates/ 代码协作约束

动 `crates/` 下任何代码前，**先读对应组件的架构文档**，确认改动不违反其口径（分层红线、依赖方向、契约语义）；与文档冲突时先指出冲突点、说明取舍，再执行——禁止"代码先行、文档追认"（10-contracts.md 破坏性变更流程）。

## crate ↔ 架构文档对照（动工前先读）

| crate | 对应文档 |
|---|---|
| trans-epub-contract | `docs/architecture/10-contracts.md`（本身即契约源） |
| trans-epub-core | `docs/architecture/20-core.md`（pipeline）、`30-frontends.md`（三前端与抽取红线）、`50-writer.md`（epub3 writer） |
| trans-epub-fetch | `docs/architecture/40-fetch.md`（依赖方向 fetch→contract；fetch 不做内容理解） |
| trans-epub-cli | `docs/architecture/60-cli.md` + `docs/guide/cli.md`（旗标全表单一事实源，两处须同步） |

## 提醒

- 契约/接口类事实变更：先改 `10-contracts.md` 再动代码，golden 样书随之更新（`UPDATE_GOLDEN=1` 再生，diff 人工确认）。
- 门禁跑法见根 `AGENTS.md` 质量约束节。
