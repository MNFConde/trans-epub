//! 集成测试入口（单 binary）：语义分层目录在此挂载（ADR 0011）。
//!
//! - `contract/`：契约层——锁对外承诺，近乎冻结（golden 字节、确定性、
//!   epubcheck 验收门）。改这里=改承诺。
//! - `behavior/`：行为层——随产品演进的结构断言（md / mdbook / web）。
//! - `arch/`：红线层——架构约束（依赖方向），永不动。
//! - `support/`：测试 helper（fixtures 路径、转换入口、zip 解包）。
//!
//! 防静默跳过：新增测试文件必须挂进对应 mod.rs，漏挂不报错但不执行。
//! 对账：`cargo test -p trans-epub-core --test integration -- --list`
//! 应列出全部测试（当前 11 个）。

mod arch;
mod behavior;
mod contract;
mod support;
