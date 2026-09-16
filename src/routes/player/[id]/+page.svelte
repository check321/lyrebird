<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { page } from "$app/stores";
  import SubtitlesOctopus from "libass-wasm";
  import workerUrl from "libass-wasm/dist/js/subtitles-octopus-worker.js?url";
  import legacyWorkerUrl from "libass-wasm/dist/js/subtitles-octopus-worker-legacy.js?url";
  import wasmUrl from "libass-wasm/dist/js/subtitles-octopus-worker.wasm?url";
  import {
    buildAss,
    styleFromSettings,
    styleToSettings,
    DEFAULT_ASS_STYLE,
    type AssStyleOptions,
  } from "$lib/ass";
  import SubtitleStylePanel from "$lib/SubtitleStylePanel.svelte";
  import ExportDialog from "$lib/ExportDialog.svelte";
  import { speakWord } from "$lib/speech";
  import ReadingPanel from "$lib/ReadingPanel.svelte";
  import { ui } from "$lib/ui.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { marked } from "marked";
  import { fade } from "svelte/transition";
  import { onMount } from "svelte";

  type Video = {
    id: number;
    url: string;
    title: string | null;
    video_path: string | null;
    subtitle_source: string | null;
    tldr: string | null;
    position_secs: number;
    channel: string | null;
    description: string | null;
    category: string;
    duration_secs: number | null;
    created_at: string;
  };
  type Cue = {
    id: number;
    idx: number;
    start_secs: number;
    end_secs: number;
    text_en: string;
    text_zh: string | null;
    speaker: string | null;
  };
  type Sentence = {
    id: number;
    para_idx: number;
    sent_idx: number;
    start_secs: number;
    end_secs: number;
    text_en: string;
    text_zh: string | null;
    cue_from: number;
    cue_to: number;
  };
  type TranslateProgress = {
    video_id: number;
    done: number;
    total: number;
    pairs: [number, string][];
  };
  type DictEntry = {
    word: string;
    phonetic: string | null;
    definition: string | null;
    translation: string | null;
    pos: string | null;
    tag: string | null;
    exchange: string | null;
    collins?: number | null;
    bnc?: number | null;
    frq?: number | null;
  };
  type RichLookup = { mdx_html: string | null; entry: DictEntry | null };
  type SavedWord = {
    word: string;
    definition: string | null;
    ai_analysis: string | null;
  };
  type Chapter = {
    id: number;
    video_id: number;
    idx: number;
    start_secs: number;
    end_secs: number;
    title: string;
    summary: string | null;
  };
  type Storyboard = { interval: number; paths: string[] };
  type Popover = {
    word: string;
    x: number;
    y: number;
    sentText: string;
    startSecs: number;
    cueId: number | null;
    lookup: RichLookup | null | undefined; // undefined=加载中
    aiAnalysis: string | null;
    analyzing: boolean;
    added: boolean;
  };

  let video = $state<Video | null>(null);
  let cues = $state<Cue[]>([]);
  let sentences = $state<Sentence[]>([]);
  let savedWords = $state<SavedWord[]>([]);
  let showZh = $state(true);
  let currentTime = $state(0);
  let videoEl: HTMLVideoElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  let pipelineStatus = $state<string | null>(null);
  let translating = $state(false);
  let translateProgress = $state<{ done: number; total: number } | null>(null);
  let tab = $state<"refined" | "bilingual" | "chapters">("refined");
  let popover = $state<Popover | null>(null);
  let hoverCard = $state<{
    word: string;
    definition: string | null;
    ai_analysis: string | null;
    x: number;
    y: number;
  } | null>(null);
  let actionError = $state("");

  let videoSrc = $state<string | null>(null);
  let lastSavedPos = 0;
  let assStyle = $state<AssStyleOptions>(DEFAULT_ASS_STYLE);
  let settingsLoaded = $state(false);
  let jso: SubtitlesOctopus | null = null;
  let showStylePanel = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  let theater = $state(false);
  let prevSidebarCollapsed = false;
  let pausePanelHidden = $state(false);
  let showExport = $state(false);
  let chapters = $state<Chapter[]>([]);
  let storyboard = $state<Storyboard | null>(null);
  let chaptersGenerating = $state(false);
  let hoverT = $state<number | null>(null);
  let hoverX = $state(0);
  let storyboardStarted = false;

  // 自定义播放控制条状态
  let playing = $state(false);
  let duration = $state(0);
  let volume = $state(1);
  let rate = $state(1);
  let videoW = $state(0);
  let videoH = $state(0);

  const id = Number($page.params.id);

  /** 段落分组：[[句...], [句...]] */
  const paragraphs = $derived(
    (() => {
      const map = new Map<number, Sentence[]>();
      for (const s of sentences) {
        const list = map.get(s.para_idx) ?? [];
        list.push(s);
        map.set(s.para_idx, list);
      }
      return [...map.entries()]
        .sort((a, b) => a[0] - b[0])
        .map(([, v]) => v);
    })(),
  );

  /** 当前句子 id（多行整体高亮） */
  const currentSentId = $derived(
    sentences.find((s) => s.start_secs <= currentTime && currentTime < s.end_secs)
      ?.id ?? -1,
  );

  /** 句子级中文是否已生成 */
  const hasSentenceZh = $derived(sentences.some((s) => s.text_zh != null));

  const savedWordMap = $derived(
    new Map(savedWords.map((w) => [w.word.toLowerCase(), w])),
  );

  /** 当前播放位置所在章节 */
  const currentChapter = $derived(
    chapters.find((c) => currentTime >= c.start_secs && currentTime < c.end_secs),
  );
  const currentChapterId = $derived(currentChapter?.id ?? -1);
  const chapterStatus = $derived<"none" | "generating" | "ready">(
    chaptersGenerating ? "generating" : chapters.length > 0 ? "ready" : "none",
  );

  /** 后台生成/读取时间轴缩略图（storyboard），静默失败仅影响悬停预览 */
  function loadStoryboard() {
    const dur = video?.duration_secs || duration;
    if (storyboardStarted || !dur) return;
    storyboardStarted = true;
    invoke<Storyboard>("video_storyboard", { videoId: id, durationSecs: dur })
      .then((s) => (storyboard = s))
      .catch(() => {});
  }

  async function regenerateChapters() {
    if (chaptersGenerating || sentences.length === 0) return;
    chaptersGenerating = true;
    try {
      await invoke("generate_chapters", { videoId: id });
      chapters = await invoke<Chapter[]>("list_chapters", { videoId: id });
    } catch (e) {
      actionError = String(e);
    } finally {
      chaptersGenerating = false;
    }
  }

  onMount(() => {
    const unlisteners: (() => void)[] = [];
    (async () => {
      video = await invoke<Video | null>("get_video", { id });
      cues = await invoke<Cue[]>("list_cues", { videoId: id });
      sentences = await invoke<Sentence[]>("list_sentences", { videoId: id });
      savedWords = await invoke<SavedWord[]>("list_saved_words", { videoId: id });
      chapters = await invoke<Chapter[]>("list_chapters", { videoId: id });
      // 已有整理精校结果时默认展示精校 tab
      if (sentences.length > 0) tab = "refined";
      if (video?.video_path) {
        videoSrc = await invoke<string>("media_url", { path: video.video_path });
        loadStoryboard();
      }
      const settings = await invoke<Record<string, string>>("get_settings");
      assStyle = styleFromSettings(settings);
      settingsLoaded = true;

      const unlistenTranslate = await listen<TranslateProgress>(
        "translate-progress",
        (e) => {
          if (e.payload.video_id !== id) return;
          const { done, total, pairs } = e.payload;
          translateProgress = { done, total };
          pipelineStatus = `AI 翻译中 ${done}/${total}`;
          if (pairs.length === 0) return;
          // 渐进式翻译：每块译文就地更新 cue，并同步拼接到句级中文，字幕立即可见
          const zhByCue = new Map(pairs);
          cues = cues.map((c) => {
            const zh = zhByCue.get(c.id);
            return zh != null ? { ...c, text_zh: zh } : c;
          });
          if (sentences.length > 0) {
            sentences = fillSentenceZhLocal(cues, sentences);
          }
        },
      );
      const unlistenStructure = await listen<{ video_id: number; done: number; total: number }>(
        "structure-progress",
        (e) => {
          if (e.payload.video_id !== id) return;
          pipelineStatus = `断句分段中 ${e.payload.done}/${e.payload.total}`;
        },
      );
      unlisteners.push(unlistenTranslate, unlistenStructure);

      // 自动管线：进入页面自动做断句分段（不翻译）；TLDR 自动生成；翻译由用户手动触发
      if (cues.length > 0) {
        if (sentences.length === 0) {
          pipelineStatus = "断句分段中…";
          try {
            await invoke("structure_video", { videoId: id, force: false });
            sentences = await invoke<Sentence[]>("list_sentences", { videoId: id });
            tab = "refined";
          } catch (e) {
            actionError = String(e);
          } finally {
            pipelineStatus = null;
          }
        }
        // 已翻译但句级中文缺失（如重跑过分段）：本地补齐，不调 LLM
        if (
          sentences.length > 0 &&
          sentences.some((s) => s.text_zh == null) &&
          cues.some((c) => c.text_zh != null)
        ) {
          await invoke("fill_sentence_zh", { videoId: id });
          sentences = await invoke<Sentence[]>("list_sentences", { videoId: id });
        }
        // 后台依次补齐 TLDR / 章节概要（共享 pipelineStatus，串行避免闪烁）
        const needTldr = video && !video.tldr;
        const needChapters = sentences.length > 0 && chapters.length === 0;
        if (needTldr || needChapters) {
          (async () => {
            if (needTldr) {
              pipelineStatus = "TLDR 生成中…";
              await invoke<string>("generate_tldr", { videoId: id })
                .then((t) => {
                  if (video) video = { ...video, tldr: t };
                })
                .catch(() => {});
            }
            if (needChapters) {
              pipelineStatus = "章节概要生成中…";
              chaptersGenerating = true;
              await invoke("generate_chapters", { videoId: id })
                .then(async () => {
                  chapters = await invoke<Chapter[]>("list_chapters", {
                    videoId: id,
                  });
                })
                .catch(() => {});
              chaptersGenerating = false;
            }
          })().finally(() => (pipelineStatus = null));
        }
      }
    })();

    const onKey = (e: KeyboardEvent) => {
      // 空格 播放/暂停；←/→ 后退/快进 5s（输入控件聚焦时不抢）
      const t = e.target as HTMLElement;
      if (t.closest("input, textarea, [contenteditable='true']")) return;
      if (e.code === "Space") {
        e.preventDefault();
        togglePlay();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        skipBy(-5);
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        skipBy(5);
      }
    };
    window.addEventListener("keydown", onKey);

    return () => {
      window.removeEventListener("keydown", onKey);
      unlisteners.forEach((u) => u());
      persistPosition();
      jso?.dispose();
      jso = null;
    };
  });

  // libass 字幕渲染：优先用整理后的完整句子（没有则退回原始 cue）；中文开关/样式变化时更新 track
  $effect(() => {
    if (!videoEl || !settingsLoaded || !videoSrc) return;
    const source = sentences.length > 0 ? sentences : cues;
    const content = buildAss(source, assStyle, showZh);
    if (!jso) {
      jso = new SubtitlesOctopus({
        video: videoEl,
        subContent: content,
        workerUrl,
        legacyWorkerUrl,
        wasmUrl,
        fonts: [
          "/fonts/Roboto-Regular.ttf",
          "/fonts/NotoSansCJKsc-Regular.otf",
          "/fonts/Merriweather.ttf",
          "/fonts/Nunito.ttf",
          "/fonts/Caveat.ttf",
        ],
        fallbackFont: "/fonts/NotoSansCJKsc-Regular.otf",
      });
    } else {
      jso.setTrack(content);
    }
  });

  /** 面板改动：$effect 已保证实时渲染；这里防抖持久化到设置 */
  function onStyleChange(s: AssStyleOptions) {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      invoke("save_settings", { values: styleToSettings(s) }).catch(() => {});
    }, 600);
  }

  function persistPosition() {
    const t = videoEl?.currentTime ?? 0;
    if (t > 3 && Math.abs(t - lastSavedPos) > 2) {
      lastSavedPos = t;
      invoke("save_position", { id, secs: t }).catch(() => {});
    }
  }

  const hasUntranslated = $derived(cues.some((c) => c.text_zh == null));

  /** 翻译环进度：翻译中=已译分块比例；开始前/分段阶段=不确定态（旋转） */
  const RING_R = 8;
  const RING_C = 2 * Math.PI * RING_R;
  const ringPct = $derived(
    translateProgress &&
      translateProgress.total > 0 &&
      translateProgress.done < translateProgress.total
      ? translateProgress.done / translateProgress.total
      : null,
  );
  const translateTooltip = $derived(
    ringPct != null && translateProgress
      ? `AI 翻译中 ${translateProgress.done}/${translateProgress.total} · ${Math.round(ringPct * 100)}%`
      : (pipelineStatus ?? "AI 翻译中…"),
  );

  /** 与后端 fill_sentence_zh 一致：共享 cue 的译文按时间重叠比例互补划分（渐进翻译时实时刷新） */
  const STRONG_PUNCT = new Set(["。", "！", "？", "!", "?", "；", ";"]);

  function snapZhBoundary(chars: string[], cut: number, min: number, total: number): number {
    const base = Math.min(Math.max(cut, min), total);
    if (base === min || base === total) return base;
    const back = Math.max(base - 2, min, 1);
    for (let p = base; p >= back; p--) {
      if (STRONG_PUNCT.has(chars[p - 1])) return p;
    }
    const limit = Math.min(base + 8, total);
    for (let p = base + 1; p <= limit; p++) {
      if (STRONG_PUNCT.has(chars[p - 1])) return p;
    }
    return base;
  }

  function fillSentenceZhLocal(cs: Cue[], sents: Sentence[]): Sentence[] {
    const byIdx = new Map(cs.map((c) => [c.idx, c]));
    const consumed = new Map<number, number>();
    return sents.map((s) => {
      let zh = "";
      for (let i = s.cue_from; i <= s.cue_to; i++) {
        const c = byIdx.get(i);
        if (!c?.text_zh) continue;
        const chars = [...c.text_zh];
        const total = chars.length;
        const cur = consumed.get(i) ?? 0;
        if (cur >= total) continue;
        let cut: number;
        if (s.end_secs >= c.end_secs - 0.01) {
          cut = total;
        } else {
          const dur = Math.max(0.01, c.end_secs - c.start_secs);
          const overlap = Math.max(
            0,
            Math.min(s.end_secs, c.end_secs) - Math.max(s.start_secs, c.start_secs),
          );
          cut = snapZhBoundary(chars, cur + Math.round(total * (overlap / dur)), cur, total);
        }
        zh += chars.slice(cur, cut).join("");
        consumed.set(i, cut);
      }
      zh = zh.trim();
      if (!zh) return s;
      return s.text_zh === zh ? s : { ...s, text_zh: zh };
    });
  }

  /** 手动触发 AI 翻译（分块渐进返回，边译边显示；完成后本地补齐句级中文，不重跑 LLM 分段） */
  async function startTranslate() {
    actionError = "";
    translating = true;
    translateProgress = null;
    pipelineStatus = "AI 翻译中…";
    try {
      await invoke("translate_video", { videoId: id });
      // 与 DB 最终对齐一次（事件驱动已增量更新过）
      cues = await invoke<Cue[]>("list_cues", { videoId: id });
      // 分段还没完成（极端情况）先补分段，再本地拼接句级中文
      if (sentences.length === 0) {
        pipelineStatus = "断句分段中…";
        await invoke("structure_video", { videoId: id, force: false });
        tab = "refined";
      }
      await invoke("fill_sentence_zh", { videoId: id });
      sentences = await invoke<Sentence[]>("list_sentences", { videoId: id });
      // 章节概要随翻译链路补齐（句子已就绪）
      if (chapters.length === 0) {
        pipelineStatus = "章节概要生成中…";
        chaptersGenerating = true;
        await invoke("generate_chapters", { videoId: id }).catch(() => {});
        chapters = await invoke<Chapter[]>("list_chapters", { videoId: id });
        chaptersGenerating = false;
      }
    } catch (e) {
      actionError = String(e);
    } finally {
      translating = false;
      translateProgress = null;
      pipelineStatus = null;
    }
  }

  /** 重新整理：强制重跑断句分段（套用最新去口水词规则）；句级中文由 cue 译文本地回填，不重调 LLM */
  async function restructure() {
    if (pipelineStatus || translating) return;
    if (
      !confirm(
        "将清除当前整理结果并重新断句分段（含去口水词），已翻译的中文会自动回填。确定重新整理？",
      )
    )
      return;
    actionError = "";
    pipelineStatus = "断句分段中…";
    try {
      await invoke("structure_video", { videoId: id, force: true });
      await invoke("fill_sentence_zh", { videoId: id });
      sentences = await invoke<Sentence[]>("list_sentences", { videoId: id });
      // 段落变了章节也要重生成
      pipelineStatus = "章节概要生成中…";
      chaptersGenerating = true;
      await invoke("generate_chapters", { videoId: id }).catch(() => {});
      chapters = await invoke<Chapter[]>("list_chapters", { videoId: id });
      chaptersGenerating = false;
      tab = "refined";
    } catch (e) {
      actionError = String(e);
    } finally {
      pipelineStatus = null;
    }
  }

  async function startAsr() {
    actionError = "";
    pipelineStatus = "ASR 转写启动中…";
    try {
      await invoke("transcribe_video", { videoId: id });
      cues = await invoke<Cue[]>("list_cues", { videoId: id });
    } catch (e) {
      actionError = String(e);
    } finally {
      pipelineStatus = null;
    }
  }

  function togglePlay() {
    if (!videoEl) return;
    if (videoEl.paused) videoEl.play();
    else videoEl.pause();
  }

  function seek(t: number) {
    if (videoEl) videoEl.currentTime = t;
  }

  /** 快进/后退 delta 秒：保持当前播放状态，钳制在 [0, duration] */
  function skipBy(delta: number) {
    if (videoEl)
      seek(Math.min(Math.max(videoEl.currentTime + delta, 0), duration || 0));
  }

  function setVolume(v: number) {
    volume = v;
    if (videoEl) {
      videoEl.volume = v;
      videoEl.muted = v === 0;
    }
  }

  function setRate(r: number) {
    rate = r;
    if (videoEl) videoEl.playbackRate = r;
  }

  function fmtTime(secs: number): string {
    const s = Math.max(0, Math.floor(secs));
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const r = s % 60;
    return h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}`
      : `${m}:${String(r).padStart(2, "0")}`;
  }

  function toggleTheater() {
    theater = !theater;
    if (theater) {
      prevSidebarCollapsed = ui.sidebarCollapsed;
      ui.sidebarCollapsed = true;
    } else {
      ui.sidebarCollapsed = prevSidebarCollapsed;
    }
  }

  /** 全屏：Tauri 窗口级全屏 + 沉浸模式，字幕层随页面一起全屏 */
  async function toggleFullscreen() {
    const win = getCurrentWindow();
    const isFs = await win.isFullscreen();
    if (isFs) {
      await win.setFullscreen(false);
      if (theater) toggleTheater();
    } else {
      if (!theater) toggleTheater();
      await win.setFullscreen(true);
    }
  }

  function onTimeUpdate() {
    const t = videoEl?.currentTime ?? 0;
    currentTime = t;
    persistPosition();
  }

  function onVideoMeta() {
    if (!videoEl) return;
    duration = videoEl.duration || 0;
    videoW = videoEl.videoWidth || 0;
    videoH = videoEl.videoHeight || 0;
    // ?t= 参数（生词卡跳转）优先，否则从上次断点续播
    const t = Number($page.url.searchParams.get("t"));
    if (t > 0) {
      videoEl.currentTime = t;
    } else if (video && video.position_secs > 3) {
      videoEl.currentTime = video.position_secs;
    }
  }

  function seekToSent(s: Sentence) {
    seekTo(s.start_secs);
  }

  function seekTo(secs: number) {
    if (videoEl) {
      videoEl.currentTime = secs + 0.001;
      videoEl.play();
    }
  }

  /** 剪辑条拖动联动预览：只定位不强制播放 */
  function scrubTo(secs: number) {
    if (videoEl) {
      videoEl.pause();
      videoEl.currentTime = secs + 0.001;
    }
  }

  /** 时间轴悬停：计算预览时间与预览框 x（clamp 防出屏） */
  function onTimelineHover(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || duration <= 0) return;
    const x = Math.min(rect.width, Math.max(0, e.clientX - rect.left));
    hoverT = (x / rect.width) * duration;
    hoverX = Math.min(rect.width - 72, Math.max(72, x));
  }

  // 当前句/章节变化时滚动到可视区域（按当前 tab 选目标）
  $effect(() => {
    if (tab === "chapters") {
      if (currentChapterId >= 0) {
        listEl
          ?.querySelector(`[data-chapter="${currentChapterId}"]`)
          ?.scrollIntoView({ block: "center", behavior: "smooth" });
      }
    } else if (currentSentId >= 0) {
      listEl
        ?.querySelector(`[data-sent="${currentSentId}"]`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
    }
  });

  async function clickWord(
    word: string,
    text: string,
    startSecs: number,
    cueId: number | null,
    e: MouseEvent,
  ) {
    const x = Math.min(e.clientX, window.innerWidth - 380);
    const y = Math.min(e.clientY, window.innerHeight - 400);
    hoverCard = null;
    popover = {
      word,
      x,
      y,
      sentText: text,
      startSecs,
      cueId,
      lookup: undefined,
      aiAnalysis: null,
      analyzing: false,
      added: savedWordMap.has(word.toLowerCase()),
    };
    try {
      const lookup = await invoke<RichLookup>("lookup_word", { word });
      if (popover?.word === word) {
        popover = { ...popover, lookup };
        // 短语通常查不到词典，未命中时自动 AI 解析
        if (word.includes(" ") && !lookup.mdx_html && !lookup.entry)
          aiAnalyze();
      }
    } catch (err) {
      if (popover?.word === word)
        popover = { ...popover, lookup: null, aiAnalysis: String(err) };
    }
  }

  function speak(word: string) {
    speakWord(word);
  }

  /** MDX HTML 最小化净化：去脚本/事件属性 */
  function sanitizeHtml(html: string): string {
    return html
      .replace(/<script[\s\S]*?<\/script>/gi, "")
      .replace(/\son\w+="[^"]*"/gi, "");
  }

  const EXCHANGE_LABELS: Record<string, string> = {
    p: "过去式",
    d: "过去分词",
    i: "现在分词",
    "3": "第三人称",
    r: "比较级",
    t: "最高级",
    s: "复数",
    "0": "原型",
  };
  const TAG_LABELS: Record<string, string> = {
    zk: "中考",
    gk: "高考",
    cet4: "四级",
    cet6: "六级",
    ky: "考研",
    ielts: "雅思",
    toefl: "托福",
    gre: "GRE",
  };
  function wordForms(exchange: string | null): [string, string][] {
    if (!exchange) return [];
    return exchange
      .split("/")
      .map((p) => p.split(":"))
      .filter((p) => p.length === 2 && p[1] && EXCHANGE_LABELS[p[0]])
      .map((p) => [EXCHANGE_LABELS[p[0]], p[1]]);
  }
  function examTags(tag: string | null): string[] {
    if (!tag) return [];
    return tag
      .split(/\s+/)
      .filter((t) => TAG_LABELS[t])
      .map((t) => TAG_LABELS[t]);
  }

  function showHover(saved: SavedWord, e: MouseEvent) {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    hoverCard = {
      word: saved.word,
      definition: saved.definition,
      ai_analysis: saved.ai_analysis,
      x: Math.min(rect.left, window.innerWidth - 320),
      y: rect.bottom + 6,
    };
  }

  async function aiAnalyze() {
    if (!popover || popover.analyzing) return;
    popover = { ...popover, analyzing: true };
    try {
      const analysis = await invoke<string>("analyze_word", {
        word: popover.word,
        context: popover.sentText,
        videoId: id,
        cueId: popover.cueId,
      });
      if (popover) popover = { ...popover, aiAnalysis: analysis, analyzing: false };
    } catch (e) {
      if (popover)
        popover = { ...popover, aiAnalysis: String(e), analyzing: false };
    }
  }

  async function addCard() {
    if (!popover || popover.added) return;
    try {
      await invoke("add_word_card", {
        word: popover.lookup?.entry?.word ?? popover.word,
        phonetic: popover.lookup?.entry?.phonetic ?? null,
        definition:
          popover.lookup?.entry?.translation ??
          popover.lookup?.entry?.definition ??
          (popover.lookup?.mdx_html ? "（详见 MDX 词典）" : null),
        aiAnalysis: popover.aiAnalysis,
        videoId: id,
        cueId: popover.cueId,
        context: popover.sentText,
        startSecs: popover.startSecs,
      });
      savedWords = await invoke<SavedWord[]>("list_saved_words", { videoId: id });
      popover = null; // 加入成功后自动关闭词典面板，高亮词即反馈
    } catch (e) {
      actionError = String(e);
    }
  }

  function closePopover() {
    popover = null;
  }

  /** 人工修订句子文本：更新数据库与本地状态（内嵌字幕、阅读区高亮随 sentences 自动重建） */
  async function editSentence(sentenceId: number, text: string) {
    try {
      await invoke("update_sentence", { id: sentenceId, textEn: text });
      sentences = sentences.map((s) =>
        s.id === sentenceId ? { ...s, text_en: text } : s,
      );
    } catch (e) {
      actionError = String(e);
    }
  }
</script>

<svelte:window
  onclick={(e) => {
    const t = e.target as HTMLElement;
    if (popover && !t.closest(".word-popover, .cue-word")) {
      closePopover();
    }
    if (showStylePanel && !t.closest(".sub-style-panel, .sub-style-btn")) {
      showStylePanel = false;
    }
  }}
/>

{#if video}
  <!-- 面包屑 -->
  <nav class="mb-3 flex items-center gap-1.5 text-xs text-zinc-400">
    <a href="/" class="hover:text-indigo-600">媒体库</a>
    <span>/</span>
    <span class="text-zinc-500">{video.category}</span>
    <span>/</span>
    <span class="max-w-96 truncate text-zinc-600" title={video.title ?? ""}>
      {video.title ?? "未命名视频"}
    </span>
  </nav>

  <div class="flex h-[calc(100%-1.75rem)] gap-6">
    <div class="relative flex min-w-0 flex-1 flex-col">
      <div class="mb-3 flex items-center justify-between gap-3">
        <h2 class="min-w-0 truncate text-lg font-semibold text-zinc-800">
          {video.title ?? "未命名视频"}
        </h2>
        <div class="flex shrink-0 items-center gap-1">
          {#if video.video_path && videoSrc}
            <button
              onclick={() => (showExport = !showExport)}
              class="flex h-8 w-8 items-center justify-center rounded-md {showExport
                ? 'bg-indigo-50 text-indigo-600'
                : 'text-zinc-400 hover:bg-zinc-100 hover:text-zinc-700'}"
              title="导出剪辑（可烧录字幕）"
              aria-label="导出剪辑"
            >
              <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="6" r="3" /><path d="M8.12 8.12 12 12" /><path d="M20 4 8.12 15.88" /><circle cx="6" cy="18" r="3" /><path d="M14.8 14.8 20 20" /></svg>
            </button>
          {/if}
          <button
            onclick={toggleTheater}
            class="flex h-8 w-8 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-700"
            title={theater ? "退出沉浸模式" : "沉浸模式（隐藏字幕栏和侧边栏）"}
            aria-label="沉浸模式"
          >
            {#if theater}
              <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m14 10 7-7" /><path d="M20 10h-6V4" /><path d="m3 21 7-7" /><path d="M4 14h6v6" /></svg>
            {:else}
              <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21 21-6-6" /><path d="M21 15v6h-6" /><path d="m3 3 6 6" /><path d="M9 3v6H3" /></svg>
            {/if}
          </button>
        </div>
      </div>

      {#if video.video_path && videoSrc}
        <div class="relative {theater ? 'min-h-0 flex-1' : ''}">
          <video
            bind:this={videoEl}
            src={videoSrc}
            class="{theater
              ? 'h-full w-full object-contain'
              : 'w-full'} cursor-pointer rounded-lg bg-black"
            onclick={togglePlay}
            onplay={() => (playing = true)}
            onpause={() => {
              playing = false;
              persistPosition();
            }}
            ontimeupdate={onTimeUpdate}
            onloadedmetadata={onVideoMeta}
          >
            <track kind="captions" />
          </video>
        </div>

        <!-- 自定义播放控制条：时间轴独占一行，控件第二行 -->
        <div
          class="relative mt-2 rounded-lg border border-zinc-200 bg-white px-3 py-2 shadow-sm"
        >
          <!-- 时间轴：滑杆 + 章节刻度 + 悬停预览（缩略图/时间/章节标题） -->
          <div
            class="relative"
            onmousemove={onTimelineHover}
            onmouseleave={() => (hoverT = null)}
            role="presentation"
          >
            <input
              type="range"
              min="0"
              max={duration || 0}
              step="0.1"
              value={currentTime}
              oninput={(e) => seek(Number((e.target as HTMLInputElement).value))}
              onchange={(e) => (e.target as HTMLInputElement).blur()}
              class="h-1 w-full accent-indigo-600"
              aria-label="播放进度"
            />
            {#each chapters as ch, i (ch.id)}
              {#if i > 0 && duration > 0}
                <div
                  class="pointer-events-none absolute top-1/2 h-1.5 w-0.5 -translate-y-1/2 rounded-full bg-white shadow-[0_0_3px_rgba(0,0,0,0.6)]"
                  style="left: {(ch.start_secs / duration) * 100}%"
                ></div>
              {/if}
            {/each}
            {#if hoverT != null && duration > 0}
              {@const sbIdx = storyboard
                ? Math.min(
                    storyboard.paths.length - 1,
                    Math.floor(hoverT / storyboard.interval),
                  )
                : -1}
              {@const hch = chapters.find(
                (c) => hoverT! >= c.start_secs && hoverT! < c.end_secs,
              )}
              <div
                class="pointer-events-none absolute bottom-full z-30 mb-2 -translate-x-1/2 rounded-lg border border-zinc-200 bg-white p-1.5 shadow-lg"
                style="left: {hoverX}px"
              >
                {#if sbIdx >= 0 && storyboard?.paths[sbIdx]}
                  <img
                    src={convertFileSrc(storyboard.paths[sbIdx])}
                    alt=""
                    class="h-18 w-32 rounded object-cover"
                  />
                {/if}
                <div class="flex items-center gap-1.5 px-0.5 pt-1">
                  <span class="shrink-0 font-mono text-[10px] text-zinc-500"
                    >{fmtTime(hoverT)}</span
                  >
                  {#if hch}
                    <span class="max-w-40 truncate text-xs font-medium text-zinc-700"
                      >{hch.title}</span
                    >
                  {/if}
                </div>
              </div>
            {/if}
          </div>

          <div class="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <button
              onclick={() => skipBy(-5)}
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-600 hover:bg-zinc-100"
              title="后退 5 秒（←）"
              aria-label="后退 5 秒"
            >
              <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" /><path d="M3 3v5h5" /><text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="600" fill="currentColor" stroke="none">5</text></svg>
            </button>

            <button
              onclick={togglePlay}
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-600 hover:bg-zinc-100"
              title={playing ? "暂停（空格）" : "播放（空格）"}
              aria-label={playing ? "暂停" : "播放"}
            >
              {#if playing}
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1" /><rect x="14" y="4" width="4" height="16" rx="1" /></svg>
              {:else}
                <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.14v13.72a1 1 0 0 0 1.52.86l11-6.86a1 1 0 0 0 0-1.72l-11-6.86A1 1 0 0 0 8 5.14Z" /></svg>
              {/if}
            </button>

            <button
              onclick={() => skipBy(5)}
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-zinc-600 hover:bg-zinc-100"
              title="快进 5 秒（→）"
              aria-label="快进 5 秒"
            >
              <svg class="h-5 w-5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="600" fill="currentColor" stroke="none">5</text></svg>
            </button>

            <span class="shrink-0 font-mono text-xs text-zinc-500">
              {fmtTime(currentTime)} / {fmtTime(duration)}
            </span>

            {#if currentChapter}
              <span
                class="max-w-48 truncate text-xs font-medium text-indigo-600"
                title={currentChapter.title}>{currentChapter.title}</span
              >
            {/if}

            <div class="flex shrink-0 overflow-hidden rounded-md border border-zinc-200">
              {#each [0.75, 1, 1.25, 1.5, 2] as r (r)}
                <button
                  onclick={() => setRate(r)}
                  class="px-2 py-1 text-xs {rate === r
                    ? 'bg-indigo-600 text-white'
                    : 'text-zinc-500 hover:bg-zinc-50'}"
                >{r}×</button>
              {/each}
            </div>

            <div class="flex shrink-0 items-center gap-1.5">
              <button
                onclick={() => setVolume(volume === 0 ? 1 : 0)}
                class="flex h-7 w-7 items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100"
                title={volume === 0 ? "取消静音" : "静音"}
                aria-label="音量"
              >
                {#if volume === 0}
                  <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="m22 9-6 6" /><path d="m16 9 6 6" /></svg>
                {:else}
                  <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="M15.54 8.46a5 5 0 0 1 0 7.07" /></svg>
                {/if}
              </button>
              <input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={volume}
                oninput={(e) => setVolume(Number((e.target as HTMLInputElement).value))}
                onchange={(e) => (e.target as HTMLInputElement).blur()}
                class="h-1 w-16 accent-indigo-600"
                aria-label="音量大小"
              />
            </div>

            <div class="flex-1"></div>

            {#if cues.length > 0 && (hasUntranslated || translating)}
              <span class="group relative flex shrink-0">
                <button
                  onclick={startTranslate}
                  disabled={translating}
                  class="flex h-7 items-center gap-1.5 rounded-md px-2 text-xs font-medium text-indigo-600 hover:bg-indigo-50 disabled:opacity-60"
                  title={translating ? undefined : "AI 翻译字幕（完成后自动分段整理）"}
                  aria-label={translating ? translateTooltip : "AI 翻译字幕"}
                >
                  {#if translating}
                    <span class="relative flex h-5 w-5 items-center justify-center">
                      <svg
                        class="absolute inset-0 h-full w-full {ringPct == null
                          ? 'animate-spin'
                          : '-rotate-90'}"
                        viewBox="0 0 20 20"
                        fill="none"
                      >
                        <circle
                          cx="10"
                          cy="10"
                          r={RING_R}
                          stroke="currentColor"
                          stroke-opacity="0.2"
                          stroke-width="2"
                        />
                        {#if ringPct != null}
                          <circle
                            cx="10"
                            cy="10"
                            r={RING_R}
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-dasharray={RING_C}
                            stroke-dashoffset={RING_C * (1 - ringPct)}
                            style="transition: stroke-dashoffset 0.35s ease"
                          />
                        {:else}
                          <circle
                            cx="10"
                            cy="10"
                            r={RING_R}
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-dasharray="{RING_C * 0.28} {RING_C * 0.72}"
                          />
                        {/if}
                      </svg>
                      <svg class="h-2.5 w-2.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6" /><path d="m4 14 6-6 2-3" /><path d="M2 5h12" /><path d="M7 2h1" /><path d="m22 22-5-10-5 10" /><path d="M14 18h6" /></svg>
                    </span>
                    翻译中
                  {:else}
                    <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6" /><path d="m4 14 6-6 2-3" /><path d="M2 5h12" /><path d="M7 2h1" /><path d="m22 22-5-10-5 10" /><path d="M14 18h6" /></svg>
                    翻译
                  {/if}
                </button>
                {#if translating}
                  <span
                    class="pointer-events-none absolute right-0 bottom-full z-30 mb-1.5 rounded-md bg-zinc-900/90 px-2 py-1 text-[11px] whitespace-nowrap text-white opacity-0 shadow-md transition-opacity group-hover:opacity-100"
                  >
                    {translateTooltip}
                  </span>
                {/if}
              </span>
            {/if}

            <label class="flex shrink-0 items-center gap-1.5 text-xs text-zinc-500 select-none">
              <input type="checkbox" bind:checked={showZh} class="accent-indigo-500" />
              中文
            </label>

            <button
              onclick={() => (showStylePanel = !showStylePanel)}
              class="sub-style-btn flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100 {showStylePanel &&
                'bg-indigo-50 text-indigo-600'}"
              title="字幕样式"
              aria-label="字幕样式"
            >
              <span class="text-xs font-semibold">Aa</span>
            </button>

            <button
              onclick={toggleFullscreen}
              class="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-zinc-500 hover:bg-zinc-100"
              title="全屏"
              aria-label="全屏"
            >
              <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3" /><path d="M21 8V5a2 2 0 0 0-2-2h-3" /><path d="M3 16v3a2 2 0 0 0 2 2h3" /><path d="M16 21h3a2 2 0 0 0 2-2v-3" /></svg>
            </button>
          </div>

          {#if showStylePanel}
            <div
              class="sub-style-panel absolute right-2 bottom-full z-40 mb-2 rounded-xl border border-zinc-200 bg-white shadow-xl"
            >
              <SubtitleStylePanel bind:style={assStyle} onchange={onStyleChange} />
            </div>
          {/if}
        </div>
      {:else if video.video_path}
        <p class="text-sm text-zinc-400">正在加载视频…</p>
      {:else}
        <p class="text-sm text-zinc-400">视频文件尚未就绪。</p>
      {/if}

      {#if showExport && video.video_path && videoSrc}
        <ExportDialog
          videoId={id}
          title={video.title}
          {cues}
          {sentences}
          {assStyle}
          {duration}
          {currentTime}
          {hasSentenceZh}
          videoWidth={videoW}
          videoHeight={videoH}
          {storyboard}
          onseek={scrubTo}
          onclose={() => (showExport = false)}
        />
      {/if}

      {#if actionError}
        <p class="mt-3 rounded-md bg-red-50 px-3 py-2 text-sm text-red-600">
          {actionError}
        </p>
      {/if}

      <!-- 视频信息 + 自动 TLDR（沉浸模式下隐藏，把高度让给视频） -->
      {#if !theater}
        <div class="mt-4 space-y-3 overflow-y-auto pb-6">
        <div
          class="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border border-zinc-200 bg-white px-4 py-3 text-xs text-zinc-500 shadow-sm"
        >
          <span class="font-medium text-indigo-600">{video.channel ?? "未知频道"}</span>
          {#if video.duration_secs}
            <span>时长 {fmtTime(video.duration_secs)}</span>
          {/if}
          <span>{video.subtitle_source === "asr" ? "ASR 字幕" : "CC 字幕"}</span>
          <span>{video.created_at}</span>
          <a
            href={video.url}
            target="_blank"
            class="ml-auto text-zinc-400 hover:text-indigo-600">原始链接 ↗</a
          >
        </div>

        {#if video.tldr}
          <div class="rounded-lg border border-zinc-200 bg-white p-4 shadow-sm">
            <h3 class="mb-2 text-sm font-semibold text-zinc-600">TLDR</h3>
            <div class="tldr text-sm leading-relaxed text-zinc-600">
              {@html marked.parse(video.tldr)}
            </div>
          </div>
        {:else if pipelineStatus === "TLDR 生成中…"}
          <div class="rounded-lg border border-zinc-200 bg-white p-4 shadow-sm">
            <h3 class="mb-2 text-sm font-semibold text-zinc-600">TLDR</h3>
            <p class="animate-pulse text-sm text-zinc-400">AI 正在阅读全文并生成摘要…</p>
          </div>
        {/if}
        </div>
      {/if}

      <!-- 沉浸模式：暂停时右侧渐显半透明阅读区，便于随手加生词卡；可随时隐藏/打开 -->
      {#if theater && !playing && cues.length > 0}
        {#if pausePanelHidden}
          <button
            transition:fade={{ duration: 180 }}
            onclick={() => (pausePanelHidden = false)}
            class="absolute top-3 right-14 z-20 flex h-8 w-8 items-center justify-center rounded-md border border-zinc-200 bg-white/80 text-zinc-500 shadow-sm backdrop-blur-md hover:text-zinc-800"
            title="显示阅读区"
            aria-label="显示阅读区"
          >
            <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="18" height="18" x="3" y="3" rx="2" /><path d="M15 3v18" /></svg>
          </button>
        {:else}
          <div
            transition:fade={{ duration: 180 }}
            class="absolute top-0 right-3 bottom-0 z-20 flex w-[24rem] flex-col py-2"
          >
            <div
              class="flex min-h-0 flex-1 flex-col rounded-xl border border-zinc-200/80 bg-white/80 p-3 shadow-xl backdrop-blur-md"
            >
              <ReadingPanel
                bind:tab
                bind:listEl
                {cues}
                {sentences}
                {paragraphs}
                {currentSentId}
                {savedWordMap}
                {pipelineStatus}
                {translating}
                {hasSentenceZh}
                {chapters}
                {currentChapterId}
                {chapterStatus}
                subtitleSource={video.subtitle_source}
                onseek={seekTo}
                onword={clickWord}
                ontranslate={startTranslate}
                onasr={startAsr}
                onrestructure={restructure}
                ongenerate={regenerateChapters}
                onhover={(saved, e) => (saved ? showHover(saved, e!) : (hoverCard = null))}
                onhide={() => (pausePanelHidden = true)}
                oneditsentence={editSentence}
              />
            </div>
          </div>
        {/if}
      {/if}
    </div>

    {#if !theater}
      <div class="flex w-96 shrink-0 flex-col rounded-lg border border-zinc-200 bg-white shadow-sm">
        <ReadingPanel
          bind:tab
          bind:listEl
          {cues}
          {sentences}
          {paragraphs}
          {currentSentId}
          {savedWordMap}
          {pipelineStatus}
          {translating}
          {hasSentenceZh}
          {chapters}
          {currentChapterId}
          {chapterStatus}
          subtitleSource={video.subtitle_source}
          onseek={seekTo}
          onword={clickWord}
          ontranslate={startTranslate}
          onasr={startAsr}
          onrestructure={restructure}
          ongenerate={regenerateChapters}
          onhover={(saved, e) => (saved ? showHover(saved, e!) : (hoverCard = null))}
          oneditsentence={editSentence}
        />
      </div>
    {/if}
  </div>

  <!-- 生词悬停速览卡 -->
  {#if hoverCard && !popover}
    <div
      class="pointer-events-none fixed z-40 w-72 rounded-lg border border-zinc-200 bg-white p-3 shadow-lg"
      style="left: {hoverCard.x}px; top: {hoverCard.y}px;"
    >
      <p class="mb-1 text-sm font-semibold text-zinc-800">{hoverCard.word}</p>
      {#if hoverCard.definition}
        <p class="line-clamp-3 whitespace-pre-wrap text-xs leading-snug text-zinc-600">
          {hoverCard.definition}
        </p>
      {/if}
      {#if hoverCard.ai_analysis}
        <p class="mt-1 line-clamp-2 text-xs text-indigo-700">
          {hoverCard.ai_analysis}
        </p>
      {/if}
    </div>
  {/if}

  <!-- 点词弹卡 -->
  {#if popover}
    {@const lk = popover.lookup}
    {@const entry = lk?.entry}
    <div
      class="word-popover fixed z-50 max-h-[60vh] w-96 overflow-y-auto rounded-xl border border-zinc-200 bg-white p-4 shadow-xl"
      style="left: {popover.x}px; top: {popover.y}px;"
    >
      <div class="mb-1 flex items-center gap-2">
        <span class="text-base font-semibold text-zinc-800">
          {entry?.word ?? popover.word}
        </span>
        {#if popover.word.includes(" ")}
          <span class="rounded bg-indigo-50 px-1.5 py-0.5 text-[10px] font-medium text-indigo-600">短语</span>
        {/if}
        {#if entry?.phonetic}
          <span class="text-xs text-zinc-400">/{entry.phonetic}/</span>
        {/if}
        <button
          onclick={() => speak(popover?.word ?? "")}
          class="flex h-6 w-6 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-indigo-600"
          title="发音"
          aria-label="发音"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="M15.54 8.46a5 5 0 0 1 0 7.07" /><path d="M19.07 4.93a10 10 0 0 1 0 14.14" /></svg>
        </button>
      </div>

      {#if lk === undefined}
        <p class="text-sm text-zinc-400">查询中…</p>
      {:else if lk?.mdx_html}
        <!-- MDX 富文本词条（详细释义/例句） -->
        <div class="mdx-entry text-sm leading-relaxed text-zinc-700">
          {@html sanitizeHtml(lk.mdx_html)}
        </div>
      {:else if entry}
        {#if entry.translation}
          <p class="whitespace-pre-wrap text-sm leading-snug text-zinc-700">
            {entry.translation}
          </p>
        {:else if entry.definition}
          <p class="whitespace-pre-wrap text-xs leading-snug text-zinc-500">
            {entry.definition}
          </p>
        {/if}
        {#if wordForms(entry.exchange ?? null).length > 0}
          <p class="mt-2 text-xs text-zinc-500">
            {#each wordForms(entry.exchange ?? null) as [label, form]}
              <span class="mr-2"><span class="text-zinc-400">{label}</span> <span class="font-medium">{form}</span></span>
            {/each}
          </p>
        {/if}
        {#if examTags(entry.tag ?? null).length > 0}
          <p class="mt-1.5 flex flex-wrap gap-1">
            {#each examTags(entry.tag ?? null) as t}
              <span class="rounded bg-zinc-100 px-1.5 py-0.5 text-[10px] text-zinc-500">{t}</span>
            {/each}
          </p>
        {/if}
      {:else if lk}
        <p class="text-sm text-zinc-400">词典中未找到该词</p>
      {/if}

      {#if popover.aiAnalysis}
        <div class="mt-2 rounded-md bg-indigo-50 p-2">
          <p class="text-xs leading-relaxed text-indigo-900">
            {popover.aiAnalysis}
          </p>
        </div>
      {/if}

      <div class="mt-3 flex gap-2">
        <button
          onclick={aiAnalyze}
          disabled={popover.analyzing}
          class="rounded-md border border-indigo-600 px-2.5 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-50 disabled:opacity-50"
        >
          {popover.analyzing ? "解析中…" : "AI 深入解析"}
        </button>
        <button
          onclick={addCard}
          disabled={popover.added}
          class="rounded-md bg-indigo-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
        >
          {popover.added ? "已加入 ✓" : "加入生词卡"}
        </button>
      </div>
    </div>
  {/if}
{:else}
  <p class="text-sm text-zinc-400">加载中…</p>
{/if}
