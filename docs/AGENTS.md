# docs/ 写作约束

> 作用范围：仅在本目录（docs/）写/改文档时生效。

- **frontmatter 必填 4 项**：`type` / `status` / `summary` / `updated`；`tags`、`contains`、`created`、`related` 可选。
- **编号规则**：00 总纲 / 10-70 领域篇（十位一层、留空洞）/ 80-diff、90-env 通用尾位。编号管阅读顺序，不管稳定度。
- **单一事实源**：同一事实只一处详述，其余链接回指；契约类变更先改 [10-contracts.md](architecture/10-contracts.md) 再动代码。
- **方案状态标记**：多候选用 `【当前】/【备选】/【已否决】/【已替换】`，行内简记 = 链接，块记 = 小标题 + callout；状态变化同步更新触发/原因。
- **ADR**：一篇一文件 `NNNN-slug.md` 连续递增 + [README 索引](adr/README.md)；必填状态/日期/规范位置/背景/决策/依据/影响。
- **存档**：结构重组、精简、整篇重写前，先快照到 `archive_docs_v{num}/`（不含既有存档目录）；旧存档永不删除、不再修改。
- **归属**：本目录只收架构与实现相关文档；协作约定、工作流不入（放 `.agents/rules/` 或根 AGENTS.md）；坑与经验教训沉淀走 cairn（见 `cairn/RULES.md`），docs 只留结论与指针。
