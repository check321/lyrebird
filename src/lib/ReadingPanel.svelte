<script lang="ts">
  import { onMount } from "svelte";

  type Cue = {
    id: number;
    start_secs: number;
    end_secs: number;
    text_en: string;
  };
  type Sentence = {
    id: number;
    para_idx: number;
    start_secs: number;
    end_secs: number;
    text_en: string;
    text_zh: string | null;
  };
  type SavedWord = {
    word: string;
    definition: string | null;
    ai_analysis: string | null;
  };
  type Chapter = {
    id: number;
    idx: number;
    start_secs: number;
    end_secs: number;
    title: string;
    summary: string | null;
  };

  let {
    tab = $bindable(),
    cues,
    sentences,
    paragraphs,
    currentSentId,
    savedWordMap,
    pipelineStatus,
    translating,
    hasSentenceZh,
    chapters = [],
    currentChapterId = -1,
    chapterStatus = "none",
    subtitleSource,
    onseek,
    onword,
    ontranslate,
    onasr,
    onrestructure,
    ongenerate,
    onhover,
    onhide,
    oneditsentence,
    listEl = $bindable(),
  }: {
    tab: "refined" | "bilingual" | "chapters";
    cues: Cue[];
    sentences: Sentence[];
    paragraphs: Sentence[][];
    currentSentId: number;
    savedWordMap: Map<string, SavedWord>;
    pipelineStatus: string | null;
    translating: boolean;
    hasSentenceZh: boolean;
    chapters?: Chapter[];
    currentChapterId?: number;
    chapterStatus?: "none" | "generating" | "ready";
    subtitleSource: string | null;
    onseek: (secs: number) => void;
    onword: (
      word: string,
      text: string,
      startSecs: number,
      cueId: number | null,
      e: MouseEvent,
    ) => void;
    ontranslate: () => void;
    onasr: () => void;
    onrestructure?: () => void;
    ongenerate?: () => void;
    onhover: (saved: SavedWord | null, e?: MouseEvent) => void;
    onhide?: () => void;
    oneditsentence?: (id: number, text: string) => void;
    listEl?: HTMLDivElement | undefined;
  } = $props();

  /** 拆词 + 标记已收藏生词/短语 */
  function tokenize(
    text: string,
  ): { token: string; isWord: boolean; saved?: SavedWord }[] {
    // 长短语优先，避免 "a lot" 抢走 "a lot of"；\b 防止匹配进更长单词内部
    const saved = [...savedWordMap.keys()]
      .filter((w) => w.includes(" "))
      .sort((a, b) => b.length - a.length);
    const savedRe =
      saved.length > 0
        ? new RegExp(
            `\\b(${saved.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})\\b`,
            // 不能加 g：带 g 时 match() 返回纯数组，没有 index
            "i",
          )
        : null;
    const parts: { token: string; isWord: boolean; saved?: SavedWord }[] = [];
    let rest = text;
    while (rest.length > 0) {
      const m = savedRe ? rest.match(savedRe) : null;
      const cut = m && m.index !== undefined ? m.index : rest.length;
      for (const piece of rest
        .slice(0, cut)
        .split(/([A-Za-z]+(?:['’-][A-Za-z]+)*)/)) {
        if (!piece) continue;
        const isWord = /^[A-Za-z]/.test(piece);
        const sw = savedWordMap.get(piece.toLowerCase());
        parts.push({ token: piece, isWord, saved: isWord ? sw : undefined });
      }
      if (m && m.index !== undefined) {
        const phrase = m[0];
        parts.push({
          token: phrase,
          isWord: true,
          saved: savedWordMap.get(phrase.toLowerCase()),
        });
        rest = rest.slice(m.index + phrase.length);
      } else {
        rest = "";
      }
    }
    return parts;
  }

  function cueIdForSentence(s: Sentence): number | null {
    return (
      cues.find(
        (c) => c.start_secs >= s.start_secs - 0.01 && c.start_secs < s.end_secs,
      )?.id ?? null
    );
  }

  /** 段落对应的章节（章节起点落在该段时间范围内），用于精校文本流内嵌章节标题 */
  function chapterForPara(para: Sentence[]): Chapter | null {
    if (para.length === 0) return null;
    const st = para[0].start_secs;
    const en = para[para.length - 1].end_secs;
    return (
      chapters.find((ch) => ch.start_secs >= st - 0.5 && ch.start_secs < en) ??
      null
    );
  }

  /** 拖拽框选短语：记录行内 token 区间，mouseup 时拼接为短语走查词流程 */
  let sel = $state<{
    row: string;
    anchor: number;
    head: number;
    text: string;
    startSecs: number;
    cueId: number | null;
  } | null>(null);
  let justDragged = false;

  function inSel(row: string, i: number): boolean {
    if (!sel || sel.row !== row) return false;
    return (
      i >= Math.min(sel.anchor, sel.head) && i <= Math.max(sel.anchor, sel.head)
    );
  }

  function startSel(
    row: string,
    i: number,
    text: string,
    startSecs: number,
    cueId: number | null,
    e: MouseEvent,
  ) {
    e.preventDefault(); // 避免触发原生文本选择
    sel = { row, anchor: i, head: i, text, startSecs, cueId };
  }

  function extendSel(row: string, i: number) {
    if (sel && sel.row === row && i !== sel.head) sel = { ...sel, head: i };
  }

  function finishSel(e: MouseEvent) {
    if (!sel) return;
    const cur = sel;
    sel = null;
    const tokens = tokenize(cur.text);
    const lo = Math.min(cur.anchor, cur.head);
    const hi = Math.max(cur.anchor, cur.head);
    const range = tokens.slice(lo, hi + 1);
    if (range.filter((t) => t.isWord).length < 2) return; // 未跨词：走原单击逻辑
    const phrase = range
      .map((t) => t.token)
      .join("")
      .replace(/^[^A-Za-z]+|[^A-Za-z]+$/g, "");
    if (!phrase) return;
    justDragged = true; // 抑制随后的 click（单词查词 / 行 seek）
    setTimeout(() => (justDragged = false), 0);
    onword(phrase, cur.text, cur.startSecs, cur.cueId, e);
  }

  // 拖拽跨词松手后浏览器会对 mousedown/mouseup 的公共祖先派发一次 click，
  // 其目标不是 .cue-word，会命中播放页 window 的「点外部关弹卡」逻辑。
  // 在 window 捕获阶段吞掉这次 click，避免弹卡一闪即关。
  onMount(() => {
    const swallow = (e: MouseEvent) => {
      if (justDragged) {
        justDragged = false;
        e.stopPropagation();
      }
    };
    window.addEventListener("click", swallow, true);
    return () => window.removeEventListener("click", swallow, true);
  });

  /** 人工修订：行内编辑状态（editingId 为句子 id） */
  let editingId = $state<number | null>(null);
  let editingText = $state("");

  function startEdit(s: Sentence, e: MouseEvent) {
    e.stopPropagation();
    editingId = s.id;
    editingText = s.text_en;
  }

  function saveEdit(s: Sentence) {
    const text = editingText.trim();
    if (text && text !== s.text_en) oneditsentence?.(s.id, text);
    editingId = null;
  }

  /** 挂载即聚焦并把光标放到文末 */
  function focusEnd(el: HTMLTextAreaElement) {
    el.focus();
    el.setSelectionRange(el.value.length, el.value.length);
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
</script>

<svelte:window onmouseup={finishSel} />

<div class="mb-2 flex shrink-0 items-stretch gap-1.5">
  <div class="flex flex-1 overflow-hidden rounded-lg border border-zinc-200 bg-white shadow-sm">
    {#each [["chapters", "章节"], ["refined", "整理精校"], ["bilingual", "中英对照"]] as const as [key, label]}
      <button
        onclick={() => (tab = key)}
        class="flex-1 py-1.5 text-xs font-medium transition-colors {tab === key
          ? 'bg-indigo-600 text-white'
          : 'text-zinc-500 hover:bg-zinc-50'}"
      >{label}</button>
    {/each}
  </div>
  {#if tab === "chapters" && ongenerate && chapters.length > 0}
    <button
      onclick={ongenerate}
      disabled={chapterStatus === "generating"}
      class="flex w-7 shrink-0 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600 disabled:pointer-events-none disabled:opacity-40"
      title="重新生成章节概要"
      aria-label="重新生成章节概要"
    >
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /><path d="M8 16H3v5" /></svg>
    </button>
  {/if}
  {#if onrestructure && cues.length > 0}
    <button
      onclick={onrestructure}
      disabled={pipelineStatus != null || translating}
      class="flex w-7 shrink-0 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600 disabled:pointer-events-none disabled:opacity-40"
      title="重新整理（重跑断句分段与去口水词，中文自动回填）"
      aria-label="重新整理"
    >
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" /><path d="M21 3v5h-5" /><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" /><path d="M8 16H3v5" /></svg>
    </button>
  {/if}
  {#if onhide}
    <button
      onclick={onhide}
      class="flex w-7 shrink-0 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600"
      title="隐藏阅读区"
      aria-label="隐藏阅读区"
    >
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18" /><path d="m6 6 12 12" /></svg>
    </button>
  {/if}
</div>

<div
  bind:this={listEl}
  class="min-h-0 flex-1 overflow-y-auto rounded-lg p-4"
>
  {#if cues.length === 0}
    <div class="p-2">
      <p class="mb-3 text-sm text-zinc-400">
        {subtitleSource === "asr"
          ? "该视频无自带字幕，可用本地 ASR 模型转写。"
          : "暂无字幕。"}
      </p>
      {#if pipelineStatus}
        <p class="thinking-text thinking-dots text-sm font-medium">
          {pipelineStatus.replace(/…$/, "")}
        </p>
      {:else}
        <button
          onclick={onasr}
          class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
          >开始 ASR 转写</button
        >
      {/if}
    </div>

  {:else if tab === "chapters"}
    <!-- 章节概要：按话题自动分段，点击跳转 -->
    {#if chapterStatus === "generating"}
      <div class="flex h-full flex-col items-center justify-center p-6 text-center">
        <div class="mb-3 flex h-9 w-9 items-center justify-center rounded-full bg-indigo-50">
          <span class="thinking-text text-lg">✦</span>
        </div>
        <p class="thinking-text thinking-dots text-sm font-medium">章节概要生成中</p>
        <p class="mt-1.5 text-xs text-zinc-400">AI 正在按话题为视频分段</p>
      </div>
    {:else if chapters.length === 0}
      <div class="flex h-full flex-col items-center justify-center p-6 text-center">
        {#if sentences.length === 0}
          <p class="text-sm text-zinc-400">整理精校后自动生成章节概要</p>
        {:else if ongenerate}
          <p class="mb-3 text-sm text-zinc-500">还没有章节概要</p>
          <button
            onclick={ongenerate}
            class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
          >生成章节概要</button>
        {/if}
      </div>
    {:else}
      <div class="space-y-1">
        {#each chapters as ch (ch.id)}
          <div
            data-chapter={ch.id}
            role="button"
            tabindex="0"
            onclick={() => onseek(ch.start_secs)}
            onkeydown={(e) => e.key === "Enter" && onseek(ch.start_secs)}
            class="flex cursor-pointer items-baseline gap-2 rounded-md px-2 py-1.5 transition-colors {ch.id ===
            currentChapterId
              ? 'bg-indigo-100/80 text-indigo-950'
              : 'text-zinc-600 hover:bg-zinc-50/70'}"
          >
            <span
              class="shrink-0 select-none font-mono text-[10px] {ch.id ===
              currentChapterId
                ? 'text-indigo-400'
                : 'text-zinc-300'}">{fmtTime(ch.start_secs)}</span
            >
            <div class="min-w-0 flex-1">
              <p class="text-sm font-medium {ch.id === currentChapterId ? 'text-indigo-950' : 'text-zinc-800'}">
                {ch.idx + 1}. {ch.title}
              </p>
              {#if ch.summary}
                <p class="mt-0.5 line-clamp-2 text-xs text-zinc-500">{ch.summary}</p>
              {/if}
            </div>
            <span class="shrink-0 font-mono text-[10px] text-zinc-300">{fmtTime(ch.end_secs - ch.start_secs)}</span>
          </div>
        {/each}
      </div>
    {/if}

  {:else if sentences.length === 0}
    <!-- 整理中/未开始 -->
    <div class="flex h-full flex-col items-center justify-center p-6 text-center">
      {#if pipelineStatus}
        <div
          class="mb-3 flex h-9 w-9 items-center justify-center rounded-full bg-indigo-50"
        >
          <span class="thinking-text text-lg">✦</span>
        </div>
        <p class="thinking-text thinking-dots text-sm font-medium">
          {pipelineStatus.replace(/…$/, "")}
        </p>
        <p class="mt-1.5 text-xs text-zinc-400">AI 正在整理字幕文本，请稍候</p>
      {:else}
        <p class="text-sm text-zinc-400">暂无整理结果</p>
      {/if}
    </div>

  {:else if tab === "bilingual" && !hasSentenceZh}
    <!-- 中英对照需要先翻译 -->
    <div class="flex h-full flex-col items-center justify-center p-6 text-center">
      {#if translating}
        <div
          class="mb-3 flex h-9 w-9 items-center justify-center rounded-full bg-indigo-50"
        >
          <span class="thinking-text text-lg">✦</span>
        </div>
        <p class="thinking-text thinking-dots text-sm font-medium">
          {(pipelineStatus ?? "AI 翻译中").replace(/…$/, "")}
        </p>
      {:else}
        <p class="mb-3 text-sm text-zinc-500">翻译后显示中英对照</p>
        <button
          onclick={ontranslate}
          class="flex items-center gap-1.5 rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
        >
          <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 8 6 6" /><path d="m4 14 6-6 2-3" /><path d="M2 5h12" /><path d="M7 2h1" /><path d="m22 22-5-10-5 10" /><path d="M14 18h6" /></svg>
          AI 翻译字幕
        </button>
        <p class="mt-2 text-xs text-zinc-400">翻译完成后自动补齐每句的中文</p>
      {/if}
    </div>

  {:else}
    <!-- 整理精校 / 中英对照：段落 + 完整句子（章节边界处内嵌标题，仅阅读区展示，不进导出字幕） -->
    <div class="space-y-5">
      {#each paragraphs as para}
        {@const ch = chapterForPara(para)}
        {#if ch}
          <div
            role="button"
            tabindex="0"
            onclick={() => onseek(ch.start_secs)}
            onkeydown={(e) => e.key === "Enter" && onseek(ch.start_secs)}
            class="group flex cursor-pointer items-center gap-2 pt-1"
            title="跳转到该章节"
          >
            <span class="shrink-0 font-mono text-[10px] text-zinc-300 group-hover:text-indigo-400">{fmtTime(ch.start_secs)}</span>
            <span class="shrink-0 text-xs font-semibold text-indigo-600">{ch.idx + 1} · {ch.title}</span>
            <div class="h-px min-w-8 flex-1 bg-zinc-200"></div>
          </div>
        {/if}
        <div class="space-y-1.5">
          {#each para as s (s.id)}
            <div
              data-sent={s.id}
              role="button"
              tabindex="0"
              onclick={() => {
                if (!justDragged) onseek(s.start_secs);
              }}
              onkeydown={(e) => e.key === "Enter" && onseek(s.start_secs)}
              class="group flex cursor-pointer items-baseline gap-2 rounded-md px-2 py-1 transition-colors {s.id ===
              currentSentId
                ? 'bg-indigo-100/80 text-indigo-950'
                : 'text-zinc-600 hover:bg-zinc-50/70'}"
            >
              <span
                class="shrink-0 select-none font-mono text-[10px] {s.id ===
                currentSentId
                  ? 'text-indigo-400'
                  : 'text-zinc-300'}">{fmtTime(s.start_secs)}</span
              >
              <div class="min-w-0 flex-1">
                {#if editingId === s.id}
                  <!-- 人工修订：行内编辑 -->
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <div
                    class="space-y-1.5"
                    role="presentation"
                    onclick={(e) => e.stopPropagation()}
                    onmousedown={(e) => e.stopPropagation()}
                  >
                    <textarea
                      use:focusEnd
                      bind:value={editingText}
                      rows={2}
                      class="block w-full rounded-md border border-indigo-300 px-2 py-1 text-sm leading-relaxed outline-none focus:ring-1 focus:ring-indigo-500"
                      aria-label="修订字幕文本"
                      onkeydown={(e) => {
                        e.stopPropagation();
                        if (e.key === "Escape") editingId = null;
                        if ((e.metaKey || e.ctrlKey) && e.key === "Enter")
                          saveEdit(s);
                      }}
                    ></textarea>
                    <div class="flex items-center gap-3 text-xs">
                      <button
                        onclick={() => saveEdit(s)}
                        class="font-medium text-indigo-600 hover:underline"
                        >保存</button
                      >
                      <button
                        onclick={() => (editingId = null)}
                        class="text-zinc-400 hover:underline">取消</button
                      >
                      <span class="ml-auto text-zinc-300">⌘↵ 保存 · Esc 取消</span>
                    </div>
                  </div>
                {:else}
                  <p class="text-sm leading-relaxed">
                  {#each tokenize(s.text_en) as t, i}
                    {#if t.isWord}
                      <span
                        role="button"
                        tabindex="-1"
                        class="cue-word rounded px-0.5 {inSel(`sent-${s.id}`, i)
                          ? 'bg-indigo-200 text-indigo-900'
                          : t.saved && t.token.includes(' ')
                            ? 'bg-emerald-100 font-medium text-emerald-900'
                            : t.saved
                              ? 'bg-amber-100 font-medium text-amber-900'
                              : 'hover:bg-zinc-200/60'} {s.id === currentSentId && !t.saved
                          ? 'hover:bg-indigo-200'
                          : ''}"
                        onclick={(e) => {
                          e.stopPropagation();
                          if (justDragged) return;
                          onword(
                            t.token,
                            s.text_en,
                            s.start_secs,
                            cueIdForSentence(s),
                            e,
                          );
                        }}
                        onkeydown={() => {}}
                        onmousedown={(e) =>
                          startSel(
                            `sent-${s.id}`,
                            i,
                            s.text_en,
                            s.start_secs,
                            cueIdForSentence(s),
                            e,
                          )}
                        onmouseenter={(e) => {
                          if (sel) extendSel(`sent-${s.id}`, i);
                          else if (t.saved) onhover(t.saved, e);
                        }}
                        onmouseleave={() => onhover(null)}
                      >{t.token}</span>
                    {:else if inSel(`sent-${s.id}`, i)}
                      <span class="bg-indigo-200/70">{t.token}</span>
                    {:else}
                      {t.token}
                    {/if}
                  {/each}
                  </p>
                {/if}
                {#if tab === "bilingual" && s.text_zh}
                  <p
                    class="mt-0.5 px-0.5 text-xs leading-relaxed {s.id ===
                    currentSentId
                      ? 'text-indigo-400'
                      : 'text-zinc-400'}"
                  >
                    {s.text_zh}
                  </p>
                {/if}
              </div>
              {#if oneditsentence && editingId !== s.id}
                <button
                  onclick={(e) => startEdit(s, e)}
                  class="shrink-0 self-start rounded-md p-1 text-zinc-300 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-zinc-100 hover:text-indigo-600"
                  title="修订字幕文本"
                  aria-label="修订字幕文本"
                >
                  <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" /></svg>
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</div>
