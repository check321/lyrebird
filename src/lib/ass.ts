/** 字幕 cue → ASS 字幕文件内容，供 libass（JavascriptSubtitlesOctopus）渲染 */

export type AssCue = {
  start_secs: number;
  end_secs: number;
  text_en: string;
  text_zh: string | null;
};

export type AssStyleOptions = {
  fontSize: number; // px（按 PlayResY=288 折算）
  fontFamily: string; // FONT_NAMES 的 key
  color: string; // #RRGGBB
  bold: boolean;
  italic: boolean;
  spacing: number; // 字间距（ASS 单位）
  outline: number; // 描边宽度 0-4
  outlineColor: string;
  shadow: number; // 阴影深度 0-4
  glow: string | null; // 霓虹发光颜色（外层模糊描边），null 关闭
  bgColor: string;
  bgOpacity: number; // 0-100；0 表示无底盒（BorderStyle 1），>0 显示底盒（BorderStyle 4，颜色/透明度取 BackColour）
  position: "bottom" | "middle" | "top";
  zhScale: number; // 中文字号比例 %
};

export const DEFAULT_ASS_STYLE: AssStyleOptions = {
  fontSize: 15,
  fontFamily: "roboto",
  color: "#ffffff",
  bold: false,
  italic: false,
  spacing: 0,
  outline: 1,
  outlineColor: "#000000",
  shadow: 0,
  glow: null,
  bgColor: "#000000",
  bgOpacity: 70,
  position: "bottom",
  zhScale: 85,
};

/** CapCut 式预设模板 */
export const SUBTITLE_PRESETS: { name: string; style: AssStyleOptions }[] = [
  { name: "经典底盒", style: { ...DEFAULT_ASS_STYLE } },
  {
    name: "极简白字",
    style: {
      ...DEFAULT_ASS_STYLE,
      bgOpacity: 0,
      outline: 1,
      shadow: 1,
    },
  },
  {
    name: "综艺花字",
    style: {
      ...DEFAULT_ASS_STYLE,
      fontFamily: "noto",
      fontSize: 20,
      color: "#fde047",
      bold: true,
      outline: 3,
      outlineColor: "#1e293b",
      shadow: 1,
      bgOpacity: 0,
    },
  },
  {
    name: "霓虹辉光",
    style: {
      ...DEFAULT_ASS_STYLE,
      fontFamily: "nunito",
      fontSize: 17,
      color: "#ffffff",
      bold: true,
      glow: "#22d3ee",
      outline: 0,
      bgOpacity: 0,
    },
  },
  {
    name: "影院衬线",
    style: {
      ...DEFAULT_ASS_STYLE,
      fontFamily: "merriweather",
      color: "#faf3e0",
      italic: true,
      outline: 1,
      bgOpacity: 0,
      shadow: 1,
    },
  },
  {
    name: "手写便签",
    style: {
      ...DEFAULT_ASS_STYLE,
      fontFamily: "caveat",
      fontSize: 22,
      color: "#ffffff",
      outline: 0,
      shadow: 1,
      bgOpacity: 0,
    },
  },
  {
    name: "元气圆体",
    style: {
      ...DEFAULT_ASS_STYLE,
      fontFamily: "nunito",
      fontSize: 17,
      color: "#fda4af",
      bold: true,
      outline: 2,
      outlineColor: "#ffffff",
      bgOpacity: 0,
    },
  },
  {
    name: "白底清爽",
    style: {
      ...DEFAULT_ASS_STYLE,
      color: "#18181b",
      bgColor: "#ffffff",
      bgOpacity: 92,
      outline: 0,
    },
  },
];

const FONT_NAMES: Record<string, string> = {
  roboto: "Roboto",
  noto: "Noto Sans CJK SC",
  merriweather: "Merriweather",
  nunito: "Nunito",
  caveat: "Caveat",
};

export function styleFromSettings(s: Record<string, string>): AssStyleOptions {
  const d = DEFAULT_ASS_STYLE;
  return {
    fontSize: Number(s.sub_font_size) || d.fontSize,
    fontFamily: FONT_NAMES[s.sub_font_family] ? s.sub_font_family : d.fontFamily,
    color: s.sub_color || d.color,
    bold: s.sub_bold === "1",
    italic: s.sub_italic === "1",
    spacing: Number(s.sub_spacing ?? d.spacing),
    outline: Number(s.sub_outline ?? d.outline),
    outlineColor: s.sub_outline_color || d.outlineColor,
    shadow: Number(s.sub_shadow ?? d.shadow),
    glow: s.sub_glow || null,
    bgColor: s.sub_bg_color || d.bgColor,
    bgOpacity: s.sub_bg_opacity ? Number(s.sub_bg_opacity) : d.bgOpacity,
    position:
      s.sub_position === "middle" || s.sub_position === "top"
        ? s.sub_position
        : "bottom",
    zhScale: Number(s.sub_zh_scale) || d.zhScale,
  };
}

