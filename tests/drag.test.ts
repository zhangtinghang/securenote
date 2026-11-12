/**
 * 拖拽排序插入点修正算法测试
 *
 * 索引语义：indexFromY 返回的 target 是"渲染列表（含被拖项）"的插入点（0..n）。
 * 先 splice 移除被拖项后，若 target > from，实际插入索引需 -1 修正，
 * 否则向下拖动时位置逐格偏后，表现为"只能拖到最后一行"。
 * 此测试固化 NotesView / TodosView 共 5 处拖拽使用的统一修正逻辑。
 */
import { describe, it, expect } from "vitest";

/** 与组件内 onXxxPointerMove 相同的重排算法 */
function reorder<T>(arr: T[], from: number, target: number): T[] {
  const a = [...arr];
  const [moved] = a.splice(from, 1);
  const to = target > from ? target - 1 : target;
  a.splice(Math.max(0, Math.min(to, a.length)), 0, moved);
  return a;
}

describe("拖拽排序：插入点 -1 修正", () => {
  const L = ["A", "B", "C", "D", "E"];

  it("第一项拖到中间（C 后半）→ 落在 C 与 D 之间", () => {
    // idx=3（clientY 越过 A/B/C 中点），修正后插到移除列表 index 2
    expect(reorder(L, 0, 3)).toEqual(["B", "C", "A", "D", "E"]);
  });

  it("第一项拖到最底部（idx=n）→ 到最后一行", () => {
    expect(reorder(L, 0, 5)).toEqual(["B", "C", "D", "E", "A"]);
  });

  it("第一项拖到第二项前半 → 位置不变", () => {
    expect(reorder(L, 0, 1)).toEqual(["A", "B", "C", "D", "E"]);
  });

  it("第一项拖到第二项后半 → 交换到 B 后", () => {
    expect(reorder(L, 0, 2)).toEqual(["B", "A", "C", "D", "E"]);
  });

  it("最后一项拖到中间（B 前半）→ 插到 A 与 B 之间", () => {
    expect(reorder(L, 4, 1)).toEqual(["A", "E", "B", "C", "D"]);
  });

  it("最后一项拖到最顶部 → 到第一行", () => {
    expect(reorder(L, 4, 0)).toEqual(["E", "A", "B", "C", "D"]);
  });

  it("中间项向上拖 → 无 -1 修正，直接插入", () => {
    expect(reorder(L, 2, 0)).toEqual(["C", "A", "B", "D", "E"]);
  });

  it("中间项向下拖 → -1 修正后落位正确", () => {
    // B 拖到 E 前半（idx=4）→ D 与 E 之间
    expect(reorder(L, 1, 4)).toEqual(["A", "C", "D", "B", "E"]);
  });

  it("中间项原位附近 → 不动", () => {
    // C(idx2) 鼠标在 B 后半 idx=2 == from，组件层直接 return；此处验证边界
    expect(reorder(L, 2, 3)).toEqual(["A", "B", "C", "D", "E"]);
  });
});
