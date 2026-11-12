/**
 * OneNote 分区 docx 按页拆分逻辑测试
 * 验证：以 h1 为分页符，第一个 h1（分区主标题）丢弃，后续每个 h1 拆成独立笔记
 */
import { describe, it, expect } from "vitest";

function splitDocxPages(html: string): Array<{ title: string; content: string }> {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const body = doc.body;
  const pages: Array<{ title: string; content: string }> = [];
  let currentTitle = "";
  const currentNodes: Node[] = [];
  let seenH1 = false;

  const flush = () => {
    const container = document.createElement("div");
    currentNodes.forEach((n) => container.appendChild(n.cloneNode(true)));
    const content = container.innerHTML.trim();
    if (content || currentTitle) {
      pages.push({ title: currentTitle.trim(), content });
    }
    currentNodes.length = 0;
  };

  for (const node of Array.from(body.childNodes)) {
    if (node.nodeType === Node.ELEMENT_NODE) {
      const el = node as Element;
      if (el.tagName.toLowerCase() === "h1") {
        if (!seenH1) {
          seenH1 = true;
          continue;
        }
        flush();
        currentTitle = el.textContent?.trim() || "";
        seenH1 = true;
        continue;
      }
    }
    currentNodes.push(node.cloneNode(true));
  }
  flush();

  if (pages.length === 0) {
    return [{ title: "", content: html }];
  }
  return pages.filter((p) => p.title || p.content.trim());
}

describe("OneNote docx 按页拆分", () => {
  it("第一个 h1（分区标题）+ 多个 h1 页面 → 拆成多条，分区标题被丢弃", () => {
    const html =
      "<h1>我的日记分区</h1>" +
      "<h1>2024-01-01</h1><p>今天天气很好</p>" +
      "<h1>2024-01-02</h1><p>今天读了本书</p>" +
      "<h1>2024-01-03</h1><p>旅行记录</p>";
    const pages = splitDocxPages(html);
    expect(pages.length).toBe(3);
    expect(pages[0].title).toBe("2024-01-01");
    expect(pages[0].content).toContain("今天天气很好");
    expect(pages[1].title).toBe("2024-01-02");
    expect(pages[2].title).toBe("2024-01-03");
    // 分区标题不应出现在任何笔记标题里
    expect(pages.every((p) => p.title !== "我的日记分区")).toBe(true);
  });

  it("只有一个 h1（无页面分隔）→ 整体作为单条", () => {
    const html = "<h1>单条笔记</h1><p>内容A</p><p>内容B</p>";
    const pages = splitDocxPages(html);
    expect(pages.length).toBe(1);
    expect(pages[0].content).toContain("内容A");
    expect(pages[0].content).toContain("内容B");
  });

  it("无 h1 → 整体作为单条", () => {
    const html = "<p>段落一</p><p>段落二</p>";
    const pages = splitDocxPages(html);
    expect(pages.length).toBe(1);
    expect(pages[0].content).toContain("段落一");
  });

  it("页面标题下的正文（含列表/段落）正确归入对应页", () => {
    const html =
      "<h1>分区</h1>" +
      "<h1>标题A</h1><ul><li>项目1</li></ul>" +
      "<h1>标题B</h1><p>段落</p><table><tr><td>格</td></tr></table>";
    const pages = splitDocxPages(html);
    expect(pages.length).toBe(2);
    expect(pages[0].title).toBe("标题A");
    expect(pages[0].content).toContain("项目1");
    expect(pages[1].title).toBe("标题B");
    expect(pages[1].content).toContain("段落");
    expect(pages[1].content).toContain("<table>");
  });
});
