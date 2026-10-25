//! 集成测试入口（单 binary）：跑真实二进制（`CARGO_BIN_EXE_trans-epub`）
//! 覆盖退出码、产物、dry-run 与机器可读报告（docs/architecture/60-cli.md
//! 「测试」）。全程不访问网络（crawl 用回环服务）。
//!
//! - `contract/`：进程接口契约——退出码语义、stdout 形态、dry-run 不落盘。
//! - `e2e/`：端到端——真转换出 epub、crawl 回环站点出书。
//! - `support/`：进程执行 helper 与 fixtures 路径。
//!
//! 防静默跳过：新增测试文件必须挂进对应 mod.rs。对账：
//! `cargo test -p trans-epub-cli --test integration -- --list`（9 个）。

mod contract;
mod e2e;
mod support;
