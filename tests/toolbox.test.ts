import { describe, it, expect } from "vitest";
import {
  tsToDate,
  dateToTs,
  formatLocal,
  ipToInt,
  intToIp,
  cidrToMask,
  maskToCidr,
  calcIp,
  commonPrefixCidr,
  hostsToCidr,
  ipToHex,
  hexToIp,
  completeIp,
} from "../src/logic";

describe("时间戳转换", () => {
  it("10 位秒时间戳自动识别", () => {
    const r = tsToDate("1762752000");
    expect(r).not.toBeNull();
    expect(r!.sec).toBe(1762752000);
    expect(r!.ms).toBe(1762752000000);
    expect(r!.local).toBe(formatLocal(new Date(1762752000000)));
    expect(r!.iso).toBe(new Date(1762752000000).toISOString());
  });

  it("13 位毫秒时间戳自动识别", () => {
    const r = tsToDate("1762752000000");
    expect(r).not.toBeNull();
    expect(r!.ms).toBe(1762752000000);
    expect(r!.sec).toBe(1762752000);
  });

  it("非法输入返回 null", () => {
    expect(tsToDate("123")).toBeNull();
    expect(tsToDate("12345678901")).toBeNull();
    expect(tsToDate("abc")).toBeNull();
    expect(tsToDate("")).toBeNull();
  });

  it("日期时间 → 时间戳", () => {
    const r = dateToTs("2026-01-02T03:04");
    expect(r).not.toBeNull();
    expect(r!.sec).toBe(Math.floor(new Date("2026-01-02T03:04").getTime() / 1000));
    expect(dateToTs("")).toBeNull();
    expect(dateToTs("not-a-date")).toBeNull();
  });
});

describe("掩码 / IP 转换", () => {
  it("ipToInt 与 intToIp 互逆", () => {
    expect(ipToInt("192.168.1.1")).toBe(3232235777);
    expect(intToIp(3232235777)).toBe("192.168.1.1");
    expect(ipToInt("256.1.1.1")).toBeNull();
    expect(ipToInt("1.2.3")).toBeNull();
    expect(ipToInt("a.b.c.d")).toBeNull();
  });

  it("CIDR 与点分掩码互转", () => {
    expect(cidrToMask(24)).toBe("255.255.255.0");
    expect(cidrToMask(8)).toBe("255.0.0.0");
    expect(cidrToMask(0)).toBe("0.0.0.0");
    expect(cidrToMask(32)).toBe("255.255.255.255");
    expect(maskToCidr("255.255.255.0")).toBe(24);
    expect(maskToCidr("255.0.0.0")).toBe(8);
    expect(maskToCidr("255.0.255.0")).toBeNull(); // 不连续的掩码非法
  });

  it("/24 网段计算", () => {
    const r = calcIp("192.168.1.100", "24");
    expect(r).not.toBeNull();
    expect(r!.network).toBe("192.168.1.0");
    expect(r!.broadcast).toBe("192.168.1.255");
    expect(r!.firstHost).toBe("192.168.1.1");
    expect(r!.lastHost).toBe("192.168.1.254");
    expect(r!.hostCount).toBe(254);
    expect(r!.total).toBe(256);
    expect(r!.mask).toBe("255.255.255.0");
    expect(r!.wildcard).toBe("0.0.0.255");
    expect(r!.isPrivate).toBe(true);
    expect(r!.ipClass).toBe("C 类");
  });

  it("点分掩码输入与公网地址", () => {
    const r = calcIp("8.8.8.8", "255.255.255.0");
    expect(r).not.toBeNull();
    expect(r!.cidr).toBe(24);
    expect(r!.network).toBe("8.8.8.0");
    expect(r!.isPrivate).toBe(false);
  });

  it("/16 与非法输入", () => {
    const r = calcIp("10.0.0.5", "255.255.0.0");
    expect(r!.network).toBe("10.0.0.0");
    expect(r!.broadcast).toBe("10.0.255.255");
    expect(r!.hostCount).toBe(65534);
    expect(calcIp("1.2.3.4", "33")).toBeNull();
    expect(calcIp("bad-ip", "24")).toBeNull();
  });

  it("IP 段 → 最小掩码", () => {
    expect(commonPrefixCidr("192.168.1.10", "192.168.1.250")).toBe(24);
    expect(commonPrefixCidr("10.0.0.1", "10.0.0.1")).toBe(32);
    expect(commonPrefixCidr("0.0.0.0", "255.255.255.255")).toBe(0);
    expect(commonPrefixCidr("bad", "1.2.3.4")).toBeNull();
    const r = calcIp("192.168.1.10", String(commonPrefixCidr("192.168.1.10", "192.168.1.250")!));
    expect(r!.network).toBe("192.168.1.0");
  });

  it("所需 IP 数量 → 最小掩码", () => {
    expect(hostsToCidr(50)!.cidr).toBe(26); // /26 = 64 块 62 可用
    expect(hostsToCidr(62)!.cidr).toBe(26);
    expect(hostsToCidr(63)!.cidr).toBe(25);
    expect(hostsToCidr(1)!.cidr).toBe(32);
    expect(hostsToCidr(2)!.cidr).toBe(31);
    expect(hostsToCidr(0)).toBeNull();
    expect(hostsToCidr(-5)).toBeNull();
  });

  it("十六进制 ↔ 点分十进制", () => {
    expect(ipToHex("192.168.1.1")).toBe("C0A80101");
    expect(ipToHex("0.0.0.0")).toBe("00000000");
    expect(ipToHex("bad")).toBeNull();
    expect(hexToIp("C0A80101")).toBe("192.168.1.1");
    expect(hexToIp("0xFFFFFF00")).toBe("255.255.255.0");
    expect(hexToIp("ABCDEF")).toBe("0.171.205.239"); // 补齐为 0x00ABCDEF
  });

  it("点分十进制补全", () => {
    expect(completeIp("192.168")).toBe("192.168.0.0");
    expect(completeIp("10.1.2")).toBe("10.1.2.0");
    expect(completeIp("192.168.1.")).toBe("192.168.1.0");
    expect(completeIp("192.168.1.1")).toBe("192.168.1.1");
    expect(completeIp("999.1")).toBeNull();
    expect(completeIp("1.2.3.4.5")).toBeNull();
  });
});
