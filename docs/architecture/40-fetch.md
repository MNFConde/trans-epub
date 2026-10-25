---
type: architecture
status: active
summary: "trans-epub-fetch 获取库（MVP3 已落地）：PoliteFetcher（reqwest 阻塞 + robots 按 RFC 9309 口径 + 限速 + 重试 + 大小上限）、页面发现三机制（URL 列表/sitemap/同域递归 BFS）、产出 FetchedPage 列表喂 web 前端；依赖方向红线（fetch→contract，core 永不依赖 fetch）与服务化角色"
tags: [trans-epub, architecture, fetch, crawler, robots, sitemap]
contains: [design]
created: 2026-10-24
updated: 2026-10-26
related: ["../requirements.md", "10-contracts.md", "00-overview.md"]
authoring_mode: ai_generated
---

# 获取库（trans-epub-fetch）

## 定位与红线

fetch 是**采集库**：把"礼貌地拿到一批已排序页面"做成可复用能力。

- 依赖方向**只能是 fetch → contract**：fetch 实现契约的 `Fetcher`、产出 `FetchedPage`，绝不依赖 core（红线见 00-overview.md）。这样 fetch 保持轻量，也能被未来服务单独拿来当底层 HTTP 客户端。
- **core 永远不知道 fetch 的存在**。core 只见 `Fetcher` trait 与 `Source::Pages`；换实现（服务自带、测试 mock）零成本。
- **fetch 不做内容理解**：产出原始字节为止。正文抽取、编码判定、站点适配（预设 / selectors / strip）全在 core web 前端 + `WebCfg` 配置——sqlite.org 这类"无语义标签老站"的适配是用户侧 TOML，不是 fetch 代码，fetch 换实现时这些规则零牵连。
- **MVP3 已落地**（crate `trans-epub-fetch`）。MVP1 期间 CLI 内置的最小 Fetcher（reqwest 薄包装）已按计划整体平移进来，CLI 的资产拉取与页面采集统一走 `PoliteFetcher`。

## 能力面

### 默认 Fetcher 实现（PoliteFetcher）

- reqwest 阻塞客户端（core 同步内核的配套；服务复用时同样跑在 `spawn_blocking` 里）；TLS 走 rustls、crypto provider 显式选 ring（纯 Rust，交叉构建零 CMake/NASM；reqwest `rustls` feature 默认绑的 aws-lc-rs 需 C 构建，故走 no-provider + 构造时安装，见 ADR 0013）。
- **robots.txt**（`texting_robots` 解析）：按 origin 缓存，状态码口径遵循 RFC 9309 / Google 指引——2xx 解析（解析失败视同全放行）、4xx（429 除外）全放行、**429 / 5xx / 拉取失败全禁抓**（站点明确异常时保守不抓）。禁抓的 URL 返回 `FetchError::Denied`，调用方降级为 warning。`respect_robots = false`（即 `--ignore-robots`）整段跳过。robots.txt 自身占限速槽；解析前按 500 KiB 截断（库方建议）。
- **限速**：每 host（scheme+authority 为槽键）相邻请求最小间隔，默认 1s（`--rate-ms` 可配）；请求槽预约制、等待发生在锁外。
- **UA**：`trans-epub/<version> (+document conversion tool; personal use)`，可配；不伪装浏览器。
- 超时与重试：默认超时 30s；5xx 与网络错误重试 2 次（线性退避，基准 500ms）；4xx / 策略拒绝 / 超限不重试。
- 大小上限：Content-Length 预检 + 流式读取超限即断（默认与 AssetsCfg 单文件上限一致）。
- `FetchedPage.url` 采用**重定向后的最终 URL**（剥 fragment）——正文里相对引用的真实解析基。

### 页面发现（产出有序 `Vec<FetchedPage>`，顺序即书序）

三种机制对应需求定稿，全部实现（`discover` 模块）：

| 机制 | 顺序语义 | 适用 |
|---|---|---|
| **URL 列表文件**（`list.txt`，一行一 URL，`#` 注释） | 文件行序 = 书序 | 精确控序，质量最高的成书方式 |
| **sitemap.xml**（远程 URL 或本地文件均可） | 文件内条目序 = 书序；index 递归展开（深度上限 5 防环） | 站点自带结构 |
| **同域递归**（BFS） | 发现序（层序）= 书序 | 兜底；深度上限、include/exclude 路径 glob、页面数安全帽（默认 500）均可配 |

