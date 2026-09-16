<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { save } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";
  import { buildAss, type AssStyleOptions } from "./ass";
  import { renderCuePngs, renderedCueCount, toSrt } from "./subtitleRender";
  import ClipTimeline from "./ClipTimeline.svelte";

  type CueLike = {
    start_secs: number;
    end_secs: number;
    text_en: string;
    text_zh: string | null;
  };

  let {
    videoId,
    title,
    cues,
    sentences,
    assStyle,
    duration,
    currentTime,
    hasSentenceZh,
    videoWidth,
    videoHeight,
    storyboard = null,
    onseek,
    onclose,
  }: {
    videoId: number;
    title: string | null;
    cues: CueLike[];
    sentences: CueLike[];
    assStyle: AssStyleOptions;
    duration: number;
    currentTime: number;
    hasSentenceZh: boolean;
    videoWidth: number;
    videoHeight: number;
    storyboard?: { interval: number; paths: string[] } | null;
    onseek: (secs: number) => void;
    onclose: () => void;
  } = $props();

  const EXPORT_FONTS = [
    "Roboto-Regular.ttf",
    "NotoSansCJKsc-Regular.otf",
    "Merriweather.ttf",
    "Nunito.ttf",
    "Caveat.ttf",
  ];

  let mode = $state<"full" | "clip">("clip");
  const hasSubs = $derived(cues.length > 0 || sentences.length > 0);
  let subMode = $state<"bilingual" | "en" | "none">("bilingual");

  let clipStart = $state(0);
  let clipEnd = $state(0);
  let inited = $state(false);

  let thumbs = $state<string[]>([]);
  let thumbError = $state("");

  let exporting = $state(false);
  let progress = $state(0);
  let step = $state("");
  let exportedPath = $state<string | null>(null);
  let error = $state("");

  /** ffmpeg 是否有 libass（ass 滤镜）；无则片段走 PNG 叠层烧录、全片走软字幕 */
  let canBurnAss = $state<boolean | null>(null);
  /** PNG 叠层烧录的叠层数上限（每条事件一个 overlay 滤镜，过多会拖慢初始化） */
  const MAX_OVERLAYS = 200;

  $effect(() => {
    if (!inited && duration > 0) {
      // 默认片段从当前播放位置开始，向后取 30s（不足则到片尾）
      const s = Math.min(Math.max(0, currentTime), Math.max(0, duration - 0.5));
      clipStart = s;
      clipEnd = Math.min(s + 30, duration);
      inited = true;
    }
  });
  $effect(() => {
    if (!hasSentenceZh && subMode === "bilingual") subMode = "en";
    if (!hasSubs) subMode = "none";
  });

  // 进入片段模式时取胶片缩略图：优先用播放页已生成的 storyboard，否则自行拉取（后端按视频缓存）
  $effect(() => {
    if (mode !== "clip" || thumbs.length > 0 || thumbError || duration <= 0)
      return;
    if (storyboard && storyboard.paths.length > 0) {
      thumbs = storyboard.paths.map((p) => convertFileSrc(p));
      return;
    }
    invoke<{ interval: number; paths: string[] }>("video_storyboard", {
      videoId,
      durationSecs: duration,
    })
      .then((s) => (thumbs = s.paths.map((p) => convertFileSrc(p))))
      .catch((e) => (thumbError = String(e)));
  });

  async function ensureFonts() {
    const fonts: [string, Uint8Array][] = [];
    for (const f of EXPORT_FONTS) {
      const resp = await fetch(`/fonts/${f}`);
      if (!resp.ok) throw new Error(`字体加载失败: ${f}`);
      fonts.push([f, new Uint8Array(await resp.arrayBuffer())]);
    }
    await invoke("ensure_export_fonts", { fonts });
  }

  function sanitizeName(s: string): string {
    return s.replace(/[\\/:*?"<>|]/g, "_").slice(0, 60) || "clip";
  }

  /** 当前导出范围对应的字幕事件（片段模式：过滤区间 + 时间平移到片段起点） */
  function currentEvts(): CueLike[] {
    const source = sentences.length > 0 ? sentences : cues;
    if (mode !== "clip") return source;
    const s = clipStart;
    const e = clipEnd;
    return source
      .filter((c) => c.end_secs > s && c.start_secs < e)
      .map((c) => ({
        start_secs: Math.max(0, c.start_secs - s),
        end_secs: Math.min(e, c.end_secs) - s,
        text_en: c.text_en,
        text_zh: c.text_zh,
      }));
  }

  function fmtClock(secs: number): string {
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${String(m).padStart(2, "0")}${String(s).padStart(2, "0")}`;
  }

  async function doExport() {
    error = "";
    const isClip = mode === "clip";
    const s = isClip ? clipStart : 0;
    const e = isClip ? clipEnd : duration;
    if (e - s < 0.5) {
      error = "片段时长太短（至少 0.5s）";
      return;
    }
    const fname =
      sanitizeName(title ?? "lyrebird") +
      (isClip ? `_${fmtClock(s)}-${fmtClock(e)}` : "") +
      ".mp4";
    const outPath = await save({
      defaultPath: fname,
      filters: [{ name: "MP4 视频", extensions: ["mp4"] }],
    });
    if (!outPath) return;

    exporting = true;
    exportedPath = null;
    progress = 0;
    try {
      let ass: string | null = null;
      let srt: string | null = null;
      let overlays: { start: number; end: number; png: number[] }[] = [];
      if (subMode !== "none" && hasSubs) {
        const evts = currentEvts();
        const showZh = subMode === "bilingual";
        if (canBurnAss) {
          step = "准备字体…";
          await ensureFonts();
          ass = buildAss(evts, assStyle, showZh);
        } else if (
          isClip &&
          videoWidth > 0 &&
          renderedCueCount(evts) <= MAX_OVERLAYS
        ) {
          // 无 libass：Canvas 渲染每条约字幕为全帧透明 PNG，overlay 叠层烧录
          step = "渲染字幕图层…";
          overlays = (
            await renderCuePngs(evts, assStyle, showZh, videoWidth, videoHeight)
          ).map((r) => ({ start: r.start, end: r.end, png: r.png }));
        } else {
          // 全片且无 libass：内嵌可开关 mov_text 软字幕
          srt = toSrt(evts, showZh);
        }
      }
      step = "编码导出中";
      await invoke("export_video", {
        videoId,
        outPath,
        startSecs: isClip ? s : null,
        endSecs: isClip ? e : null,
        assContent: ass,
        srtContent: srt,
        overlays,
        totalSecs: e - s,
      });
      progress = 100;
      step = "完成";
      exportedPath = outPath;
    } catch (err) {
      error = String(err);
      exporting = false;
    }
  }

  async function cancel() {
    await invoke("cancel_export").catch(() => {});
  }

  async function close() {
    if (exporting && !exportedPath) await cancel();
    onclose();
  }

  onMount(() => {
    invoke<{ ass: boolean }>("ffmpeg_features")
      .then((f) => (canBurnAss = f.ass))
      .catch(() => (canBurnAss = false));
    const unlisten = listen<{ video_id: number; percent: number }>(
      "export-progress",
      (e) => {
        if (e.payload.video_id !== videoId) return;
        progress = Math.max(progress, e.payload.percent);
      },
    );
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !exporting) {
        e.stopPropagation();
        onclose();
        return;
      }
      // FCP 式标记快捷键：I = 当前位置设为起点，O = 设为终点（片段模式）
      if (mode !== "clip" || exporting) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, [contenteditable='true']")) return;
      if (e.key === "i" || e.key === "I") {
        e.preventDefault();
        clipStart = Math.min(currentTime, clipEnd - 0.5);
      } else if (e.key === "o" || e.key === "O") {
        e.preventDefault();
        clipEnd = Math.max(currentTime, clipStart + 0.5);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      unlisten.then((f) => f());
      window.removeEventListener("keydown", onKey, true);
    };
  });
</script>

<div class="mt-2 space-y-3 rounded-xl border border-zinc-200 bg-white p-4 shadow-sm">
  <div class="flex items-center gap-3">
    <h3 class="shrink-0 text-sm font-semibold text-zinc-800">导出视频</h3>

    <!-- 范围 -->
    <div class="flex overflow-hidden rounded-lg border border-zinc-200 shadow-sm">
      {#each [["full", "全片导出"], ["clip", "片段导出"]] as const as [key, label]}
        <button
          onclick={() => (mode = key)}
          disabled={exporting}
          class="px-3 py-1.5 text-xs font-medium transition-colors {mode === key
            ? 'bg-indigo-600 text-white'
            : 'text-zinc-500 hover:bg-zinc-50'}"
        >{label}</button>
      {/each}
    </div>

    <!-- 字幕 -->
    <div class="flex overflow-hidden rounded-lg border border-zinc-200 shadow-sm">
      {#each [["bilingual", "中英双语"], ["en", "仅英文"], ["none", "无字幕"]] as const as [key, label]}
        <button
          onclick={() => (subMode = key)}
          disabled={exporting || (key === "bilingual" && !hasSentenceZh) || (key !== "none" && !hasSubs)}
          class="px-3 py-1.5 text-xs font-medium transition-colors disabled:opacity-40 {subMode === key
            ? 'bg-indigo-600 text-white'
            : 'text-zinc-500 hover:bg-zinc-50'}"
        >{label}</button>
      {/each}
    </div>

    <button
      onclick={close}
      class="ml-auto flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600"
      title={exporting && !exportedPath ? "取消并关闭" : "关闭"}
      aria-label="关闭导出面板"
    >
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18" /><path d="m6 6 12 12" /></svg>
    </button>
  </div>

  {#if mode === "clip"}
    <ClipTimeline
      {duration}
      {thumbs}
      bind:start={clipStart}
      bind:end={clipEnd}
      {currentTime}
      {onseek}
    />
    {#if thumbError}
      <p class="text-xs text-amber-600">预览图不可用：{thumbError}</p>
    {/if}
    <div class="flex items-center gap-2">
      <button
        onclick={() => {
          clipStart = Math.min(currentTime, clipEnd - 0.5);
          onseek(clipStart);
        }}
        disabled={exporting}
        class="flex items-center gap-1.5 rounded-md border border-zinc-200 px-2.5 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-50 disabled:opacity-50"
        title="把片段起点设为当前播放位置（快捷键 I）"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19 20 9 12l10-8v16Z" /><path d="M5 19V5" /></svg>
        当前为起点
      </button>
      <button
        onclick={() => {
          clipEnd = Math.max(currentTime, clipStart + 0.5);
          onseek(clipEnd);
        }}
        disabled={exporting}
        class="flex items-center gap-1.5 rounded-md border border-zinc-200 px-2.5 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-50 disabled:opacity-50"
        title="把片段终点设为当前播放位置（快捷键 O）"
      >
        <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 4 10 8-10 8V4Z" /><path d="M19 5v14" /></svg>
        当前为终点
      </button>
    </div>
  {/if}

  {#if error}
    <p class="rounded-md bg-red-50 px-3 py-2 text-xs whitespace-pre-wrap text-red-600">{error}</p>
  {/if}

  {#if exporting}
    <div class="space-y-1.5">
      <div class="flex items-center justify-between text-xs">
        <span class="text-zinc-500">{step}</span>
        <span class="font-mono text-zinc-400">{Math.floor(progress)}%</span>
      </div>
      <div class="h-1.5 w-full overflow-hidden rounded-full bg-zinc-100">
        <div
          class="h-full rounded-full bg-indigo-600 transition-[width] duration-300"
          style="width: {progress}%"
        ></div>
      </div>
      <div class="flex justify-end gap-2 pt-1">
        {#if exportedPath}
          <button
            onclick={() => invoke("reveal_in_folder", { path: exportedPath })}
            class="rounded-md border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-50"
          >在 Finder 中显示</button>
          <button
            onclick={close}
            class="rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500"
          >完成</button>
        {:else}
          <button
            onclick={cancel}
            class="rounded-md border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-50"
          >取消</button>
        {/if}
      </div>
    </div>
  {:else}
    <div class="flex items-center gap-3">
      <button
        onclick={doExport}
        class="flex items-center gap-1.5 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
      >
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" /><path d="M12 15V3" /></svg>
        导出 MP4
      </button>
      <p class="text-xs text-zinc-400">
        {#if subMode === "none"}
          不烧录字幕
        {:else if canBurnAss === false}
          {mode === "clip" && renderedCueCount(currentEvts()) <= MAX_OVERLAYS
            ? "字幕按当前样式烧录进画面（PNG 叠层）"
            : "当前 ffmpeg 无 libass，将内嵌可开关软字幕"}
        {:else}
          字幕按当前样式烧录进画面
        {/if}
      </p>
    </div>
  {/if}
</div>
