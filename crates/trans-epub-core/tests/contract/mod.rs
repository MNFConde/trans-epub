//! 契约层：锁对外承诺，近乎冻结。改动这里=改承诺，按 ADR 级慎重。
//! - `bytes`：确定性 + golden 字节比对（`UPDATE_GOLDEN=1` 再生）
//! - `epubcheck`：验收门（产物必须过 epubcheck）

pub mod bytes;
pub mod epubcheck;
