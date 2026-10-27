//! fuzz 目标：ttf.rs 手写字节解析（name 表读取 / 改名重建 / 子集化）。
//! 用户 `--font-file` 可传入任意字节；三个入口同时喂，任一 panic /
//! abort 即缺陷（Option 返回是正常失败路径）。

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = trans_epub_core::ttf::family_name(data);
    let _ = trans_epub_core::ttf::rename_family(data, "F", "F Regular", "F", "note");
    let _ = trans_epub_core::ttf::subset_to_chars(data, "abc汉");
});
