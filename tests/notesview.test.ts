/**
 * NotesView 组件级回归测试：点笔记后编辑器必须显示内容
 * （复现用户反馈"点了笔记后没有内容"）
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import NotesView from "../src/views/NotesView.vue";

vi.mock("../src/api", () => {
  const store = [
    {
      id: "n1",
      title: "富文本笔记",
      content: "<h1>标题一</h1><p>这是富文本内容 ABC</p>",
      notebook: "默认",
      favorite: false,
      created_at: "2026-11-10T10:00:00Z",
      updated_at: "2026-11-10T10:00:00Z",
      deleted_at: null,
    },
    {
      id: "n2",
      title: "第二篇",
      content: "<p>第二篇内容 XYZ</p>",
      notebook: "默认",
      favorite: false,
      created_at: "2026-11-10T10:01:00Z",
      updated_at: "2026-11-10T10:01:00Z",
      deleted_at: null,
    },
  ];
  return {
    notes: {
      list: vi.fn(async () => JSON.parse(JSON.stringify(store))),
      get: vi.fn(async (id: string) => store.find((n) => n.id === id)),
      create: vi.fn(async (title: string, notebook: string) => ({
        id: "n" + Math.random().toString(36).slice(2),
        title,
        content: "",
        notebook,
        favorite: false,
        created_at: "",
        updated_at: "",
        deleted_at: null,
      })),
      update: vi.fn(async (id: string, title: string, content: string, notebook?: string) => {
        const n = store.find((x) => x.id === id)!;
        n.title = title;
        n.content = content;
        if (notebook) n.notebook = notebook;
        return { ...n };
      }),
      remove: vi.fn(async () => {}),
      toggleFavorite: vi.fn(async () => {}),
      restore: vi.fn(async () => {}),
    },
    pickDocFile: vi.fn(),
    saveTextFile: vi.fn(),
    saveDataFile: vi.fn(),
    vault: { exportToFile: vi.fn(), importFromFile: vi.fn() },
    auth: { changeMasterPassword: vi.fn() },
    setContentProtection: vi.fn(),
    todos: {},
    passwords: {},
    trash: {},
  };
});

describe("NotesView：点笔记显示内容", () => {
  beforeEach(() => {
    localStorage.clear();
    // 屏蔽拖拽相关的布局 API 调用
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it("初始自动选中第一篇且编辑器有内容", async () => {
    const wrapper = mount(NotesView, {
      global: { provide: { appConfirm: vi.fn(async () => true) } },
      attachTo: document.body,
    });
    await flushPromises();
    const editor = wrapper.find(".rich-editor");
    expect(editor.exists()).toBe(true);
    expect(editor.element.innerHTML).toContain("富文本内容");
    wrapper.unmount();
  });

  it("点击第二篇笔记后编辑器切换为该篇内容", async () => {
    const wrapper = mount(NotesView, {
      global: { provide: { appConfirm: vi.fn(async () => true) } },
      attachTo: document.body,
    });
    await flushPromises();
    const items = wrapper.findAll(".list-panel .list-item");
    expect(items.length).toBe(2);
    await items[1].trigger("click");
    await flushPromises();
    const editor = wrapper.find(".rich-editor");
    expect(editor.element.innerHTML).toContain("第二篇内容");
    // 标题也切换
    const title = wrapper.find(".title-input").element as HTMLInputElement;
    expect(title.value).toBe("第二篇");
    wrapper.unmount();
  });
});
