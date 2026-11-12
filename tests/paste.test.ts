/**
 * 粘贴 HTML 清洗逻辑测试
 * 验证：外部粘贴"文字 + 表格"时文字不丢失、白底/颜色等一切样式被彻底剥离
 */
import { describe, it, expect } from "vitest";

/** 与 NotesView 中 sanitizePastedHtml 相同的"提取式重建"逻辑（独立复刻以便单测） */
function sanitizePastedHtml(raw: string): string {
  const doc = new DOMParser().parseFromString(raw, "text/html");
  const body = doc.body;
  body.querySelectorAll("style, link, script, meta, title").forEach((n) => n.remove());

  const ALLOWED_ATTR = new Set(["colspan", "rowspan"]);
  function rebuild(el: Element): string {
    const tag = el.tagName.toLowerCase();
    let inner = "";
    for (const child of Array.from(el.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        inner += (child.textContent || "").replace(/</g, "&lt;");
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        inner += rebuild(child as Element);
      }
    }
    switch (tag) {
      case "table": return `<table>${inner}</table>`;
      case "thead": case "tbody": case "tfoot": case "tr":
        return `<${tag}>${inner}</${tag}>`;
      case "th": case "td": {
        let attrs = "";
        for (const a of ALLOWED_ATTR) {
          const v = el.getAttribute(a);
          if (v) attrs += ` ${a}="${v}"`;
        }
        return `<${tag}${attrs}>${inner || "<br>"}</${tag}>`;
      }
      case "img": {
        const src = el.getAttribute("src") || "";
        return src.startsWith("data:") ? `<img src="${src}">` : "";
      }
      case "br": return "<br>";
      case "p": {
        const t = inner.trim();
        return t ? `<p>${inner}</p>` : "";
      }
      case "div": {
        const t = inner.trim();
        if (!t) return "";
        if (/<(table|p|h[1-6]|ul|ol|blockquote|div)/i.test(inner)) return inner;
        return `<p>${inner}</p>`;
      }
      case "h1": case "h2": case "h3": case "h4": case "h5": case "h6":
        return inner.trim() ? `<${tag}>${inner}</${tag}>` : "";
      case "ul": return inner.trim() ? `<ul>${inner}</ul>` : "";
      case "ol": return inner.trim() ? `<ol>${inner}</ol>` : "";
      case "li": return inner.trim() ? `<li>${inner}</li>` : "";
      case "strong": case "b": return inner.trim() ? `<b>${inner}</b>` : "";
      case "em": case "i": return inner.trim() ? `<i>${inner}</i>` : "";
      case "u": return inner.trim() ? `<u>${inner}</u>` : "";
      case "code": return inner ? `<code>${inner}</code>` : "";
      case "blockquote": return inner.trim() ? `<blockquote>${inner}</blockquote>` : "";
      case "a": {
        const href = el.getAttribute("href") || "";
        return inner.trim() ? (href ? `<a href="${href}">${inner}</a>` : inner) : "";
      }
      case "span": case "font": case "section": case "article":
      case "header": case "footer": case "main":
        return inner;
      default:
        return inner;
    }
  }

  return Array.from(body.childNodes)
    .map((n) => {
      if (n.nodeType === Node.TEXT_NODE) return n.textContent || "";
      if (n.nodeType !== Node.ELEMENT_NODE) return "";
      return rebuild(n as Element);
    })
    .join("");
}

