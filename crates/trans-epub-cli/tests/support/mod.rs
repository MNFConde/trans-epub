//! 进程执行 helper（无 #[test]）：真实二进制 + core 的 fixtures。

use std::process::Command;

pub(crate) const BIN: &str = env!("CARGO_BIN_EXE_trans-epub");
pub(crate) const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../trans-epub-core/tests/fixtures"
);

pub(crate) struct Out {
    pub(crate) code: i32,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

pub(crate) fn run(args: &[&str]) -> Out {
    run_with_env(args, &[])
}

/// 带环境变量跑（故障注入钩子 TRANS_EPUB_FAULT 等）。
pub(crate) fn run_with_env(args: &[&str], env: &[(&str, &str)]) -> Out {
    let mut cmd = Command::new(BIN);
    cmd.args(args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let output = cmd.output().expect("binary runs");
    Out {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}
