//! 原子落盘（60-cli.md 职责 6 的测试口）：进程中途死（注入 abort / 真
//! kill）时正式路径永无半成品——要么旧完整版、要么不存在；残留 `.tmp`
//! 由下次运行覆盖。注入钩子 `TRANS_EPUB_FAULT` 是产品二进制里的故障注
//! 入面（lib.rs write_epub_atomic），abort 无清理机会，等效断电的进程视角。

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::support::{BIN, FIXTURES, run, run_with_env};

/// 同输入两次转换字节级相同（确定性，golden 已钉）——本文件所有
/// "新版本字节 == 旧版本字节" 断言都建立在这一点上。
fn convert_once(out: &Path) -> Vec<u8> {
    let result = run(&[
        &format!("{FIXTURES}/single"),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "baseline convert: {}", result.stderr);
    std::fs::read(out).expect("epub written")
}

/// 注入 tmp 写到一半 abort：正式路径保持旧完整版，tmp 半截（长度与内容
/// 前缀都对得上）。
#[test]
fn crash_mid_tmp_write_keeps_former_epub_intact() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("book.epub");
    let old = convert_once(&out);

    let result = run_with_env(
        &[
            &format!("{FIXTURES}/single"),
            "-o",
            out.to_str().unwrap(),
            "--quiet",
        ],
        &[("TRANS_EPUB_FAULT", "tmp_half_write")],
    );
    assert_ne!(result.code, 0, "注入 abort 应非零退出：{}", result.stderr);

    let current = std::fs::read(&out).expect("正式路径仍可读");
    assert_eq!(current, old, "正式路径未被中途死污染（旧完整版）");

    let tmp = dir.path().join("book.epub.tmp");
    let leftover = std::fs::read(&tmp).expect("半截 tmp 残留");
    assert_eq!(leftover.len(), old.len() / 2, "写盘死在半程");
    assert_eq!(&leftover[..], &old[..leftover.len()], "残留是正式版前缀");
}

/// 注入 tmp 完整但 rename 前 abort：正式路径保持旧完整版，tmp 完整待命；
/// 下次正常运行把 tmp 消费掉（rename 覆盖正式路径后 tmp 不复存在）。
#[test]
fn crash_before_rename_keeps_former_epub_and_next_run_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("book.epub");
    let old = convert_once(&out);
    let tmp = dir.path().join("book.epub.tmp");

    let result = run_with_env(
        &[
            &format!("{FIXTURES}/single"),
            "-o",
            out.to_str().unwrap(),
            "--quiet",
        ],
        &[("TRANS_EPUB_FAULT", "tmp_no_rename")],
    );
    assert_ne!(result.code, 0, "注入 abort 应非零退出：{}", result.stderr);
    assert_eq!(
        std::fs::read(&out).unwrap(),
        old,
        "rename 前死，正式路径未动"
    );
    assert_eq!(
        std::fs::read(&tmp).expect("完整 tmp 待命").len(),
        old.len(),
        "tmp 已写完整，只差替换"
    );

    // 下次正常运行：tmp 被 rename 消费，正式路径完整。
    let result = run(&[
        &format!("{FIXTURES}/single"),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert!(!tmp.exists(), "rename 后 tmp 不复存在");
    assert_eq!(std::fs::read(&out).unwrap(), old, "正式路径完整");
}

/// 真 kill（无钩子）：spawn 转大书，轮询到 `.tmp` 出现即 TerminateProcess，
/// 断言宽口径——正式路径不存在、或与基准字节一致。窗口是竞态的（fsync+
/// rename 对小书太快），kill 落空就换更大的书重试。
#[test]
fn kill_during_write_window_leaves_output_whole_or_absent() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("big.epub");
    let src = dir.path().join("big.md");

    // 纯文本大书（避开 syntect 染色耗时），转换快、写盘窗口随体积走。
    let mut megabytes = 8;
    loop {
        let text = format!(
            "# Big\n\n{}",
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\n"
                .repeat(megabytes * 1024 * 1024 / 60)
        );
        std::fs::write(&src, text).unwrap();

        let baseline = convert_big(&src, &out);
        let tmp = dir.path().join("big.epub.tmp");

        let mut child = Command::new(BIN)
            .args([
                src.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
                "--quiet",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn");
        let mut killed = false;
        while child.try_wait().expect("poll").is_none() {
            if tmp.exists() {
                let _ = child.kill();
                killed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let status = child.wait().expect("wait");
        if !killed {
            // 写盘窗口没赶上进程活着的时候；换更大的书拉长窗口重试。
            assert!(
                megabytes < 32,
                "两轮都没能在 .tmp 存活窗口内 kill——写盘窗口异常"
            );
            megabytes *= 4;
            continue;
        }
        assert!(!status.success(), "kill 后不应成功退出");

        if out.exists() {
            assert_eq!(
                std::fs::read(&out).unwrap(),
                baseline,
                "正式路径要么旧完整版要么不存在，不得半成品"
            );
        }
        break;
    }
}

/// 大书基准转换（不走 convert_once：输入是生成的 md 而非 fixture）。
fn convert_big(src: &Path, out: &Path) -> Vec<u8> {
    let result = run(&[
        src.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "baseline big convert");
    let bytes = std::fs::read(out).expect("epub written");
    assert!(bytes.len() > 1024, "基准产物过小，写盘窗口测试无意义");
    bytes
}