export function styleToSettings(st: AssStyleOptions): Record<string, string> {
  return {
    sub_font_size: String(st.fontSize),
    sub_font_family: st.fontFamily,
    sub_color: st.color,
    sub_bold: st.bold ? "1" : "0",
    sub_italic: st.italic ? "1" : "0",
    sub_spacing: String(st.spacing),
    sub_outline: String(st.outline),
    sub_outline_color: st.outlineColor,
    sub_shadow: String(st.shadow),
    sub_glow: st.glow ?? "",
    sub_bg_color: st.bgColor,
    sub_bg_opacity: String(st.bgOpacity),
    sub_position: st.position,
    sub_zh_scale: String(st.zhScale),
  };
}

/** #RRGGBB → ASS &HBBGGRR；可选不透明度（0 全透明-100 不透明） */
function assColor(hex: string, opacityPct = 100): string {
  const h = hex.replace("#", "");
  const n = parseInt(h, 16);
  const r = (n >> 16) & 255;
  const g = (n >> 8) & 255;
  const b = n & 255;
  const a = Math.round((100 - opacityPct) * 2.55);
  const hex2 = (v: number) => v.toString(16).padStart(2, "0").toUpperCase();
  return `&H${hex2(a)}${hex2(b)}${hex2(g)}${hex2(r)}`;
}

function assTime(secs: number): string {
  const cs = Math.max(0, Math.round(secs * 100));
  const h = Math.floor(cs / 360000);
  const m = Math.floor((cs % 360000) / 6000);
  const s = Math.floor((cs % 6000) / 100);
  const c = cs % 100;
  return `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(c).padStart(2, "0")}`;
}

/** ASS 文本转义：花括号是 override 标签定界符 */
function esc(text: string): string {
  return text.replace(/\{/g, "（").replace(/\}/g, "）").replace(/\n/g, "\\N");
}

const ALIGNMENT: Record<string, number> = { bottom: 2, middle: 5, top: 8 };
/** 各位置的垂直边距（控制条在视频区域之外，底部只需少量留白） */
const MARGIN_V: Record<string, number> = { bottom: 14, middle: 10, top: 28 };
/** px → PlayResY=288 下的 pt（约 480px 高的播放区域） */
const PX_TO_PT = 1.6;

