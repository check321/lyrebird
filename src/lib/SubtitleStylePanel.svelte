<script lang="ts">
  import {
    SUBTITLE_PRESETS,
    type AssStyleOptions,
  } from "$lib/ass";

  let {
    style = $bindable(),
    onchange,
  }: {
    style: AssStyleOptions;
    onchange?: (s: AssStyleOptions) => void;
  } = $props();

  const SIZES = [13, 15, 18, 22];
  const FONTS: { key: string; name: string; css: string }[] = [
    { key: "roboto", name: "Roboto 无衬线", css: "'Roboto', sans-serif" },
    { key: "noto", name: "Noto Sans SC 黑体", css: "'Noto Sans CJK SC', sans-serif" },
    { key: "merriweather", name: "Merriweather 衬线", css: "'Merriweather', serif" },
    { key: "nunito", name: "Nunito 圆体", css: "'Nunito', sans-serif" },
    { key: "caveat", name: "Caveat 手写", css: "'Caveat', cursive" },
  ];
  const TEXT_COLORS = [
    "#ffffff", "#fde047", "#86efac", "#7dd3fc",
    "#fda4af", "#e5e7eb", "#c4b5fd", "#18181b",
  ];
  const BG_COLORS = ["#000000", "#27272a", "#1e3a8a", "#7f1d1d", "#ffffff"];
  const GLOW_COLORS = ["#22d3ee", "#f472b6", "#facc15", "#a78bfa"];

  function update(patch: Partial<AssStyleOptions>) {
    style = { ...style, ...patch };
    onchange?.(style);
  }

  function applyPreset(p: (typeof SUBTITLE_PRESETS)[number]) {
    style = { ...p.style };
    onchange?.(style);
  }

  const activePreset = $derived(
    SUBTITLE_PRESETS.find(
      (p) => JSON.stringify(p.style) === JSON.stringify(style),
    )?.name ?? null,
  );

  /** 「透明」开关：记住上次非零透明度，在 关闭背景/恢复背景 间切换 */
  let lastBgOpacity = $state(70);
  $effect(() => {
    if (style.bgOpacity > 0) lastBgOpacity = style.bgOpacity;
  });

  function stepSize(dir: 1 | -1) {
    const i = SIZES.indexOf(style.fontSize);
    const next = SIZES[Math.min(SIZES.length - 1, Math.max(0, i + dir))];
    if (next !== style.fontSize) update({ fontSize: next });
  }

  /** 预设卡片的 CSS 近似渲染（真实效果以 libass 为准） */
  function presetCardCss(p: AssStyleOptions): string {
    const fontCss =
      FONTS.find((f) => f.key === p.fontFamily)?.css ?? "sans-serif";
    const shadows: string[] = [];
    if (p.glow) {
      shadows.push(
        `0 0 6px ${p.glow}`,
        `0 0 12px ${p.glow}`,
        `0 0 20px ${p.glow}`,
      );
    } else if (p.outline > 0) {
      const c = p.outlineColor;
      const w = p.outline;
      shadows.push(
        `${w}px 0 0 ${c}`,
        `-${w}px 0 0 ${c}`,
        `0 ${w}px 0 ${c}`,
        `0 -${w}px 0 ${c}`,
        `${w}px ${w}px 0 ${c}`,
        `-${w}px -${w}px 0 ${c}`,
        `${w}px -${w}px 0 ${c}`,
        `-${w}px ${w}px 0 ${c}`,
      );
    }
    if (p.shadow > 0) shadows.push(`${p.shadow}px ${p.shadow}px 2px rgba(0,0,0,.6)`);
    const bg =
      p.bgOpacity > 0
        ? `background: ${p.bgColor}${Math.round((p.bgOpacity / 100) * 255)
            .toString(16)
            .padStart(2, "0")}; padding: 2px 8px; border-radius: 4px;`
        : "";
    return [
      `font-family: ${fontCss}`,
      `color: ${p.color}`,
      p.bold ? "font-weight: 700" : "",
      p.italic ? "font-style: italic" : "",
      shadows.length ? `text-shadow: ${shadows.join(",")}` : "",
      bg,
    ]
      .filter(Boolean)
      .join("; ");
  }
</script>

