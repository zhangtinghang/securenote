import { describe, it, expect } from "vitest";
import {
  resolveMerge,
  validatePayloadFormat,
  passwordStrength,
  validateGeneratedPassword,
  type SyncItem,
} from "../src/logic";

describe("增量同步冲突解析 resolveMerge", () => {
  const base: SyncItem = {
    id: "a",
    updated_at: "2026-11-09T10:00:00Z",
    deleted_at: null,
  };

  it("远程不存在于本地时应新增", () => {
    const local: SyncItem[] = [];
    const remote: SyncItem[] = [base];
    const r = resolveMerge(local, remote);
    expect(r.upsert).toHaveLength(1);
    expect(r.upsert[0].id).toBe("a");
  });

  it("远程更新时间更晚时应覆盖本地", () => {
    const local: SyncItem[] = [{ ...base, updated_at: "2026-11-09T09:00:00Z" }];
    const remote: SyncItem[] = [{ ...base, updated_at: "2026-11-09T11:00:00Z" }];
    const r = resolveMerge(local, remote);
    expect(r.upsert).toHaveLength(1);
    expect(r.keepLocal).toHaveLength(0);
  });

  it("本地更新时间更晚时应保留本地", () => {
    const local: SyncItem[] = [{ ...base, updated_at: "2026-11-09T12:00:00Z" }];
    const remote: SyncItem[] = [{ ...base, updated_at: "2026-11-09T11:00:00Z" }];
    const r = resolveMerge(local, remote);
    expect(r.upsert).toHaveLength(0);
    expect(r.keepLocal).toHaveLength(1);
  });

  it("时间相同时保留本地（稳定策略）", () => {
    const local: SyncItem[] = [{ ...base }];
    const remote: SyncItem[] = [{ ...base }];
    const r = resolveMerge(local, remote);
    expect(r.upsert).toHaveLength(0);
    expect(r.keepLocal).toHaveLength(1);
  });
});

describe("导出包格式校验 validatePayloadFormat", () => {
  it("合法格式应通过", () => {
    const payload = {
      format: "securenote-vault",
      version: 1,
      payload: { notes: [], todos: [], passwords: [] },
    };
    expect(validatePayloadFormat(payload)).toBe(true);
  });

  it("错误 format 应拒绝", () => {
    const payload = { format: "other", version: 1, payload: {} };
    expect(validatePayloadFormat(payload)).toBe(false);
  });

  it("缺少数组字段应拒绝", () => {
    const payload = { format: "securenote-vault", version: 1, payload: { notes: [] } };
    expect(validatePayloadFormat(payload)).toBe(false);
  });
});

describe("密码强度评分 passwordStrength", () => {
  it("空密码为 0", () => {
    expect(passwordStrength("")).toBe(0);
  });
  it("短密码不满足长度", () => {
    expect(passwordStrength("abc")).toBe(0);
  });
  it("仅数字长度8 得2分（长度1 + 含数字1）", () => {
    expect(passwordStrength("12345678")).toBe(2);
  });
  it("大小写数字符号长度8 得4分", () => {
    expect(passwordStrength("Abc123!@")).toBe(4);
  });
  it("超12位强密码得5分封顶4", () => {
    expect(passwordStrength("Abcdef12345!@")).toBe(4);
  });
});

describe("生成密码校验 validateGeneratedPassword", () => {
  it("全字符集密码应满足所有类型", () => {
    const pwd = "Ab3$defGh1!klmN";
    expect(validateGeneratedPassword(pwd, true, true, true, true)).toBe(true);
  });
  it("不含大写时 useUpper 应失败", () => {
    const pwd = "abcdef12345";
    expect(validateGeneratedPassword(pwd, true, true, true, false)).toBe(false);
  });
  it("过短密码应失败", () => {
    expect(validateGeneratedPassword("abc", false, true, false, false)).toBe(false);
  });
});
