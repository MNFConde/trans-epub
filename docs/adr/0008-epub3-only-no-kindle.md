# 0008. EPUB3 唯一输出，不做 Kindle 适配

- 状态：accepted
- 日期：2026-10-10（2026-10-24 回填为 ADR）
- 相关：architecture/50-writer.md、research/ecosystem.md、requirements.md（非目标）

## 背景

调研确认 send-to-kindle 转换会丢 CSS，代码高亮天然脆弱；主力阅读器 Koodo Reader 与备选 Apple Books / KOReader 均为现代 EPUB3 阅读器。为 Kindle 做适配意味着牺牲性降级，直接伤及差异化支柱。

## 决策

只输出 EPUB 3，以现代 EPUB3 阅读器为基准；不做 Kindle 专项适配；mobi/PDF 列入非目标。Writer trait 预留未来输出格式的扩展点，但不为它做设计妥协。

## 后果

- 放弃 Kindle 直读市场（用户可自行转换并接受降级）。
- 验收以 epubcheck + 真机抽验为准（Koodo Reader 为主力，Apple Books / KOReader 各一）。
