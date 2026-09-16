<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  type WordCard = {
    id: number;
    word: string;
    phonetic: string | null;
    definition: string | null;
    ai_analysis: string | null;
    video_id: number | null;
    context: string | null;
    start_secs: number | null;
    created_at: string;
    video_title: string | null;
    video_thumb: string | null;
    review_count: number;
    fail_streak: number;
    mastered: number;
  };
  type Stats = { total: number; mastered: number; due: number };
  type Round = {
    id: number;
    total: number;
    correct: number;
    score: number;
    created_at: string;
  };

  let cards = $state<WordCard[]>([]);
  let stats = $state<Stats | null>(null);
  let rounds = $state<Round[]>([]);
  let query = $state("");

  async function refresh() {
    [cards, stats, rounds] = await Promise.all([
      invoke<WordCard[]>("list_word_cards"),
      invoke<Stats>("review_stats"),
      invoke<Round[]>("list_review_rounds", { limit: 5 }),
    ]);
  }

  async function remove(id: number) {
    await invoke("delete_word_card", { id });
    cards = cards.filter((c) => c.id !== id);
    stats = await invoke<Stats>("review_stats");
  }

  /** 按生词/释义/上下文搜索 */
  const filtered = $derived(
    query.trim()
      ? cards.filter((c) => {
          const q = query.trim().toLowerCase();
          return (
            c.word.toLowerCase().includes(q) ||
            (c.definition ?? "").toLowerCase().includes(q) ||
            (c.context ?? "").toLowerCase().includes(q)
          );
        })
      : cards,
  );

  /** 按来源视频分组，组内按加入时间倒排 */
  const groups = $derived(
    (() => {
      const map = new Map<string, { thumb: string | null; vid: number | null; list: WordCard[] }>();
      for (const c of filtered) {
        const key = c.video_title ?? "未关联视频";
        const g = map.get(key) ?? { thumb: c.video_thumb, vid: c.video_id, list: [] };
        g.list.push(c);
        map.set(key, g);
      }
      return [...map.entries()];
    })(),
  );

  function highlight(context: string, word: string): { text: string; hit: boolean }[] {
    const parts: { text: string; hit: boolean }[] = [];
    const re = new RegExp(`(${word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi");
    let last = 0;
    for (const m of context.matchAll(re)) {
      if (m.index > last) parts.push({ text: context.slice(last, m.index), hit: false });
      parts.push({ text: m[0], hit: true });
      last = m.index + m[0].length;
    }
    if (last < context.length) parts.push({ text: context.slice(last), hit: false });
    return parts;
  }

  onMount(refresh);
</script>

<div class="mx-auto max-w-3xl">
  <div class="mb-5 flex items-center justify-between">
    <h2 class="text-xl font-semibold text-zinc-800">生词本</h2>
    {#if stats && stats.due > 0}
      <a
        href="/words/review"
        class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500"
        >开始复习（{stats.due} 待巩固）</a
      >
    {/if}
  </div>

  {#if stats && stats.total > 0}
    <div class="mb-5 flex items-center gap-4 rounded-xl border border-zinc-200 bg-white px-4 py-3 text-xs text-zinc-500 shadow-sm">
      <span>共 <b class="text-zinc-800">{stats.total}</b> 个生词</span>
      <span>已掌握 <b class="text-green-600">{stats.mastered}</b></span>
      <span>待巩固 <b class="text-amber-600">{stats.due}</b></span>
      {#if rounds.length > 0}
        <span class="ml-auto">
          上轮复习 <b class="text-indigo-600">{rounds[0].score}</b> 分
          （{rounds[0].correct}/{rounds[0].total}）
        </span>
      {/if}
    </div>
  {/if}

  <input
    bind:value={query}
    placeholder="搜索生词 / 释义 / 例句…"
    class="mb-5 w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500"
  />

  {#if filtered.length === 0}
    <p class="text-sm text-zinc-400">
      {cards.length === 0
        ? "还没有生词。在播放页点击字幕中的单词即可查询并加入生词卡。"
        : "没有匹配的生词。"}
    </p>
  {:else}
    <div class="space-y-8">
      {#each groups as [title, g] (title)}
        <section>
          <!-- 来源视频分组头：封面 + 标题 -->
          <div class="mb-3 flex items-center gap-3">
            {#if g.thumb}
              <img
                src={convertFileSrc(g.thumb)}
                alt=""
                class="h-10 w-16 rounded-md object-cover"
              />
            {/if}
            <div class="min-w-0">
              <h3 class="truncate text-sm font-semibold text-zinc-700" title={title}>
                {title}
              </h3>
              <p class="text-xs text-zinc-400">{g.list.length} 个生词</p>
            </div>
            {#if g.vid}
              <a
                href="/player/{g.vid}"
                class="ml-auto shrink-0 text-xs text-indigo-600 hover:underline"
                >去复习视频 →</a
              >
            {/if}
          </div>

          <!-- 生词卡片：正面英文/音标，下方释义+例句 -->
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
            {#each g.list as c (c.id)}
              <div
                class="group flex flex-col rounded-xl border border-zinc-200 bg-white p-4 shadow-sm transition-shadow hover:shadow-md {c.mastered
                  ? 'opacity-60'
                  : ''}"
              >
                <div class="flex items-start justify-between gap-2">
                  <div class="min-w-0">
                    <span class="text-base font-bold text-zinc-900">{c.word}</span>
                    {#if c.word.includes(" ")}
                      <span class="ml-1.5 rounded bg-indigo-50 px-1.5 py-0.5 text-[10px] font-medium text-indigo-600">短语</span>
                    {/if}
                    {#if c.phonetic}
                      <span class="ml-1.5 text-xs text-zinc-400">/{c.phonetic}/</span>
                    {/if}
                  </div>
                  <div class="flex shrink-0 items-center gap-1">
                    {#if c.mastered}
                      <span class="rounded bg-green-50 px-1.5 py-0.5 text-[10px] font-medium text-green-600">已掌握</span>
                    {:else if c.fail_streak >= 3}
                      <span class="rounded bg-red-50 px-1.5 py-0.5 text-[10px] font-medium text-red-500">未掌握</span>
                    {:else if c.review_count > 0}
                      <span class="rounded bg-amber-50 px-1.5 py-0.5 text-[10px] font-medium text-amber-600">复习中 {c.review_count}/3</span>
                    {/if}
                    <button
                      onclick={() => remove(c.id)}
                      class="rounded p-1 text-zinc-300 opacity-0 transition-opacity group-hover:opacity-100 hover:bg-red-50 hover:text-red-500"
                      title="删除"
                      aria-label="删除"
                    >
                      <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" /></svg>
                    </button>
                  </div>
                </div>

                {#if c.definition}
                  <p class="mt-1.5 line-clamp-3 whitespace-pre-wrap text-xs leading-relaxed text-zinc-600">
                    {c.definition}
                  </p>
                {/if}
                {#if c.ai_analysis}
                  <p class="mt-1.5 rounded-md bg-indigo-50 px-2 py-1.5 text-xs leading-relaxed text-indigo-900">
                    {c.ai_analysis}
                  </p>
                {/if}
                {#if c.context}
                  <p class="mt-2 border-l-2 border-zinc-200 pl-2 text-xs leading-relaxed text-zinc-500 italic">
                    “{#each highlight(c.context, c.word) as seg}{#if seg.hit}<mark class="rounded-sm bg-amber-200/70 px-0.5 font-medium not-italic text-zinc-800">{seg.text}</mark>{:else}{seg.text}{/if}{/each}”
                  </p>
                {/if}
                {#if c.video_id}
                  <a
                    href="/player/{c.video_id}?t={Math.floor(c.start_secs ?? 0)}"
                    class="mt-2 inline-block text-xs text-indigo-600 hover:underline"
                  >
                    ⏱ 跳转到 {Math.floor(c.start_secs ?? 0)}s 出处
                  </a>
                {/if}
              </div>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}
</div>
