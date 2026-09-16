/** YouTube 链接识别：首页导入框区分视频链接与频道链接 */

export type LinkKind = "video" | "channel";

/** 识别输入链接类型；无法识别返回 null。裸 @handle 视为频道。 */
export function linkKind(input: string): LinkKind | null {
  const s = input.trim();
  if (!s) return null;
  if (/^@[\w.-]+$/.test(s)) return "channel";
  if (!/^https?:\/\//i.test(s)) return null;
  if (/(?:youtube\.com\/(?:watch|shorts)|youtu\.be\/)/i.test(s)) return "video";
  if (/youtube\.com\/(?:@|channel\/|c\/|user\/)/i.test(s)) return "channel";
  return null;
}

/** 归一化频道输入为完整 URL（裸 @handle 补全协议） */
export function normalizeChannelUrl(input: string): string {
  const s = input.trim();
  if (/^@[\w.-]+$/.test(s)) return `https://www.youtube.com/${s}`;
  return s;
}

/** 中文计数格式化：1130000 → 113万，8500 → 8500 */
export function fmtCount(n: number | null | undefined): string {
  if (n == null) return "";
  if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(1).replace(/\.0$/, "")}亿`;
  if (n >= 10_000) return `${(n / 10_000).toFixed(1).replace(/\.0$/, "")}万`;
  return String(n);
}
