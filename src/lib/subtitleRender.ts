/**
 * 字幕 → 全帧透明 PNG（Canvas 2D 渲染）。
 * 用于 ffmpeg 无 libass 时的烧录兜底：每条 cue 渲染成与视频同分辨率的 PNG，
 * 由 ffmpeg overlay 滤镜链按时间窗叠到画面上。
 * 样式参数与 ass.ts 的 buildAss 同源（PlayResY=288 折算），字体走页面 @font-face。
 */
import { chunkCue, fontFamilyName, type AssCue, type AssStyleOptions } from "./ass";

export type RenderedCue = { start: number; end: number; png: number[] };

/** cue 列表（已按导出区间平移）按样式展开后的实际事件数（chunkCue 会切长句） */
export function renderedCueCount(cues: AssCue[]): number {
  return cues.flatMap((c) => chunkCue(c)).length;
}

const MARGIN_V: Record<string, number> = { bottom: 14, middle: 10, top: 28 };

export async function renderCuePngs(
  cues: AssCue[],
  st: AssStyleOptions,
  showZh: boolean,
  width: number,
  height: number,
): Promise<RenderedCue[]> {
  await document.fonts.ready;
  const scale = height / 288;
  const family = fontFamilyName(st.fontFamily);
  // 字号：buildAss 中 fontSize(px) * 1.6 = ASS pt（PlayResY 288），再按视频高折算
  const enPx = st.fontSize * 1.6 * scale;
  const zhPx = (enPx * st.zhScale) / 100;
  const gap = enPx * 0.18;
  const boxed = st.bgOpacity > 0;
  const pad = (boxed ? Math.max(st.shadow, 2) : st.shadow) * scale;
  const outlinePx = st.outline * scale;

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d")!;

  const fontOf = (px: number) =>
    `${st.italic ? "italic " : ""}${st.bold ? "bold " : ""}${Math.round(px)}px "${family}"`;
  try {
    // WebKit 17+ 支持；不支持则静默忽略
    (ctx as CanvasRenderingContext2D & { letterSpacing?: string }).letterSpacing =
      `${st.spacing * scale}px`;
  } catch {
    /* ignore */
  }

  const out: RenderedCue[] = [];
  for (const cue of cues.flatMap((c) => chunkCue(c))) {
    const lines: { text: string; px: number }[] = [
      { text: cue.text_en, px: enPx },
    ];
    if (showZh && cue.text_zh) lines.push({ text: cue.text_zh, px: zhPx });

    // 量块：逐行测宽，总高 = 行高和 + 行距
    let maxW = 0;
    for (const l of lines) {
      ctx.font = fontOf(l.px);
      maxW = Math.max(maxW, ctx.measureText(l.text).width);
    }
    const blockH = lines.reduce((a, l) => a + l.px, 0) + gap * (lines.length - 1);
    const padX = pad + enPx * 0.35; // 两端留白（对应 buildAss 的 \h\h 与底盒内边距）
    const boxW = maxW + padX * 2;
    const boxH = blockH + pad * 2;
    const cx = width / 2;
    let boxTop: number;
    if (st.position === "top") boxTop = MARGIN_V.top * scale;
    else if (st.position === "middle") boxTop = (height - boxH) / 2;
    else boxTop = height - MARGIN_V.bottom * scale - boxH;

    ctx.clearRect(0, 0, width, height);
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.lineJoin = "round";

    if (boxed) {
      ctx.save();
      ctx.globalAlpha = st.bgOpacity / 100;
      ctx.fillStyle = st.bgColor;
      const r = Math.max(pad * 0.9, 2);
      ctx.beginPath();
      ctx.roundRect(cx - boxW / 2, boxTop, boxW, boxH, r);
      ctx.fill();
      ctx.restore();
    }

    let lineCenter = boxTop + pad;
    for (const l of lines) {
      lineCenter += l.px / 2;
      ctx.font = fontOf(l.px);
      ctx.save();
      if (st.glow) {
        ctx.shadowColor = st.glow;
        ctx.shadowBlur = (st.outline + 3) * scale * 1.6;
      } else if (!boxed && st.shadow > 0) {
        ctx.shadowColor = "rgba(0,0,0,0.6)";
        ctx.shadowBlur = 0;
        ctx.shadowOffsetY = st.shadow * scale;
      }
      if (outlinePx > 0) {
        ctx.strokeStyle = st.outlineColor;
        ctx.lineWidth = outlinePx * 2;
        ctx.strokeText(l.text, cx, lineCenter);
      }
      ctx.fillStyle = st.color;
      ctx.fillText(l.text, cx, lineCenter);
      ctx.restore();
      lineCenter += l.px / 2 + gap;
    }

    const blob = await new Promise<Blob | null>((r) =>
      canvas.toBlob(r, "image/png"),
    );
    if (!blob) continue;
    out.push({
      start: cue.start_secs,
      end: cue.end_secs,
      png: Array.from(new Uint8Array(await blob.arrayBuffer())),
    });
  }
  return out;
}

/** 软字幕兜底：cues → SRT 文本（时间已平移） */
export function toSrt(cues: AssCue[], showZh: boolean): string {
  const t = (secs: number) => {
    const ms = Math.max(0, Math.round(secs * 1000));
    const h = Math.floor(ms / 3600000);
    const m = Math.floor((ms % 3600000) / 60000);
    const s = Math.floor((ms % 60000) / 1000);
    const r = ms % 1000;
    const p = (v: number, n: number) => String(v).padStart(n, "0");
    return `${p(h, 2)}:${p(m, 2)}:${p(s, 2)},${p(r, 3)}`;
  };
  return cues
    .map((c, i) => {
      const lines = [c.text_en];
      if (showZh && c.text_zh) lines.push(c.text_zh);
      return `${i + 1}\n${t(c.start_secs)} --> ${t(c.end_secs)}\n${lines.join("\n")}`;
    })
    .join("\n\n");
}
