/**
 * 更新检测配置与逻辑
 * 说明：应用无自建服务器，构建包发布到 GitHub Release，
 *      启动时请求 GitHub Releases API 检测是否有新版本。
 */

/** GitHub 仓库地址（owner/repo 形式）。发布到 Release 前请确认此值正确。 */
export const GITHUB_REPO = "zhangtinghang/securenote";

/** 当前应用版本号（与 Cargo.toml 保持一致，构建后由 Rust 端提供真实值） */
export const CURRENT_VERSION = "1.2.0";

/** GitHub Releases API 地址（latest release） */
export function latestReleaseUrl(): string {
  return `https://api.github.com/repos/${GITHUB_REPO}/releases/latest`;
}

/** 最新版本对应的 Release 页面（供用户手动下载） */
export function releasesPageUrl(): string {
  return `https://github.com/${GITHUB_REPO}/releases/latest`;
}

export interface UpdateInfo {
  /** 最新版本号（不含 v 前缀） */
  version: string;
  /** 发布说明/更新日志 */
  notes: string;
  /** 下载页面地址 */
  url: string;
}

/** 解析 GitHub latest release 响应 */
export function parseRelease(data: any): UpdateInfo | null {
  const tag = (data?.tag_name || "").toString();
  const version = tag.replace(/^v/i, "");
  if (!version) return null;
  return {
    version,
    notes: (data?.body || "").toString(),
    url: (data?.html_url || releasesPageUrl()).toString(),
  };
}

/**
 * 比较两个语义化版本号。
 * 返回 >0 表示 a 更新，<0 表示 b 更新，0 相等。
 */
export function compareVersion(a: string, b: string): number {
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

/** 是否发现可用的新版本 */
export function hasUpdate(latest: string, current: string): boolean {
  return compareVersion(latest, current) > 0;
}

/**
 * 检查更新：请求 GitHub latest release，返回新版本信息；无更新或失败返回 null。
 */
export async function checkForUpdate(current: string): Promise<UpdateInfo | null> {
  try {
    const res = await fetch(latestReleaseUrl(), {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!res.ok) return null;
    const data = await res.json();
    const info = parseRelease(data);
    if (!info) return null;
    if (hasUpdate(info.version, current)) return info;
    return null;
  } catch (e) {
    // 静默失败：无网络/被限流时不打扰用户
    console.warn("检查更新失败", e);
    return null;
  }
}