<div class="max-h-[72vh] w-80 space-y-4 overflow-y-auto p-4">
  <!-- 预设花字 -->
  <div>
    <p class="mb-1.5 text-xs text-zinc-400">预设</p>
    <div class="grid grid-cols-2 gap-1.5">
      {#each SUBTITLE_PRESETS as p (p.name)}
        <button
          onclick={() => applyPreset(p)}
          class="rounded-lg border bg-zinc-800 px-2 py-3 text-center transition-colors {activePreset ===
          p.name
            ? 'border-indigo-500 ring-1 ring-indigo-500'
            : 'border-transparent hover:border-zinc-300'}"
        >
          <span class="text-base leading-none" style={presetCardCss(p.style)}
            >Aa 字幕</span
          >
          <span class="mt-1 block text-[10px] text-zinc-400">{p.name}</span>
        </button>
      {/each}
    </div>
  </div>

  <div class="border-t border-zinc-100"></div>

  <!-- 字号 + 加粗/斜体 + 位置 -->
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-1">
      <button
        onclick={() => stepSize(-1)}
        class="flex h-8 w-8 items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100"
        title="减小字号"
      ><span class="text-xs font-semibold">A−</span></button>
      <span class="w-8 text-center text-sm font-medium text-zinc-700">{style.fontSize}</span>
      <button
        onclick={() => stepSize(1)}
        class="flex h-8 w-8 items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100"
        title="增大字号"
      ><span class="text-base font-semibold">A+</span></button>
    </div>
    <button
      onclick={() => update({ bold: !style.bold })}
      class="flex h-8 w-8 items-center justify-center rounded-md font-bold {style.bold
        ? 'bg-indigo-100 text-indigo-700'
        : 'text-zinc-500 hover:bg-zinc-100'}"
      title="加粗"
    >B</button>
    <button
      onclick={() => update({ italic: !style.italic })}
      class="flex h-8 w-8 items-center justify-center rounded-md italic {style.italic
        ? 'bg-indigo-100 text-indigo-700'
        : 'text-zinc-500 hover:bg-zinc-100'}"
      title="斜体"
    >I</button>
    <div class="flex overflow-hidden rounded-md border border-zinc-200">
      {#each [["bottom", "下"], ["middle", "中"], ["top", "上"]] as const as [pos, label]}
        <button
          onclick={() => update({ position: pos })}
          class="px-2.5 py-1.5 text-xs {style.position === pos
            ? 'bg-indigo-600 text-white'
            : 'text-zinc-500 hover:bg-zinc-50'}"
        >{label}</button>
      {/each}
    </div>
  </div>

  <!-- 字体 -->
  <div>
    <p class="mb-1.5 text-xs text-zinc-400">字体</p>
    <div class="flex gap-1.5">
      {#each FONTS as f (f.key)}
        <button
          onclick={() => update({ fontFamily: f.key })}
          title={f.name}
          class="flex h-9 flex-1 items-center justify-center rounded-md border text-lg {style.fontFamily ===
          f.key
            ? 'border-indigo-500 bg-indigo-50 text-indigo-700'
            : 'border-zinc-200 text-zinc-600 hover:border-zinc-300'}"
          style="font-family: {f.css}"
        >Aa</button>
      {/each}
    </div>
  </div>

  <!-- 文字颜色 -->
  <div>
    <p class="mb-1.5 text-xs text-zinc-400">文字颜色</p>
    <div class="flex items-center gap-1.5">
      {#each TEXT_COLORS as c (c)}
        <button
          onclick={() => update({ color: c })}
          class="h-6 w-6 rounded-full border {style.color === c
            ? 'ring-2 ring-indigo-500 ring-offset-1'
            : 'border-zinc-200'}"
          style="background: {c}"
          title={c}
          aria-label={c}
        ></button>
      {/each}
      <label
        class="relative h-6 w-6 cursor-pointer rounded-full border border-dashed border-zinc-300 text-center text-xs leading-6 text-zinc-400"
        title="自定义颜色"
      >+
        <input
          type="color"
          class="absolute inset-0 h-full w-full cursor-pointer opacity-0"
          value={style.color}
          oninput={(e) => update({ color: (e.target as HTMLInputElement).value })}
        />
      </label>
    </div>
  </div>

  <!-- 描边 / 阴影 -->
  <div class="flex gap-4">
    <div class="flex-1">
      <p class="mb-1.5 text-xs text-zinc-400">描边</p>
      <div class="flex overflow-hidden rounded-md border border-zinc-200">
        {#each [[0, "无"], [1, "细"], [2, "中"], [3, "粗"]] as const as [w, label]}
          <button
            onclick={() => update({ outline: w })}
            class="flex-1 py-1.5 text-xs {style.outline === w
              ? 'bg-indigo-600 text-white'
              : 'text-zinc-500 hover:bg-zinc-50'}"
          >{label}</button>
        {/each}
      </div>
    </div>
    <div class="flex-1">
      <p class="mb-1.5 text-xs text-zinc-400">阴影</p>
      <div class="flex overflow-hidden rounded-md border border-zinc-200">
        {#each [[0, "无"], [1, "浅"], [2, "中"], [3, "深"]] as const as [w, label]}
          <button
            onclick={() => update({ shadow: w })}
            class="flex-1 py-1.5 text-xs {style.shadow === w
              ? 'bg-indigo-600 text-white'
              : 'text-zinc-500 hover:bg-zinc-50'}"
          >{label}</button>
        {/each}
      </div>
    </div>
  </div>

  <!-- 描边颜色（仅描边开启时） -->
  {#if style.outline > 0}
    <div class="flex items-center justify-between">
      <p class="text-xs text-zinc-400">描边颜色</p>
      <label
        class="relative h-6 w-6 cursor-pointer rounded-full border border-zinc-200"
        style="background: {style.outlineColor}"
        title="描边颜色"
      >
        <input
          type="color"
          class="absolute inset-0 h-full w-full cursor-pointer opacity-0"
          value={style.outlineColor}
          oninput={(e) =>
            update({ outlineColor: (e.target as HTMLInputElement).value })}
        />
      </label>
    </div>
  {/if}

  <!-- 霓虹发光 -->
  <div>
    <p class="mb-1.5 text-xs text-zinc-400">发光</p>
    <div class="flex items-center gap-1.5">
      <button
        onclick={() => update({ glow: null })}
        class="h-6 rounded-md border border-zinc-200 px-2 text-xs {style.glow ===
        null
          ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-500'
          : 'text-zinc-500 hover:bg-zinc-50'}"
      >关</button>
      {#each GLOW_COLORS as c (c)}
        <button
          onclick={() => update({ glow: c })}
          class="h-6 w-6 rounded-full {style.glow === c
            ? 'ring-2 ring-indigo-500 ring-offset-1'
            : ''}"
          style="background: {c}; box-shadow: 0 0 6px {c}"
          title={c}
          aria-label={c}
        ></button>
      {/each}
    </div>
  </div>

  <!-- 背景 -->
  <div>
    <p class="mb-1.5 text-xs text-zinc-400">背景</p>
    <div class="mb-2 flex items-center gap-1.5">
      <button
        onclick={() =>
          update({ bgOpacity: style.bgOpacity === 0 ? lastBgOpacity : 0 })}
        class="h-6 rounded-md border border-zinc-200 px-2 text-xs {style.bgOpacity === 0
          ? 'bg-indigo-50 text-indigo-700 ring-1 ring-indigo-500'
          : 'text-zinc-500 hover:bg-zinc-50'}"
        title={style.bgOpacity === 0 ? "恢复背景" : "关闭背景"}
      >透明</button>
      {#each BG_COLORS as c (c)}
        <button
          onclick={() => update({ bgColor: c, bgOpacity: style.bgOpacity === 0 ? lastBgOpacity : style.bgOpacity })}
          class="h-6 w-6 rounded-full border {style.bgColor === c && style.bgOpacity > 0
            ? 'ring-2 ring-indigo-500 ring-offset-1'
            : 'border-zinc-200'}"
          style="background: {c}"
          title={c}
          aria-label={c}
        ></button>
      {/each}
    </div>
    <div class="flex items-center gap-2 {style.bgOpacity === 0 ? 'opacity-30' : ''}">
      <input
        type="range"
        min="10"
        max="100"
        step="5"
        value={style.bgOpacity}
        disabled={style.bgOpacity === 0}
        oninput={(e) => update({ bgOpacity: Number((e.target as HTMLInputElement).value) })}
        class="h-1 flex-1 accent-indigo-600"
        title={style.bgOpacity === 0 ? "已选择透明背景" : "背景不透明度"}
      />
      <span class="w-9 text-right font-mono text-xs text-zinc-400">{style.bgOpacity === 0 ? "透明" : `${style.bgOpacity}%`}</span>
    </div>
  </div>

  <!-- 字间距 / 中文字号 -->
  <div class="flex items-center justify-between">
    <p class="text-xs text-zinc-400">字间距</p>
    <div class="flex overflow-hidden rounded-md border border-zinc-200">
      {#each [[0, "标准"], [1, "宽"], [2, "更宽"]] as const as [v, label]}
        <button
          onclick={() => update({ spacing: v })}
          class="px-2.5 py-1.5 text-xs {style.spacing === v
            ? 'bg-indigo-600 text-white'
            : 'text-zinc-500 hover:bg-zinc-50'}"
        >{label}</button>
      {/each}
    </div>
  </div>
  <div class="flex items-center justify-between">
    <p class="text-xs text-zinc-400">中文字号</p>
    <div class="flex overflow-hidden rounded-md border border-zinc-200">
      {#each [75, 85, 100] as s (s)}
        <button
          onclick={() => update({ zhScale: s })}
          class="px-2.5 py-1.5 text-xs {style.zhScale === s
            ? 'bg-indigo-600 text-white'
            : 'text-zinc-500 hover:bg-zinc-50'}"
        >{s}%</button>
      {/each}
    </div>
  </div>
</div>
