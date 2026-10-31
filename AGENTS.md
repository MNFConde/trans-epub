# trans-epub 协作规则

trans-epub —— markdown / mdBook / 静态文档站 → EPUB3 转换器，差异化是"编程文档保真"（构建期语法高亮、代码长行处理、mdBook 语义、链接改写）。当前状态：MVP1–MVP3 已落地、门禁全绿；发布前质量补强全部完成——真实站实抓、内嵌字体管线（TransEpub Mono/OFL 义务）、产品观察打磨、mutool 引擎冒烟门（scripts/mutool-smoke.sh）、真机抽验三档全过（Koodo 2.4.6 CDP 驱动 + KOReader WSL 构建 headless 渲染 + mutool 引擎）；License AGPL-3.0-only（ADR 0012）、分发 GitHub Releases 且 crates.io 暂缓（ADR 0013），用户手册 `docs/guide/` 与根 README 就位（仓库地址已补全）；测试补充战役与发布前测试收尾均收口（2026-10-28：163 测试全绿，CommonMark 0.31.2 spec 652 例批测、fuzz 正式一轮抓真缺陷三处已修——ttf name 表两处越界 panic、千层深目录栈溢出、ttcf 集合头 panic；覆盖率行 82.3%/分支 66.5%；测试组织细则 docs/testing/organization.md）；落盘原子化已落地（2026-10-28：tmp+rename+fsync，断电/被杀永无半成品，故障注入面 TRANS_EPUB_FAULT + 真 kill 测试，口径 60-cli.md 职责 6）；**v0.1.0 已首发**（2026-10-29：仓库 github.com/MNFConde/trans-epub，CI 门禁流水线（fmt/clippy/test/mutool 冒烟/文档三脚本 + MSRV 1.85 门）与 Releases 流水线（tag → 六目标构建+冒烟 → 资产上传 → 自举烟测）全绿，六平台预编译二进制发布且六目标构建冒烟与 golden 字节一致；首发抓到并修复跨平台确定性缺口——zip central directory system 字节归一 Unix(3) + .gitattributes 全仓 LF，契约口径 20-core.md/50-writer.md 先行，165 测试全绿）；故障上报模块落地（2026-10-30：三接收点统一实施——ProgressSink::warning 实时镜像 / BuildReport 权威汇总 / Result 致命轨，core reporter 单点收敛、pass 走 PassCtx.reporter、前端在入账合并点镜像（防 probe+load 双推），CLI stderr 实时行 + format_warning 格式单一源，退出码复核维持 0/1/2，fonts 六处失败点扩码 FontFallback，两路一致 core+e2e 双层钉死，176 测试全绿）；参照项目借鉴吸收入册（2026-10-30：md→epub 补三件——脚注 epub:type 语义化 / landmarks nav + guide / CSS 断词微补，全在 writer 层；pdf 前端设计要点——非扫描版重排、第四前端归一 BookDraft、mupdf-rs 优先建议、乱码/噪音/确定性三纪律，契约草图落 10-contracts.md、待办 todo.todo 两专项节，实施均未开始）；工具链体检复核收口（2026-10-31：ub-mode 四件套体检报告（tmp/toolchain-audit-2026-10-31.md，untracked）唯一红测试定性为**环境问题**——WSL 本地代理 + reqwest system-proxy + env NO_PROXY 不认 `127.*` 通配，回环 mock 流量被代理"洗"成 502；修复=PoliteConfig.respect_system_proxy（默认 true，口径 40-fetch.md）+ 契约测试显式脱离代理 + ci.yml proxy-immunity 死端口代理回归 job（把"开发机挂代理"场景纳入 CI），polite.rs 提取 get_with_retry 消双路 40 行重复（dryer 定性真实复制；两个小型重复豁免），变异复核 C 类 4 个证实 mutator 快照污染、A 类 10 缺口补 8 条测试，md/web register_cover CRAP 13.4/7.3 达标（detect 拆分遗留登记 todo），更正节落报告第五节，192 测试全绿 + epubcheck 全绿）；EPUB 输出增强三件落地（2026-10-31：契约先行定稿 10-contracts.md，全在 writer 层前端零改动——①脚注语义化 writer/footnotes.rs（comrak 形态字符串级识别：noteref/footnote aside/backlink，section/ol 摘除 id 保留，形态不匹配原样通过，章节页绑 xmlns:epub）；②landmarks nav（hidden，仅 cover+bodymatter；toc 自引用被 RSC-011 拒绝实证）+ OPF guide（EPUB2 legacy 三 reference，探针实证 3.3 合法）；③base.css 断词/断页（html 级 hyphens:auto + pre/code 禁断词 + widows/orphans + 表图 break-inside）配章节页/nav lang+xml:lang；golden 再生 diff 人工确认、epubcheck ALL GREEN、203 测试全绿，真机弹窗/快跳抽验点记 manual-checklist 待真机）；下一步：pdf 前端 + Apple Books 真机补验（暂缺设备）+ 增强三件真机弹窗/快跳抽验。

## 阅读顺序

1. 先读本文件（项目约束起点）。
2. 仓库存在 `cairn/` 时，先读 `cairn/RULES.md`（知识沉淀规则与完成回复门）；否则跳过。
3. 目录级约束：`docs/AGENTS.md`、`scripts/AGENTS.md`、`crates/AGENTS.md`，读取对应目录文件时自动生效。
4. 按需读 `docs/README.md`（文档地图）与 `cairn/ROADMAP.md`、`cairn/LOG.md`（存在时）。

## 文档协作规则

- 改动前判断用户要「讨论/建议」还是「直接改」；说"先看看/先评估"时先给分析，不直接重写正式文档。
- 纠正过往判断时追加更正说明，不悄悄覆盖。
- 未经确认的判断不写成既成事实。

## 协作约定

- 与用户交流一律使用中文。
- 指令若与仓库文档（`docs/`）或既有约定不符：先指出冲突点、说明取舍，再执行。
- 项目状态变化时同步更新本文件状态段。
- 契约/接口类事实变更：先改 `docs/architecture/10-contracts.md`，再动代码。

## 仲裁（skill 冲突裁决）

- 优先级：**项目内实际约定 > 项目级 skill > 全局 skill > 通用规范集**。
- 语言规范以 rust-* 系列 skill 为准；知识/教训沉淀唯一走 cairn（存在 `cairn/` 时）。
- **AGENTS.md 是本项目 tracked 资产**：任何 skill 只追加节、不独占文件，不得将其移出 git——隐私需求按内容拆分到 untracked 侧。

## 质量约束

- 代码门禁：`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test`。
- 产物验收：epubcheck 全绿（`scripts/epubcheck.sh`）+ golden 字节回归（`UPDATE_GOLDEN=1` 再生，diff 人工确认）+ 真机抽验（`docs/testing/manual-checklist.md`）。
- 提交信息遵循 Conventional Commits（类型: 主题，正文解释为什么）；`.githooks/commit-msg` 钩子强制格式（新克隆一次性安装：`git config core.hooksPath .githooks`）。
- 文档门禁：`python scripts/check-links.py` / `check-frontmatter.py` / `check-markers.py` 对 docs/ 全绿（台账见 `scripts/scripts.md`）。