describe("粘贴 HTML 清洗", () => {
  it("带 <!--StartFragment--> 注释的完整 html/body 片段不崩溃且剥离背景", () => {
    const raw =
      '<html><body><!--StartFragment--><div style="background-color:#ffffff;color:#000000">' +
      '<p style="background:#fff">第一段<span style="background-color:white">白底文字</span></p>' +
      '<table style="background:#ffffff"><tbody><tr><td bgcolor="#ffffff" style="background:#f0f0f0">单元格A</td>' +
      '<td>单元格B</td></tr></tbody></table></div><!--EndFragment--></body></html>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).not.toContain("background");
    expect(clean).not.toContain("style=");
    expect(clean).not.toContain("bgcolor");
    expect(clean).not.toContain("color:");
    expect(clean).toContain("第一段");
    expect(clean).toContain("白底文字");
    expect(clean).toContain("单元格A");
    expect(clean).toContain("单元格B");
  });
  it("文字 + 表格：表格前的文字必须保留", () => {
    const raw =
      '<p>这是表格前的说明文字</p><table><tr><td>单元格1</td></tr></table>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).toContain("表格前的说明文字");
    expect(clean).toContain("单元格1");
    expect(clean).toContain("<table>");
  });

  it("彻底剥离内联 style / 背景色 / 颜色（白底问题）", () => {
    const raw =
      '<p style="background:#ffffff;color:#000">文字</p>' +
      '<table style="background:#fff"><tr><td style="background:#ddd">格</td></tr></table>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).not.toContain("background");
    expect(clean).not.toContain("style=");
    expect(clean).not.toContain("color:");
    expect(clean).toContain("文字");
    expect(clean).toContain("格");
  });

  it("移除 script/style 等非内容节点", () => {
    const raw = '<style>p{color:red}</style><p>正文</p><script>alert(1)</script>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).not.toContain("alert");
    expect(clean).not.toContain("<style");
    expect(clean).toContain("正文");
  });

  it("外链图片被移除，data: 图片保留", () => {
    const raw =
      '<img src="https://example.com/a.png" /><img src="data:image/png;base64,AAA" />';
    const clean = sanitizePastedHtml(raw);
    expect(clean).not.toContain("example.com");
    expect(clean).toContain("data:image/png");
  });

  it("Word 复制内容（含 mso 注释、span 样式）文字完整保留", () => {
    const raw =
      '<!--[if gte mso 9]><xml>xxx</xml><![endif]-->' +
      '<p><span style="font-size:12pt">标题文字</span></p>' +
      '<table><tr><th>表头</th></tr><tr><td>内容</td></tr></table>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).toContain("标题文字");
    expect(clean).toContain("表头");
    expect(clean).toContain("内容");
    expect(clean).not.toContain("font-size");
    expect(clean).not.toContain("mso");
  });

  it("div 包裹表格时不得转成 <p> 导致非法嵌套", () => {
    const raw =
      '<div><p>段落文字</p><table><tr><td>单元格</td></tr></table></div>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).toContain("段落文字");
    expect(clean).toContain("单元格");
    // 不得出现 <p> 直接包裹 <table> 的非法嵌套
    expect(clean).not.toMatch(/<p>\s*<table/);
  });

  it("纯文本段落正常转成 <p>", () => {
    const raw = '<div>直接文字内容</div>';
    const clean = sanitizePastedHtml(raw);
    expect(clean).toContain("<p>");
    expect(clean).toContain("直接文字内容");
  });
});

/**
 * 加载历史内容时的颜色/底色剥离逻辑测试
 * 验证：暗黑皮肤下旧内容的固定白底/黑字被剥离，让底色文字色跟随主题
 */
function stripColorStyles(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  doc.body.querySelectorAll("*").forEach((el) => {
    el.removeAttribute("bgcolor");
    el.removeAttribute("background");
    const style = el.getAttribute("style");
    if (!style) return;
    const kept = style
      .split(";")
      .map((s) => s.trim())
      .filter((s) => {
        if (!s) return false;
        const prop = s.split(":")[0].trim().toLowerCase();
        return !/^(background|background-color|color|bgcolor)$/.test(prop);
      });
    if (kept.length) el.setAttribute("style", kept.join("; ") + ";");
    else el.removeAttribute("style");
  });
  return doc.body.innerHTML;
}

describe("加载内容时剥离底色/颜色（暗黑主题适配）", () => {
  it("剥离 background / background-color / color / bgcolor", () => {
    const html =
      '<p style="background:#ffffff;color:#000000">白底黑字段落</p>' +
      '<table bgcolor="#ffffff"><tr><td style="background:#f5f5f5">单元格</td></tr></table>';
    const clean = stripColorStyles(html);
    expect(clean).not.toContain("background");
    expect(clean).not.toContain("bgcolor");
    expect(clean).not.toContain("color:");
    expect(clean).toContain("白底黑字段落");
    expect(clean).toContain("单元格");
  });

  it("保留非颜色类语义样式（对齐/缩进等）", () => {
    const html = '<p style="text-align:center;padding-left:10px">居中内容</p>';
    const clean = stripColorStyles(html);
    expect(clean).toContain("text-align:center");
    expect(clean).toContain("padding-left:10px");
    expect(clean).toContain("居中内容");
  });
});