/** 单屏字幕最大词数：超出则切成词组按时间渐显 */
const MAX_WORDS_PER_CAPTION = 8;
/** 每个词组的最短显示时长（秒）：避免长句切出的词组一闪而过 */
const MIN_CHUNK_SECS = 1.2;
/** 句读类标点（段首/段尾判断用） */
const LEADING_PUNCT = /^[，。！？；：、,.!?;:"'“”‘’（）()\[\]]+/;

/** 把长句子切成词组事件（优先在标点处断开），中文按字符比例同步切分 */
function chunkCue(c: AssCue): AssCue[] {
  const words = c.text_en.split(/\s+/).filter(Boolean);
  if (words.length <= MAX_WORDS_PER_CAPTION) return [c];

  const dur = c.end_secs - c.start_secs;
  // 组数受词数与时长双重约束：保证每个词组显示不低于 MIN_CHUNK_SECS
  const groupCount = Math.min(
    Math.ceil(words.length / MAX_WORDS_PER_CAPTION),
    Math.max(1, Math.floor(dur / MIN_CHUNK_SECS)),
  );
  if (groupCount <= 1) return [c];

  // 词组划分：按组数均分，边界就近对齐到词尾标点
  const groups: string[][] = [];
  let start = 0;
  for (let gi = 0; gi < groupCount; gi++) {
    let end = Math.round(((gi + 1) / groupCount) * words.length);
    if (gi < groupCount - 1) {
      for (let j = end - 1; j <= Math.min(end + 1, words.length - 2); j++) {
        if (j > start && /[,;—–.!?]["'”’)]?$/.test(words[j])) {
          end = j + 1;
          break;
        }
      }
    }
    end = Math.min(Math.max(end, start + 1), words.length);
    groups.push(words.slice(start, end));
    start = end;
  }

  // 中文按词组词数占比切分
  const totalWords = words.length;
  const zh = c.text_zh ?? "";
  let zhPos = 0;
  const zhParts: (string | null)[] = groups.map((g, gi) => {
    if (!zh) return null;
    if (gi === groups.length - 1) return zh.slice(zhPos);
    // 段首标点承接上文，不计入本段长度（整句开头的中文标点直接丢弃，避免孤零零的符号）
    const lead = zh.slice(zhPos).match(LEADING_PUNCT)?.[0].length ?? 0;
    const take = Math.round((g.length / totalWords) * zh.length);
    let end = Math.min(zhPos + lead + take, zh.length);
    // 从目标位置往后找标点收束
    const punct = zh.slice(end, end + 7).search(/[，。！？；：、,.!?;]/);
    if (punct >= 0) end += punct + 1;
    const part = zh.slice(gi === 0 ? zhPos + lead : zhPos, end);
    zhPos = end;
    return part;
  });
  // 段首标点并回上一段，避免出现孤零零的标点段
  for (let i = 1; i < zhParts.length; i++) {
    const part = zhParts[i];
    if (!part || zhParts[i - 1] == null) continue;
    const m = part.match(LEADING_PUNCT);
    if (m) {
      zhParts[i - 1] += m[0];
      zhParts[i] = part.slice(m[0].length) || null;
    }
  }

  // 时间按词数比例分配
  let t = c.start_secs;
  return groups.map((g, gi) => {
    const span = (g.length / totalWords) * dur;
    const ev: AssCue = {
      start_secs: t,
      end_secs: gi === groups.length - 1 ? c.end_secs : t + span,
      text_en: g.join(" "),
      text_zh: zhParts[gi],
    };
    t += span;
    return ev;
  });
}

export function buildAss(
  cues: AssCue[],
  st: AssStyleOptions,
  showZh: boolean,
): string {
  const fontSize = Math.round(st.fontSize * PX_TO_PT);
  const zhSize = Math.round((fontSize * st.zhScale) / 100);
  const boxed = st.bgOpacity > 0;
  const font = FONT_NAMES[st.fontFamily];
  const bold = st.bold ? -1 : 0;
  const italic = st.italic ? -1 : 0;
  // BorderStyle=4（libass 扩展）：底盒用 BackColour 着色（支持透明度），整条事件一个盒子，
  // 描边仍按 BorderStyle=1 绘制；此时 Shadow 字段变为底盒内边距，至少给 2 避免贴边
  const shadowOrPad = boxed ? Math.max(st.shadow, 2) : st.shadow;

  const header = `[Script Info]
Title: Lyrebird subtitles
ScriptType: v4.00+
PlayResX: 512
PlayResY: 288
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: En,${font},${fontSize},${assColor(st.color)},${assColor(st.color)},${assColor(st.outlineColor)},${assColor(st.bgColor, st.bgOpacity)},${bold},${italic},0,0,100,100,${st.spacing},0,${boxed ? 4 : 1},${st.outline},${shadowOrPad},${ALIGNMENT[st.position]},28,28,${MARGIN_V[st.position]},1
Style: Zh,${font},${zhSize},${assColor(st.color)},${assColor(st.color)},${assColor(st.outlineColor)},${assColor(st.bgColor, st.bgOpacity)},${bold},${italic},0,0,100,100,${st.spacing},0,${boxed ? 4 : 1},${st.outline},${shadowOrPad},${ALIGNMENT[st.position]},28,28,${MARGIN_V[st.position]},1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
`;

  const events = cues
    .flatMap((c) => chunkCue(c))
    .map((c) => {
      // 底盒模式下两端加头发丝空格，让背景左右有留白
      const pad = boxed ? "\\h\\h" : "";
      let text = `${pad}${esc(c.text_en)}${pad}`;
      if (showZh && c.text_zh) {
        text += `\\N{\\rZh}${pad}${esc(c.text_zh)}${pad}`;
      }
      const t0 = assTime(c.start_secs);
      const t1 = assTime(c.end_secs);
      const lines = [`Dialogue: 1,${t0},${t1},En,,0,0,0,,${text}`];
      // 霓虹发光：底层叠一条透明填充 + 加粗模糊描边（\r 会清掉 override，需重新打标；
      // \4a&HFF& 防止该底层事件在底盒模式下再叠一个不透明底盒）
      if (st.glow) {
        const g = `\\1a&HFF&\\4a&HFF&\\3c&H${assColor(st.glow).slice(4)}\\bord${st.outline + 3}\\blur2`;
        let glowText = `{${g}}${pad}${esc(c.text_en)}${pad}`;
        if (showZh && c.text_zh) {
          glowText += `\\N{\\rZh}${`{${g}}`}${pad}${esc(c.text_zh)}${pad}`;
        }
        lines.unshift(`Dialogue: 0,${t0},${t1},En,,0,0,0,,${glowText}`);
      }
      return lines.join("\n");
    })
    .join("\n");

  return header + events + "\n";
}
