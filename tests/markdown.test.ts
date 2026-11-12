import { describe, it, expect } from "vitest";
import { markdownToHtml, htmlToMarkdown, isHtmlContent } from "../src/logic";

describe("Markdown → HTML", () => {
  it("标题 h1/h2/h3", () => {
    const html = markdownToHtml("# 标题一\n## 标题二\n### 标题三");
    expect(html).toContain("<h1>标题一</h1>");
    expect(html).toContain("<h2>标题二</h2>");
    expect(html).toContain("<h3>标题三</h3>");
  });

  it("粗体、斜体、删除线、行内代码", () => {
    const html = markdownToHtml("**粗** *斜* ~~删~~ `code`");
    expect(html).toContain("<strong>粗</strong>");
    expect(html).toContain("<em>斜</em>");
    expect(html).toContain("<del>删</del>");
    expect(html).toContain("<code>code</code>");
  });

  it("无序与有序列表", () => {
    const html = markdownToHtml("- 甲\n- 乙\n\n1. 一\n2. 二").replace(/\s+/g, "");
    expect(html).toContain("<ul><li>甲</li><li>乙</li></ul>");
    expect(html).toContain("<ol><li>一</li><li>二</li></ol>");
  });

  it("任务列表", () => {
    const html = markdownToHtml("- [x] 完成\n- [ ] 未完成");
    expect(html).toContain("checked");
    expect(html).not.toMatch(/- \[ \]/);
  });

  it("表格", () => {
    const html = markdownToHtml("| A | B |\n| --- | --- |\n| 1 | 2 |");
    expect(html).toContain("<table>");
    expect(html).toContain("<th>A</th>");
    expect(html).toContain("<td>2</td>");
  });

  it("图片与链接", () => {
    const html = markdownToHtml("![截图](data:image/png;base64,xxx) [首页](https://a.com)");
    expect(html).toContain('<img src="data:image/png;base64,xxx" alt="截图">');
    expect(html).toContain('<a href="https://a.com">首页</a>');
  });

  it("代码块与引用", () => {
    const html = markdownToHtml("```\nlet a = 1;\n```\n> 引用文字");
    expect(html).toContain("<pre><code>");
    expect(html).toContain("let a = 1;");
    expect(html).toContain("<blockquote>引用文字</blockquote>");
  });
});

describe("HTML → Markdown", () => {
  it("标题与粗斜体", () => {
    const md = htmlToMarkdown("<h1>标题</h1><p><strong>粗</strong><em>斜</em></p>");
    expect(md).toContain("# 标题");
    expect(md).toContain("**粗**");
    expect(md).toContain("*斜*");
  });

  it("表格转换", () => {
    const md = htmlToMarkdown(
      "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>"
    );
    expect(md).toContain("| A | B |");
    expect(md).toContain("| 1 | 2 |");
    expect(md).toContain("| --- | --- |");
  });

  it("图片（dataURL）转换", () => {
    const md = htmlToMarkdown('<p><img src="data:image/png;base64,AAA" alt="截图"></p>');
    expect(md).toContain("![截图](data:image/png;base64,AAA)");
  });

  it("列表转换", () => {
    const md = htmlToMarkdown("<ul><li>甲</li><li>乙</li></ul>");
    expect(md).toContain("- 甲");
    expect(md).toContain("- 乙");
  });
});

describe("双向往返", () => {
  it("MD → HTML → MD 基本保持", () => {
    const src = "# 标题\n\n**粗体**和*斜体*";
    const html = markdownToHtml(src);
    const back = htmlToMarkdown(html);
    expect(back).toContain("# 标题");
    expect(back).toContain("**粗体**");
    expect(back).toContain("*斜体*");
  });
});

describe("内容格式判断 isHtmlContent", () => {
  it("以 < 开头视为 HTML", () => {
    expect(isHtmlContent("<p>富文本</p>")).toBe(true);
  });
  it("普通文本视为 Markdown", () => {
    expect(isHtmlContent("# 标题")).toBe(false);
    expect(isHtmlContent("")).toBe(false);
  });
});
