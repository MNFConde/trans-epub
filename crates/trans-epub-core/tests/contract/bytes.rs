//! 确定性与 golden 字节比对：`tests/golden/{name}.epub`。
//! 输入集（fixtures）本身是承诺的一部分——加场景走新增条目，不改已有
//! 输入。更新 golden：
//! `UPDATE_GOLDEN=1 cargo test -p trans-epub-core --test integration contract::bytes`
//! （diff 人工确认后再提交）。

use crate::support::{GOLDEN, convert, convert_web};

#[test]
fn deterministic_output() {
    let a = convert("flat");
    let b = convert("flat");
    assert_eq!(a.bytes, b.bytes, "同输入必须字节级一致");
}

#[test]
fn golden_bytes() {
    for name in ["flat", "tree", "single", "mdbook"] {
        let output = convert(name);
        let golden_path = format!("{GOLDEN}/{name}.epub");
        if std::env::var("UPDATE_GOLDEN").is_ok() {
            std::fs::write(&golden_path, &output.bytes).expect("write golden");
            continue;
        }
        let golden = std::fs::read(&golden_path)
            .unwrap_or_else(|_| panic!("golden {name}.epub 缺失；先运行 UPDATE_GOLDEN=1 生成"));
        assert_eq!(
            output.bytes, golden,
            "{name} 与 golden 不一致（意图变更请 UPDATE_GOLDEN=1 重新生成并 review diff）"
        );
    }
    // web 样书（Pages 源，无 UPDATE_GOLDEN 之外的差异面）
    let output = convert_web();
    let golden_path = format!("{GOLDEN}/web.epub");
    if std::env::var("UPDATE_GOLDEN").is_ok() {
        std::fs::write(&golden_path, &output.bytes).expect("write golden");
    } else {
        let golden = std::fs::read(&golden_path)
            .unwrap_or_else(|_| panic!("golden web.epub 缺失；先运行 UPDATE_GOLDEN=1 生成"));
        assert_eq!(output.bytes, golden, "web 与 golden 不一致");
    }
}
