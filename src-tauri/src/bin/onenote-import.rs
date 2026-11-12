//! OneNote 迁移导入工具（一次性脚本，不进产品 UI）
//!
//! 规则（用户提供）：
//! - docx 文件名 = 笔记本（分类）名；文档第一段（第一个日期行之前）为文档标题，丢弃
//! - 「YYYY年M月D日」+「HH:MM」两行为页面分隔标志
//! - 时间行后的第一个非空段落 = 笔记标题
//! - 其后内容 = 正文（直到下一个日期行）
//!
//! 用法：
//!   onenote-import.exe [docx目录] [vault数据库路径] [--dry-run]
//!   默认目录 = F:\个人文件夹
//!   默认数据库 = %APPDATA%\com.securenote.app\securenote.vault
//! 运行时输入主密码；导入前自动备份数据库文件。
//! 防重：同（笔记本, 标题）已存在则跳过，可安全重复运行。

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use securenote_lib::crypto;
use securenote_lib::vault::{new_id, Vault};

// ============ docx 解析 ============

#[derive(Debug, Clone)]
struct Para {
    text: String,
    /// w:pStyle val："1"=heading1，"2"=heading2…
    style: Option<String>,
    /// 段内图片 rId（按出现顺序）
    image_rids: Vec<String>,
}

#[derive(Debug, Clone)]
enum Block {
    P(Para),
    /// 表格 → 简单 HTML
    Table(String),
}

fn local_name<'a>(node: &roxmltree::Node<'a, 'a>) -> &'a str {
    node.tag_name().name()
}

/// 读取 zip 内文本文件（UTF-8，容错）
fn read_zip_text<R: std::io::Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    use std::io::Read;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).to_string())
}

/// 提取段落：文本、样式、图片 rId
fn parse_para(p: roxmltree::Node) -> Para {
    let mut text = String::new();
    let mut style = None;
    let mut image_rids = Vec::new();
    for n in p.descendants() {
        match local_name(&n) {
            "pStyle" => {
                for attr in n.attributes() {
                    // 命名空间属性按本地名匹配（w:val）
                    if attr.name() == "val" {
                        style = Some(attr.value().to_string());
                    }
                }
            }
            "t" => text.push_str(n.text().unwrap_or("")),
            "tab" => text.push('\t'),
            "br" => text.push('\n'),
            "blip" => {
                // <a:blip r:embed="rId4"/>——本地名 embed
                for attr in n.attributes() {
                    if attr.name() == "embed" {
                        image_rids.push(attr.value().to_string());
                    }
                }
            }
            _ => {}
        }
    }
    Para {
        text,
        style,
        image_rids,
    }
}

