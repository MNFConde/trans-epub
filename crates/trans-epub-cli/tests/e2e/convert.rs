//! 本地转换端到端：tree fixture → 真实 epub 产物。

use crate::support::{FIXTURES, run};

#[test]
fn converts_tree_fixture_to_epub() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("tree.epub");
    let result = run(&[
        &format!("{FIXTURES}/tree"),
        "-o",
        out.to_str().unwrap(),
        "--quiet",
    ]);
    assert_eq!(result.code, 0, "stderr: {}", result.stderr);
    let bytes = std::fs::read(&out).expect("epub written");
    assert!(bytes.len() > 4000, "epub too small: {}", bytes.len());
    assert_eq!(&bytes[..2], b"PK", "zip magic");
}
