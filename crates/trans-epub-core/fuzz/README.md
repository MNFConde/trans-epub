# trans-epub-core fuzz 目标（常驻，不进日常 CI）

覆盖两个不可信输入入口（docs/testing/organization.md 落点表；robots/sitemap
已有畸形负例测试，暂不设目标）：

| 目标 | 被测面 | 为什么 |
|---|---|---|
| `fuzz_web_html` | web 前端 `load` 全链 | 网络输入完全不可控（畸形/截断/乱码/巨嵌套 HTML） |
| `fuzz_ttf` | `ttf.rs` name 表读取 / 改名 / 子集化 | 手写字节操作；用户 `--font-file` 任意字节 |

## 运行（发布前跑一轮；需要 nightly 与 cargo-fuzz）

**在 Linux / WSL 跑**（cargo-fuzzer 一等平台）。Windows MSVC 本机实测：默认
ASAN 模式缺 `clang_rt.asan_dynamic_runtime_thunk`，`--sanitizer none` 又缺
sancov runtime 符号——目标代码本身可编译（stable `cargo build` 验证到链接
层），只缺 libFuzzer runtime 环境，不在 Windows 上硬绕。

```bash
cargo install cargo-fuzz          # 一次性
cd crates/trans-epub-core/fuzz
cargo +nightly fuzz run fuzz_web_html -- -max_total_time=300
cargo +nightly fuzz run fuzz_ttf      -- -max_total_time=300
```

- 种子语料在 `corpus/<目标>/`（入库，小而精：有效样本 + 截断/片段变体）；
  libFuzzer 发现的新覆盖自动追加进 corpus 目录——提交前人工筛一遍再入库。
- 崩溃样本落在 `artifacts/`（gitignore，不入库）；修复后把触发样本提炼成
  回归测试进 `src/ttf.rs` 内嵌或 `tests/behavior/`（回归纪律见
  docs/testing/organization.md）。