/// 表格 → 简单 HTML <table>
fn parse_table(tbl: roxmltree::Node) -> String {
    let mut html = String::from("<table>");
    for tr in tbl.descendants().filter(|n| local_name(&n) == "tr") {
        html.push_str("<tr>");
        for tc in tr.children().filter(|n| local_name(&n) == "tc") {
            let mut cell = String::new();
            for p in tc.descendants().filter(|n| local_name(&n) == "p") {
                let t = parse_para(p).text.trim().to_string();
                if !t.is_empty() {
                    if !cell.is_empty() {
                        cell.push_str("<br>");
                    }
                    cell.push_str(&html_escape(&t));
                }
            }
            html.push_str(&format!("<td>{}</td>", cell));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    html
}

fn html_escape(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
}

fn is_date_line(s: &str) -> bool {
    let t = s.trim().replace('\u{00a0}', "").trim().to_string();
    // "2019年10月17日" → [2019, 10, 17, ""]；"2020年6月21日 星期日" → 尾部带星期后缀
    let parts: Vec<&str> = t.split(['年', '月', '日']).collect();
    if parts.len() != 4 {
        return false;
    }
    let tail = parts[3].trim();
    let tail_ok = tail.is_empty() || tail.starts_with("星期") || tail.starts_with("周");
    tail_ok
        && parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && !parts[1].is_empty()
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && !parts[2].is_empty()
        && parts[2].chars().all(|c| c.is_ascii_digit())
}

fn is_time_line(s: &str) -> bool {
    let t = s.trim().replace('\u{00a0}', "").trim().to_string();
    let parts: Vec<&str> = t.split(':').collect();
    parts.len() == 2
        && !parts[0].is_empty()
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[0].len() <= 2
        && parts[1].len() == 2
        && parts[1].chars().all(|c| c.is_ascii_digit())
}

fn is_blank(s: &str) -> bool {
    s.trim().replace('\u{00a0}', "").trim().is_empty()
}

#[derive(Debug)]
struct Page {
    date: Option<String>, // "2019年10月17日"
    time: Option<String>, // "11:33"
    title: String,
    body: Vec<Block>,
}

/// 解析段落/表格块序列 → 页面列表
/// 规则：
/// - 第一个非空段落 = 第一页的标题（OneNote 页面标题在文档顶部）
/// - 「日期行(+时间行)」开启新页；时间行后的第一个非空段 = 该页标题（若尚无标题）
/// - 文档开头（第一个日期行前）除首段外的其余非空块 → 第一页正文开头
fn split_pages(blocks: Vec<Block>) -> (Vec<Page>, usize) {
    let mut pages: Vec<Page> = Vec::new();
    let mut cur: Option<Page> = None;
    let mut expect_time = false;
    let mut expect_title = false;
    let mut header: Vec<Block> = Vec::new(); // 第一个日期行前的非空块
    let mut seen_first_date = false;
    let mut header_dropped = 0; // 无日期结构的文档：只报告不计（首段作标题）

    for block in blocks {
        match block {
            Block::P(p) => {
                let text = p.text.clone();
                if is_date_line(&text) {
                    seen_first_date = true;
                    if let Some(page) = cur.take() {
                        pages.push(page);
                    }
                    let mut page = Page {
                        date: Some(text.trim().to_string()),
                        time: None,
                        title: String::new(),
                        body: Vec::new(),
                    };
                    // 第一页：文档首段是页面标题，其余 header 块进正文开头
                    if pages.is_empty() && !header.is_empty() {
                        if let Some(Block::P(first)) = header.first().cloned() {
                            page.title = first.text.trim().to_string();
                        }
                        if header.len() > 1 {
                            page.body = header[1..].to_vec();
                        }
                        header.clear();
                    }
                    expect_title = page.title.is_empty();
                    cur = Some(page);
                    expect_time = true;
                    continue;
                }
                if expect_time && is_time_line(&text) {
                    if let Some(page) = cur.as_mut() {
                        page.time = Some(text.trim().to_string());
                    }
                    expect_time = false;
                    continue;
                }
                if is_blank(&text) && p.image_rids.is_empty() {
                    continue; // 跳过空段（含 &nbsp;）
                }
                if let Some(page) = cur.as_mut() {
                    // 标题必须是非空文本段；纯图片/空文本段直接进正文
                    if expect_title && page.title.is_empty() && !text.trim().is_empty() {
                        page.title = text.trim().to_string();
                        expect_title = false;
                        if p.image_rids.is_empty() {
                            continue; // 标题行本身不进正文
                        }
                    }
                    page.body.push(Block::P(p));
                } else {
                    // 第一个日期行之前：非空块存入 header（首段=标题候选）
                    header.push(Block::P(p));
                }
            }
            Block::Table(html) => {
                if let Some(page) = cur.as_mut() {
                    page.body.push(Block::Table(html));
                } else {
                    header.push(Block::Table(html));
                }
            }
        }
    }
    if let Some(mut page) = cur.take() {
        pages.push(page);
    }
    // 兜底：整个文档没有任何日期行 → header 全部作为单页内容（首段=标题）
    if pages.is_empty() && !header.is_empty() {
        let mut page = Page {
            date: None,
            time: None,
            title: String::new(),
            body: Vec::new(),
        };
        if let Some(Block::P(first)) = header.first().cloned() {
            page.title = first.text.trim().to_string();
        }
        if header.len() > 1 {
            page.body = header[1..].to_vec();
        }
        pages.push(page);
    }
    // 兜底：无标题页面用「日期 时间」作标题，不丢弃（如纯截图页面）
    for page in pages.iter_mut() {
        if page.title.is_empty() {
            page.title = match (&page.date, &page.time) {
                (Some(d), Some(t)) => format!("{} {}", d, t),
                (Some(d), None) => d.clone(),
                _ => "未命名".to_string(),
            };
        }
    }
    (pages, header_dropped)
}

fn mime_for(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

/// 渲染正文 HTML；图片字节通过 save_img 回调上传为附件，返回 snatt:// 引用
fn render_body(
    body: Vec<Block>,
    dek: &[u8],
    note_id: &str,
    conn: &rusqlite::Connection,
    rels: &HashMap<String, String>,
    zip: &mut zip::ZipArchive<std::io::BufReader<std::fs::File>>,
    doc_prefix: &str,
) -> Result<String, String> {
    let mut html = String::new();
    for block in body {
        match block {
            Block::Table(t) => html.push_str(&t),
            Block::P(p) => {
                // 图片 → 加密附件，snatt:// 引用
                let mut imgs = String::new();
                for rid in &p.image_rids {
                    let target = match rels.get(rid) {
                        Some(t) => t.clone(),
                        None => continue,
                    };
                    let media_path = format!("word/{}", target.trim_start_matches('/'));
                    let bytes = match zip.by_name(&media_path) {
                        Ok(mut r) => {
                            use std::io::Read;
                            let mut buf = Vec::new();
                            if r.read_to_end(&mut buf).is_err() {
                                continue;
                            }
                            buf
                        }
                        Err(_) => continue,
                    };
                    if bytes.len() > 100 * 1024 * 1024 {
                        continue; // 单图 100MB 上限，防呆
                    }
                    let mime = mime_for(&target);
                    let att_id = new_id();
                    let enc = crypto::encrypt(&bytes, dek).map_err(|e| e.to_string())?;
                    conn.execute(
                        "INSERT INTO attachments(id, note_id, mime, data, created_at)
                         VALUES(?1,?2,?3,?4,?5)",
                        rusqlite::params![
                            att_id,
                            note_id,
                            mime,
                            enc,
                            chrono::Local::now().to_rfc3339()
                        ],
                    )
                    .map_err(|e| e.to_string())?;
                    imgs.push_str(&format!(
                        "<img src=\"snatt://{}\" alt=\"{}\" style=\"max-width:100%;\">",
                        att_id, doc_prefix
                    ));
                }
                let text = p.text.trim().replace('\u{00a0}', " ").trim().to_string();
                if !text.is_empty() {
                    let esc = html_escape(&text);
                    match p.style.as_deref() {
                        Some("1") => html.push_str(&format!("<h2>{}</h2>", esc)),
                        Some("2") => html.push_str(&format!("<h3>{}</h3>", esc)),
                        _ => html.push_str(&format!("<p>{}</p>", esc)),
                    }
                }
                if !imgs.is_empty() {
                    html.push_str(&format!("<p>{}</p>", imgs));
                }
            }
        }
    }
    Ok(html)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut dry_run = false;
    let mut positional: Vec<String> = Vec::new();
    for a in args.iter().skip(1) {
        if a == "--dry-run" {
            dry_run = true;
        } else {
            positional.push(a.clone());
        }
    }
    let dir = positional
        .first()
        .cloned()
        .unwrap_or_else(|| "F:\\个人文件夹".to_string());
    let default_db = dirs::data_dir()
        .map(|d| d.join("com.securenote.app").join("securenote.vault"))
        .expect("无法定位应用数据目录");
    let db_path = positional
        .get(1)
        .map(PathBuf::from)
        .unwrap_or(default_db);

    println!("=== OneNote → 个人笔记本 导入工具 ===");
    println!("docx 目录: {}", dir);
    println!("数据库:    {}", db_path.display());
    if dry_run {
        println!("模式: DRY-RUN（只解析预览，不写库）");
    }

    if !Path::new(&dir).is_dir() {
        eprintln!("错误: 目录不存在: {}", dir);
        std::process::exit(1);
    }
    if !db_path.exists() {
        eprintln!("错误: 数据库不存在: {}", db_path.display());
        std::process::exit(1);
    }

    // 主密码：交互 tty 用不回显输入；管道输入（自动化）直接读一行
    use std::io::IsTerminal;
    let pwd = if std::io::stdin().is_terminal() {
        rpassword::prompt_password("请输入主密码: ").unwrap_or_default()
    } else {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).unwrap_or(0);
        s.trim().to_string()
    };
    if pwd.is_empty() {
        eprintln!("错误: 主密码为空");
        std::process::exit(1);
    }

    let data_dir = db_path.parent().unwrap().to_path_buf();
    let vault = Vault::new(data_dir.clone()).expect("打开库失败");
    match vault.unlock(&pwd) {
        Ok(true) => println!("✅ 主密码验证通过"),
        Ok(false) => {
            eprintln!("❌ 主密码错误");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("❌ 解锁失败: {e}");
            std::process::exit(1);
        }
    }
    let dek = vault.dek().expect("DEK 不可用");

    // 备份数据库文件
    if !dry_run {
        let bak = db_path.with_extension(format!(
            "vault.bak-import-{}",
            chrono::Local::now().format("%Y%m%d_%H%M%S")
        ));
        std::fs::copy(&db_path, &bak).expect("备份数据库失败");
        println!("📦 已备份数据库 → {}", bak.display());
    }

    let store = vault.store();
    let conn = store.conn_mut();

    // 防重集合：现有 (notebook, title)
    let mut existing: HashSet<(String, String)> = HashSet::new();
    {
        let mut stmt = conn
            .prepare("SELECT title, notebook FROM notes WHERE deleted_at IS NULL")
            .expect("查询现有笔记失败");
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .expect("query 失败");
        for row in rows {
            if let Ok((t, nb)) = row {
                if let (Ok(t), Ok(nb)) = (
                    crypto::decrypt_str(&t, dek.as_ref()),
                    crypto::decrypt_str(&nb, dek.as_ref()),
                ) {
                    existing.insert((nb.to_string(), t.to_string()));
                }
            }
        }
    }

    // 遍历 docx
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("读取目录失败")
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            p.extension().map_or(false, |x| x == "docx")
                && !name.starts_with("~$")
        })
        .collect();
    files.sort();

    let mut total_notes = 0usize;
    let mut total_imgs = 0usize;
    let mut total_skipped = 0usize;
    let mut failed_files: Vec<String> = Vec::new();

    for file in &files {
        let fname = file.file_name().unwrap().to_string_lossy().to_string();
        let notebook = fname.trim_end_matches(".docx").to_string();
        print!("📄 {} … ", fname);
        std::io::stdout().flush().ok();

        let result = (|| -> Result<(usize, usize, usize, Vec<String>, Vec<String>), String> {
            let f = std::fs::File::open(file).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(std::io::BufReader::new(f))
                .map_err(|e| format!("不是有效 docx: {e}"))?;

            let doc_xml = read_zip_text(&mut zip, "word/document.xml")
                .ok_or("缺少 document.xml")?;
            let rels_xml = read_zip_text(&mut zip, "word/_rels/document.xml.rels");

            // rels: rId → media 目标
            let mut rels: HashMap<String, String> = HashMap::new();
            if let Some(rx) = rels_xml {
                if let Ok(doc) = roxmltree::Document::parse(&rx) {
                    for n in doc.descendants() {
                        if local_name(&n) == "Relationship" {
                            if let (Some(id), Some(target)) =
                                (n.attribute("Id"), n.attribute("Target"))
                            {
                                if target.contains("media/") {
                                    rels.insert(id.to_string(), target.to_string());
                                }
                            }
                        }
                    }
                }
            }

            let doc = roxmltree::Document::parse(&doc_xml)
                .map_err(|e| format!("XML 解析失败: {e}"))?;
            let body = doc
                .descendants()
                .find(|n| local_name(&n) == "body")
                .ok_or("缺少 body")?;

            let mut blocks: Vec<Block> = Vec::new();
            for child in body.children() {
                match local_name(&child) {
                    "p" => blocks.push(Block::P(parse_para(child))),
                    "tbl" => blocks.push(Block::Table(parse_table(child))),
                    _ => {}
                }
            }

            let (pages, header_dropped) = split_pages(blocks);
            let mut created = 0usize;
            let mut imgs = 0usize;
            let mut skipped = 0usize;
            let mut skipped_empty: Vec<String> = Vec::new();
            let mut skipped_dup: Vec<String> = Vec::new();

            for page in &pages {
                if page.title.is_empty() {
                    skipped += 1;
                    skipped_empty.push(format!(
                        "[{} {}]",
                        page.date.clone().unwrap_or_default(),
                        page.time.clone().unwrap_or_default()
                    ));
                    continue;
                }
                if existing.contains(&(notebook.clone(), page.title.clone())) {
                    skipped += 1;
                    skipped_dup.push(page.title.clone());
                    continue;
                }
                let note_id = new_id();
                if dry_run {
                    created += 1;
                    continue;
                }
                let html = render_body(
                    page.body.clone(),
                    dek.as_ref(),
                    &note_id,
                    &conn,
                    &rels,
                    &mut zip,
                    &page.title,
                )?;
                let img_count = html.matches("snatt://").count();
                imgs += img_count;

                // 时间戳：解析页面日期+时间（本地时区）
                let (created_at, updated_at) = match (&page.date, &page.time) {
                    (Some(d), Some(t)) => {
                        let dt = parse_cn_datetime(d, t);
                        (dt.clone(), dt)
                    }
                    (Some(d), None) => {
                        let dt = parse_cn_datetime(d, "00:00");
                        (dt.clone(), dt)
                    }
                    _ => {
                        let now = chrono::Local::now().to_rfc3339();
                        (now.clone(), now)
                    }
                };

                conn.execute(
                    "INSERT INTO notes(id, title, content, notebook, favorite, created_at, updated_at, deleted_at)
                     VALUES(?1,?2,?3,?4,0,?5,?6,NULL)",
                    rusqlite::params![
                        note_id,
                        crypto::encrypt_str(&page.title, dek.as_ref()).map_err(|e| e.to_string())?,
                        crypto::encrypt_str(&html, dek.as_ref()).map_err(|e| e.to_string())?,
                        crypto::encrypt_str(&notebook, dek.as_ref()).map_err(|e| e.to_string())?,
                        created_at,
                        updated_at,
                    ],
                )
                .map_err(|e| e.to_string())?;
                existing.insert((notebook.clone(), page.title.clone()));
                created += 1;
            }
            let _ = header_dropped;
            Ok((created, imgs, skipped, skipped_empty, skipped_dup))
        })();

        match result {
            Ok((c, i, s, se, sd)) => {
                println!(
                    "✅ {} 页{}{}",
                    c,
                    if i > 0 { format!("，{} 图", i) } else { String::new() },
                    if s > 0 { format!("，跳过 {}", s) } else { String::new() }
                );
                if !se.is_empty() {
                    println!("   ⚠ 无标题页面: {}", se.join(" "));
                }
                if !sd.is_empty() {
                    println!(
                        "   ⚠ 标题重复跳过: {}",
                        sd.iter().take(5).cloned().collect::<Vec<_>>().join(" | ")
                    );
                }
                total_notes += c;
                total_imgs += i;
                total_skipped += s;
            }
            Err(e) => {
                println!("❌ {}", e);
                failed_files.push(format!("{}: {}", fname, e));
            }
        }
    }

    println!();
    println!("=== 完成 ===");
    if dry_run {
        println!("DRY-RUN 预览：共 {} 页可导入，{} 页跳过（重复/无标题）", total_notes, total_skipped);
        println!("确认无误后去掉 --dry-run 真正执行");
    } else {
        println!(
            "共导入 {} 条笔记，{} 张图片附件；跳过 {} 页（重复/无标题）",
            total_notes, total_imgs, total_skipped
        );
    }
    if !failed_files.is_empty() {
        println!("失败文件 {} 个:", failed_files.len());
        for f in &failed_files {
            println!("  ❌ {}", f);
        }
    }
    let _ = std::io::stdout().flush();
}

/// "2019年10月17日" + "11:33" → RFC3339（本地时区）
fn parse_cn_datetime(date: &str, time: &str) -> String {
    use chrono::TimeZone;
    let parts: Vec<&str> = date.split(['年', '月', '日']).collect();
    if parts.len() < 3 {
        return chrono::Local::now().to_rfc3339();
    }
    let y: i32 = parts[0].parse().unwrap_or(2000);
    let mo: u32 = parts[1].parse().unwrap_or(1);
    let d: u32 = parts[2].parse().unwrap_or(1);
    let tp: Vec<&str> = time.split(':').collect();
    let h: u32 = tp.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let mi: u32 = tp.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    chrono::Local
        .with_ymd_and_hms(y, mo, d, h, mi, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| chrono::Local::now().to_rfc3339())
}