- 采集时只保留 `text/html`；非页面资源跳过并 warning。BFS 链接抽取对明显非 HTML 的扩展名（png/js/zip…）黑名单短路，省一次请求。
- **同域以起始 URL 的 host 为界**：起始 URL 重定向到另一域名时（如 `docs.gitbook.com` → `gitbook.com`），站内链接全判跨域不跟进（2026-10 实抓实证；保守正确但反直觉，改判最终 host 属发现语义变更，需求出现再议——CLI 侧已在用户手册提示用最终域名作起点）。
- 排序是网站成书的质量命门：`--dry-run` 在发现完成后打印清单即停，用户可以把清单粘进 `list.txt` 重排后二次转换。list / sitemap 只发现 URL，页面体由壳层按需 `fetch_pages`；BFS 在发现时顺带保留页面体，免二次抓取。
- sitemap 经 html5ever（scraper）**宽松解析**：sitemap 的标签集（`urlset/url/loc`、`sitemapindex/sitemap/loc`）小写无前缀、实体集与 HTML 一致，按 HTML 解析正确且免独立 XML 依赖，对缺 xmlns 等非标准变体更宽容；带命名空间前缀的变体不支持（已文档化）。
- 编码处理在 web 前端（body 传原始字节），fetch 不做有损转换。

### 与 CLI 的接线

```
trans-epub crawl <start-url | sitemap.xml | list.txt> [--depth N] [--max-pages N]
                 [--include GLOB] [--exclude GLOB] [--rate-ms MS] [--ignore-robots]
                 [--preset mkdocs|docusaurus|vitepress|gitbook] [通用输出/配置旗标]
```

输入判别：http(s) URL 且 path 以 `.xml` 结尾 → 远程 sitemap；其他 http(s) URL → BFS 起始页；本地文件按扩展名分本地 sitemap / list.txt。`--dry-run` 打印有序清单即停（BFS 模式下发现本身需要抓页面）。

### 服务化角色（三期）

服务需要的是"带队列/调度/缓存的采集系统"，那是服务的领域层；fetch crate 是它下面的**礼貌客户端库**。两种消费方式都成立：直接复用 PoliteFetcher，或实现契约 `Fetcher` 换成服务自己的实现（代理、鉴权、分布式缓存）。队列/优先级/任务持久化不在本 crate 范围。

替换边界（约束）：换实现（爬虫框架、代理池等）只替换**传输与发现**这一层；正文抽取与站点适配仍走 core web 前端 + `WebCfg` 同一事实源，**服务壳层不得绕过 core 自写抽取**——否则 CLI 与服务的抽取规则两套漂移。替换实现须保持礼貌与发现语义（robots 口径、限速、发现序即书序），fetch 的 mock 契约测试与 web golden 样书是行为锚点。

## 合规

- robots.txt 遵守是默认行为，仅显式开关可关（`--ignore-robots`，文档标注自负其责）。
- 默认限速温和；不伪装浏览器 UA。
- README 与 `--help` 明示：面向个人学习用途的文档化采集，使用者对目标站点的条款负责。

## 测试（已落地）

- 组织（ADR 0011）：`tests/` 单 binary 分 `contract/`（polite_http / discover_pages——对上游的承诺）与 `support/`（mock 服务）；跑法 `cargo test -p trans-epub-fetch --test integration`（对账 20 个）。
- **PoliteFetcher**：本地 mock HTTP 服务（`tests/support/mock_server.rs`，回环监听）覆盖 robots 允许/禁止/缺失/5xx 禁抓/缓存/开关、限速节拍（时间下界断言）、重试与退避（前 N 次 500）、4xx 不重试、大小上限截断。
- **发现机制**（契约 MockFetcher 离线驱动）：sitemap 标准序 / index 嵌套 / 非标准无 xmlns / 缺失与自引用环、BFS 深度 / include-exclude / 跨域剔除 / 去重 / 页面帽 / 非 HTML 跳过、list 解析与 `fetch_pages` 过滤。
- 原计划的"三个公开文档站录制 FetchedPage 快照"落地为**预设样本 fixture**（`trans-epub-core/tests/fixtures/web/presets/`，四预设各一）＋ web golden 样书——同口径防外网依赖，且样本可控可 diff；真实站点录制属可选加强，未做。
