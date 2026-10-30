//! 故障上报三接收点的端到端对账（10-contracts.md「故障上报：三接收点
//! 统一」）：真实二进制里，stderr 实时 warning 行（ProgressSink::warning
//! 的 CLI 出口适配）与 `--report json` 的 BuildReport.warnings（权威汇总）
//! 两路一致；--quiet 关实时行；致命轨不走实时行（走退出码）。

use crate::support::{FIXTURES, run};

/// fixtures/flat 内含 remote.png 引用；`--no-remote-images` 注入离线降级
///（asset-resolve 记 asset-fetch-failed，不访问网络）。
#[test]
fn realtime_lines_match_json_report_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("flat.epub");
    let result = run(&[
        &format!("{FIXTURES}/flat"),
        "-o",
        out.to_str().unwrap(),
        "--no-remote-images",
        "--report",
        "json",
    ]);
    assert_eq!(result.code, 0, "成功带 warning 仍为 0：{}", result.stderr);

    // 权威轨：stdout JSON 的 warnings（code 机器可读 kebab-case）
    let report: serde_json::Value = serde_json::from_str(&result.stdout).expect("json report");
    let mut reported_codes: Vec<&str> = report["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|w| w["code"].as_str().expect("kebab code"))
        .collect();
    assert!(
        reported_codes.contains(&"asset-fetch-failed"),
        "注入面必须产生 asset-fetch-failed：{reported_codes:?}"
    );

    // 实时轨：stderr 逐条行，逐行抠出 [code]——两路码多重集全等（同源）
    let mut realtime_codes: Vec<&str> = result
        .stderr
        .lines()
        .filter(|l| l.starts_with("trans-epub: warning ["))
        .map(|l| {
            let start = l.find('[').unwrap();
            let end = l.find(']').unwrap();
            &l[start + 1..end]
        })
        .collect();
    reported_codes.sort_unstable();
    realtime_codes.sort_unstable();
    assert_eq!(
        realtime_codes, reported_codes,
        "实时行与权威汇总必须同源同量\nstderr: {}\nreport: {reported_codes:?}",
        result.stderr
    );

    // 人读消息同源：报告里的 message 必须在实时行中逐字出现
    for w in report["warnings"].as_array().unwrap() {
        let message = w["message"].as_str().unwrap();
        assert!(
            result.stderr.contains(message),
            "实时行须携带报告同款 message：{message:?}"
        );
    }
}

#[test]
fn quiet_suppresses_realtime_warning_lines() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("flat.epub");
    let result = run(&[
        &format!("{FIXTURES}/flat"),
        "-o",
        out.to_str().unwrap(),
        "--no-remote-images",
        "--quiet",
    ]);
    assert_eq!(result.code, 0);
    assert!(
        !result
            .stderr
            .lines()
            .any(|l| l.starts_with("trans-epub: warning ")),
        "--quiet 下无实时 warning 行：{}",
        result.stderr
    );
}

#[test]
fn fatal_never_prints_as_realtime_warning() {
    // 致命轨走退出码与错误行，不借 warning 通道（无前端可识别 → 退出码 2）
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("only.bin");
    std::fs::write(&input, b"\x00\x01").unwrap();
    let out = dir.path().join("x.epub");
    let result = run(&[input.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(result.code, 2, "无前端可识别 = 用法错误");
    assert!(
        !result
            .stderr
            .lines()
            .any(|l| l.starts_with("trans-epub: warning ")),
        "致命故障不得以 warning 行呈现：{}",
        result.stderr
    );
}
