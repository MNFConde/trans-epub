//! 字体工具：name 表读取与改名（OFL-1.1 修改版义务的运行时半边）。
//!
//! 只处理 sfnt v1 的 TTF（Maple Mono CN 与用户字体均此形态）；不做完整
//! 解析——name 表遍历 + 记录重写即可。改名的产物正确性由测试用
//! `ttf_parser::Face` 回读断言。

// ---------------------------------------------------------------------------
// name 表读取
// ---------------------------------------------------------------------------

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    let o = b.get(i..i + 2)?;
    Some(u16::from_be_bytes([o[0], o[1]]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    let o = b.get(i..i + 4)?;
    Some(u32::from_be_bytes([o[0], o[1], o[2], o[3]]))
}

/// name 表定位：返回 (offset, length)。
fn name_table(font: &[u8]) -> Option<(usize, usize)> {
    if font.len() < 12 {
        return None;
    }
    let num_tables = u16_at(font, 4)? as usize;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        let tag = u32_at(font, rec)?;
        if tag == u32::from_be_bytes(*b"name") {
            return Some((
                u32_at(font, rec + 8)? as usize,
                u32_at(font, rec + 12)? as usize,
            ));
        }
    }
    None
}

/// name 表记录。
struct NameRecord {
    platform: u16,
    encoding: u16,
    language: u16,
    name_id: u16,
    #[allow(dead_code)]
    length: u16,
    offset: u16,
}

fn name_records(font: &[u8]) -> Option<(usize, usize, Vec<NameRecord>)> {
    let (off, len) = name_table(font)?;
    if len < 6 {
        return None;
    }
    let t = &font[off..off + len];
    let count = u16_at(t, 2)? as usize;
    let str_off = u16_at(t, 4)? as usize;
    let mut recs = Vec::with_capacity(count);
    for i in 0..count {
        let r = 6 + i * 12;
        if r + 12 > t.len() {
            return None;
        }
        recs.push(NameRecord {
            platform: u16_at(t, r)?,
            encoding: u16_at(t, r + 2)?,
            language: u16_at(t, r + 4)?,
            name_id: u16_at(t, r + 6)?,
            length: u16_at(t, r + 8)?,
            offset: u16_at(t, r + 10)?,
        });
    }
    Some((off, str_off, recs))
}

fn record_string(font: &[u8], table_off: usize, str_off: usize, rec: &NameRecord) -> String {
    let start = table_off + str_off + rec.offset as usize;
    let end = start + rec.length as usize;
    let bytes = font.get(start..end).unwrap_or(&[]);
    if rec.platform == 3 {
        // Windows：UTF-16BE
        bytes
            .chunks(2)
            .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
            .collect::<Vec<u16>>()
            .iter()
            .map(|&u| char::from_u32(u as u32).unwrap_or('\u{FFFD}'))
            .collect()
    } else {
        // Mac Roman 等按字节直读
        bytes.iter().map(|&b| b as char).collect()
    }
}

/// 读字体族名（nameID=1；优先 Windows 英文记录，其次任意）。
pub fn family_name(font: &[u8]) -> Option<String> {
    let (table_off, str_off, recs) = name_records(font)?;
    let mut fallback = None;
    for rec in &recs {
        if rec.name_id == 1 {
            let s = record_string(font, table_off, str_off, rec);
            if rec.platform == 3 && rec.language == 0x409 {
                return Some(s);
            }
            fallback = fallback.or(Some(s));
        }
    }
    fallback
}

// ---------------------------------------------------------------------------
// 改名（OFL 修改版：族名避开原族名；0 号保留版权 + 追加修改声明）
// ---------------------------------------------------------------------------

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
}

fn encode_for(platform: u16, s: &str) -> Vec<u8> {
    if platform == 3 {
        utf16be(s)
    } else {
        s.bytes().collect()
    }
}

