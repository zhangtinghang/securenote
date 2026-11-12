/**
 * 版本比较与更新检测逻辑测试
 */
import { describe, it, expect } from "vitest";

function compareVersion(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x - y;
  }
  return 0;
}

function hasUpdate(latest: string, current: string): boolean {
  return compareVersion(latest, current) > 0;
}

function parseRelease(data: any): { version: string; notes: string; url: string } | null {
  const tag = (data?.tag_name || "").toString();
  const version = tag.replace(/^v/i, "");
  if (!version) return null;
  return {
    version,
    notes: (data?.body || "").toString(),
    url: (data?.html_url || "").toString(),
  };
}

describe("版本比较", () => {
  it("标准语义化版本比较", () => {
    expect(compareVersion("1.1.0", "1.0.0")).toBeGreaterThan(0);
    expect(compareVersion("1.0.1", "1.0.0")).toBeGreaterThan(0);
    expect(compareVersion("1.0.0", "1.0.0")).toBe(0);
    expect(compareVersion("1.0.0", "1.1.0")).toBeLessThan(0);
  });

  it("不同位数版本号（2.0 vs 2.0.1）", () => {
    expect(compareVersion("2.0.1", "2.0")).toBeGreaterThan(0);
    expect(compareVersion("2.0", "2.0.1")).toBeLessThan(0);
  });

  it("hasUpdate 判定", () => {
    expect(hasUpdate("1.1.0", "1.0.0")).toBe(true);
    expect(hasUpdate("1.0.0", "1.0.0")).toBe(false);
    expect(hasUpdate("0.9.0", "1.0.0")).toBe(false);
  });

  it("parseRelease 剥离 v 前缀", () => {
    const info = parseRelease({ tag_name: "v1.2.0", body: "更新日志", html_url: "https://x" });
    expect(info?.version).toBe("1.2.0");
    expect(info?.notes).toBe("更新日志");
  });
});
