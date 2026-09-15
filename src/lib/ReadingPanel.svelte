<script lang="ts">
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

  let {
    tab = $bindable(),
    cues,
    sentences,
    paragraphs,
    currentCueId,
    currentSentId,
    savedWordMap,
    pipelineStatus,
    translating,
    hasSentenceZh,
    subtitleSource,
    onseek,
    onword,
    ontranslate,
    onasr,
    onrestructure,
    onhover,
    onhide,
    listEl = $bindable(),
  }: {
    tab: "raw" | "refined" | "bilingual";
    cues: Cue[];
    sentences: Sentence[];
    paragraphs: Sentence[][];
    currentCueId: number;
    currentSentId: number;
    savedWordMap: Map<string, SavedWord>;
    pipelineStatus: string | null;
    translating: boolean;
    hasSentenceZh: boolean;
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
    onhover: (saved: SavedWord | null, e?: MouseEvent) => void;
    onhide?: () => void;
    listEl?: HTMLDivElement | undefined;
  } = $props();

  /** 拆词 + 标记已收藏生词/短语 */
  function tokenize(
    text: string,
  ): { token: string; isWord: boolean; saved?: SavedWord }[] {
    const saved = [...savedWordMap.keys()].filter((w) => w.includes(" "));
    const savedRe =
      saved.length > 0
        ? new RegExp(
            `(${saved.map((w) => w.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`,
            "gi",
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

<div class="mb-2 flex shrink-0 items-stretch gap-1.5">
  <div class="flex flex-1 overflow-hidden rounded-lg border border-zinc-200 bg-white shadow-sm">
    {#each [["raw", "原字幕"], ["refined", "整理精校"], ["bilingual", "中英对照"]] as const as [key, label]}
      <button
        onclick={() => (tab = key)}
        class="flex-1 py-1.5 text-xs font-medium transition-colors {tab === key
          ? 'bg-indigo-600 text-white'
          : 'text-zinc-500 hover:bg-zinc-50'}"
      >{label}</button>
    {/each}
  </div>
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

  {:else if tab === "raw"}
    <!-- 原字幕：逐 cue 显示 -->
    <div class="space-y-1">
      {#each cues as c (c.id)}
        <div
          data-cue={c.id}
          role="button"
          tabindex="0"
          onclick={() => onseek(c.start_secs)}
          onkeydown={(e) => e.key === "Enter" && onseek(c.start_secs)}
          class="flex cursor-pointer items-baseline gap-2 rounded-md px-2 py-1 transition-colors {c.id ===
          currentCueId
            ? 'bg-indigo-100/80 text-indigo-950'
            : 'text-zinc-600 hover:bg-zinc-50/70'}"
        >
          <span
            class="shrink-0 select-none font-mono text-[10px] {c.id ===
            currentCueId
              ? 'text-indigo-400'
              : 'text-zinc-300'}">{fmtTime(c.start_secs)}</span
          >
          <p class="min-w-0 text-sm leading-relaxed">
            {#each tokenize(c.text_en) as t}
              {#if t.isWord}
                <span
                  role="button"
                  tabindex="-1"
                  class="cue-word rounded px-0.5 {t.saved
                    ? 'bg-amber-100 font-medium text-amber-900'
                    : 'hover:bg-zinc-200/60'}"
                  onclick={(e) => {
                    e.stopPropagation();
                    onword(t.token, c.text_en, c.start_secs, c.id, e);
                  }}
                  onkeydown={() => {}}
                  onmouseenter={(e) => t.saved && onhover(t.saved, e)}
                  onmouseleave={() => onhover(null)}
                >{t.token}</span>
              {:else}
                {t.token}
              {/if}
            {/each}
          </p>
        </div>
      {/each}
    </div>

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
    <!-- 整理精校 / 中英对照：段落 + 完整句子 -->
    <div class="space-y-5">
      {#each paragraphs as para}
        <div class="space-y-1.5">
          {#each para as s (s.id)}
            <div
              data-sent={s.id}
              role="button"
              tabindex="0"
              onclick={() => onseek(s.start_secs)}
              onkeydown={(e) => e.key === "Enter" && onseek(s.start_secs)}
              class="flex cursor-pointer items-baseline gap-2 rounded-md px-2 py-1 transition-colors {s.id ===
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
              <div class="min-w-0">
                <p class="text-sm leading-relaxed">
                  {#each tokenize(s.text_en) as t}
                    {#if t.isWord}
                      <span
                        role="button"
                        tabindex="-1"
                        class="cue-word rounded px-0.5 {t.saved
                          ? 'bg-amber-100 font-medium text-amber-900'
                          : 'hover:bg-zinc-200/60'} {s.id === currentSentId && !t.saved
                          ? 'hover:bg-indigo-200'
                          : ''}"
                        onclick={(e) => {
                          e.stopPropagation();
                          onword(
                            t.token,
                            s.text_en,
                            s.start_secs,
                            cueIdForSentence(s),
                            e,
                          );
                        }}
                        onkeydown={() => {}}
                        onmouseenter={(e) => t.saved && onhover(t.saved, e)}
                        onmouseleave={() => onhover(null)}
                      >{t.token}</span>
                    {:else}
                      {t.token}
                    {/if}
                  {/each}
                </p>
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
            </div>
          {/each}
        </div>
      {/each}
    </div>
  {/if}
</div>