/// 改名并返回新字体字节。
///
/// - nameID 1/2/3/4/6 → 替换为 `family`/"Regular"/唯一 ID/全名/PostScript 名
/// - nameID 16/17（排版用子族）剔除，避免残留原族名
/// - nameID 0（版权）保留原文并追加 `notice`（修改声明 + 溯源）
///
/// 实现方式：重建 name 表追加到文件尾，更新目录项（含 checksum）。
pub fn rename_family(
    font: &[u8],
    family: &str,
    full_name: &str,
    ps_name: &str,
    notice: &str,
) -> Option<Vec<u8>> {
    let (table_off, str_off, recs) = name_records(font)?;
    let unique = format!("{full_name}; subset by trans-epub");

    // 重建：header(6) + records(12n) + 字符串区
    struct Out {
        platform: u16,
        encoding: u16,
        language: u16,
        name_id: u16,
        bytes: Vec<u8>,
    }
    let mut out = Vec::new();
    for rec in &recs {
        let original = record_string(font, table_off, str_off, rec);
        let bytes = match rec.name_id {
            0 => encode_for(rec.platform, &format!("{original}\n{notice}")),
            1 => encode_for(rec.platform, family),
            2 => encode_for(rec.platform, "Regular"),
            3 => encode_for(rec.platform, &unique),
            4 => encode_for(rec.platform, full_name),
            6 => encode_for(rec.platform, ps_name),
            16 | 17 => continue,
            _ => font[table_off + str_off + rec.offset as usize
                ..table_off + str_off + rec.offset as usize + rec.length as usize]
                .to_vec(),
        };
        out.push(Out {
            platform: rec.platform,
            encoding: rec.encoding,
            language: rec.language,
            name_id: rec.name_id,
            bytes,
        });
    }

    let count = out.len() as u16;
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    table.extend_from_slice(&count.to_be_bytes());
    let strings_at = 6 + out.len() * 12;
    table.extend_from_slice(&(strings_at as u16).to_be_bytes());
    let mut strings = Vec::new();
    for o in &out {
        table.extend_from_slice(&o.platform.to_be_bytes());
        table.extend_from_slice(&o.encoding.to_be_bytes());
        table.extend_from_slice(&o.language.to_be_bytes());
        table.extend_from_slice(&o.name_id.to_be_bytes());
        table.extend_from_slice(&(o.bytes.len() as u16).to_be_bytes());
        table.extend_from_slice(&(strings.len() as u16).to_be_bytes());
        strings.extend_from_slice(&o.bytes);
    }
    table.extend_from_slice(&strings);

    // checksum：u32 大端求和（补零到 4 字节倍数）
    let mut sum = 0u32;
    let padded = table.len() / 4 * 4 == table.len();
    let mut chk = table.clone();
    if !padded {
        chk.resize(table.len() + (4 - table.len() % 4), 0);
    }
    for c in chk.chunks_exact(4) {
        sum = sum.wrapping_add(u32::from_be_bytes([c[0], c[1], c[2], c[3]]));
    }

    let mut result = font.to_vec();
    let new_off = result.len();
    result.extend_from_slice(&table);
    // 目录项更新
    let num_tables = u16_at(font, 4)? as usize;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if u32_at(font, rec)? == u32::from_be_bytes(*b"name") {
            result[rec + 4..rec + 8].copy_from_slice(&sum.to_be_bytes());
            result[rec + 8..rec + 12].copy_from_slice(&(new_off as u32).to_be_bytes());
            result[rec + 12..rec + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
            break;
        }
    }
    Some(result)
}

// ---------------------------------------------------------------------------
// 子集化
// ---------------------------------------------------------------------------

/// 按字符集子集化（`subsetter`，glyph 级；char→glyph 经 ttf-parser）。
/// 返回的字体保留原 name 表——builtin 源随后须再过 [`rename_family`]。
pub fn subset_to_chars(font: &[u8], chars: &str) -> Option<Vec<u8>> {
    let face = ttf_parser::Face::parse(font, 0).ok()?;
    let mut gids: Vec<u16> = vec![0]; // .notdef 必留
    for c in chars.chars() {
        if let Some(g) = face.glyph_index(c) {
            gids.push(g.0);
        }
    }
    gids.sort_unstable();
    gids.dedup();
    // Profile::pdf：保留 cmap/head/hhea/hmtx/maxp/name/OS2/post + 轮廓表，
    // 剔 GSUB/GPOS（连字特性随之丢失——代码场景可接受，等宽逐字渲染）
    subsetter::subset(font, 0, subsetter::Profile::pdf(&gids)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VENDORED: &[u8] = include_bytes!("../assets/fonts/TransEpubMono-Regular.subset.ttf");

    #[test]
    fn family_name_reads_vendored() {
        assert_eq!(family_name(VENDORED).as_deref(), Some("TransEpub Mono"));
    }

    #[test]
    fn rename_roundtrip_readable() {
        let renamed = rename_family(
            VENDORED,
            "X Family",
            "X Family Regular",
            "XFamily-Regular",
            "mod-note",
        )
        .unwrap();
        assert_eq!(family_name(&renamed).as_deref(), Some("X Family"));
        // 0 号版权含修改声明
        let (toff, soff, recs2) = name_records(&renamed).unwrap();
        let copyright = recs2
            .iter()
            .find(|r| r.name_id == 0)
            .map(|r| record_string(&renamed, toff, soff, r))
            .unwrap();
        assert!(copyright.contains("mod-note"));
        assert!(copyright.contains("Maple Mono"), "保留上游版权行");
        assert!(copyright.contains("Maple Mono"), "保留上游版权行");
        // ttf-parser 可解析（结构有效）
        assert!(ttf_parser::Face::parse(&renamed, 0).is_ok());
    }

    #[test]
    fn subset_keeps_requested_glyphs() {
        // subsetter 0.1 的 cmap 原样保留（不重编 glyph id）：未请求字符仍可
        // 查到 gid，但轮廓已剔除——以包围盒判定"字形是否保留"。
        let text = "A一汉z";
        let sub = subset_to_chars(VENDORED, text).unwrap();
        assert!(sub.len() < VENDORED.len(), "子集应显著变小");
        let face = ttf_parser::Face::parse(&sub, 0).unwrap();
        for c in text.chars() {
            let gid = face.glyph_index(c).unwrap_or_else(|| panic!("缺失 {c}"));
            assert!(face.glyph_bounding_box(gid).is_some(), "字形被剔 {c}");
        }
        if let Some(gid) = face.glyph_index('鼠') {
            assert!(
                face.glyph_bounding_box(gid).is_none(),
                "未请求的汉字不应保留轮廓"
            );
        }
    }
}
