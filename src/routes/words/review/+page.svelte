<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { onDestroy, onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { sfxCorrect, sfxTick, sfxWrong, setSfxEnabled } from "$lib/sfx";

  type WordCard = {
    id: number;
    word: string;
    phonetic: string | null;
    definition: string | null;
    ai_analysis: string | null;
    video_id: number | null;
    context: string | null;
    start_secs: number | null;
    video_title: string | null;
    video_thumb: string | null;
  };

  type Question = {
    card: WordCard;
    kind: "spelling" | "meaning";
    options: string[]; // meaning 题用
    hintLevel: number; // 0 未提示 1 已提示
    answered: "pending" | "correct" | "wrong";
  };

  type Phase = "loading" | "empty" | "playing" | "summary";

  const ROUND_SIZE = 10;
  const QUESTION_SECS = 30;

  let phase = $state<Phase>("loading");
  let questions = $state<Question[]>([]);
  let qi = $state(0);
  let score = $state(0);
  let correctCount = $state(0);
  let failedWords = $state<WordCard[]>([]);
  let input = $state("");
  let inputEl: HTMLInputElement | undefined = $state();
  let soundOn = $state(true);
  let timeLeft = $state(QUESTION_SECS);
  let timer: ReturnType<typeof setInterval> | null = null;
  let timerQid = -1;
  let lastTickSec = -1;

  const q = $derived(questions[qi] ?? null);

  function stopTimer() {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  /** 每题 30s：最后 5s 变红 + 每秒滴答；超时判错 */
  function startTimer() {
    stopTimer();
    timeLeft = QUESTION_SECS;
    lastTickSec = -1;
    timer = setInterval(() => {
      timeLeft = Math.max(0, timeLeft - 0.1);
      const sec = Math.ceil(timeLeft);
      if (timeLeft > 0 && timeLeft <= 5 && sec !== lastTickSec) {
        lastTickSec = sec;
        sfxTick();
      }
      if (timeLeft <= 0) {
        stopTimer();
        onTimeout();
      }
    }, 100);
  }

  // 题目切换/作答完成时控制计时器（提示等字段变化不重置）
  $effect(() => {
    if (phase === "playing" && q && q.answered === "pending") {
      if (timerQid !== q.card.id) {
        timerQid = q.card.id;
        startTimer();
      }
    } else {
      timerQid = -1;
      stopTimer();
    }
  });

  async function onTimeout() {
    if (!q || q.answered !== "pending") return;
    q.answered = "wrong";
    sfxWrong();
    await judge("failed");
  }

  async function toggleSound() {
    soundOn = !soundOn;
    setSfxEnabled(soundOn);
    await invoke("save_settings", {
      values: { review_sound: soundOn ? "1" : "0" },
    }).catch(() => {});
  }

  onDestroy(stopTimer);

  onMount(async () => {
    const settings = await invoke<Record<string, string>>("get_settings").catch(
      () => ({}) as Record<string, string>,
    );
    soundOn = settings.review_sound !== "0";
    setSfxEnabled(soundOn);
    const cards = await invoke<WordCard[]>("list_review_candidates", {
      limit: ROUND_SIZE,
    });
    if (cards.length === 0) {
      phase = "empty";
      return;
    }
    const qs: Question[] = [];
    for (const c of cards) {
      // 有释义且能拿到 3 个干扰项 → 词义选择题；否则拼写题
      let options: string[] = [];
      if (c.definition) {
        options = await invoke<string[]>("random_distractors", {
          excludeWord: c.word,
          count: 3,
        });
        if (options.length >= 3) {
          options.push(c.definition);
          options.sort(() => Math.random() - 0.5);
        } else {
          options = [];
        }
      }
      const kind: Question["kind"] =
        options.length >= 4 && Math.random() < 0.5 ? "meaning" : "spelling";
      qs.push({
        card: c,
        kind,
        options,
        hintLevel: 0,
        answered: "pending",
      });
    }
    questions = qs;
    phase = "playing";
  });

  /** 拼写题：上下文里挖空生词 */
  function blankedContext(c: WordCard): string {
    if (!c.context) return "";
    const re = new RegExp(
      c.word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"),
      "gi",
    );
    return c.context.replace(re, "＿".repeat(Math.min(c.word.length, 8)));
  }

  /** 提示：拼写题揭示前几个字母 */
  function hintText(card: WordCard, level: number): string {
    const w = card.word;
    const reveal = Math.min(level, w.length - 1);
    return w.slice(0, reveal) + " _".repeat(Math.max(1, w.length - reveal));
  }

  function useHint() {
    if (!q || q.hintLevel > 0) return;
    q.hintLevel = 1;
    if (q.kind === "spelling") input = q.card.word.slice(0, 2);
  }

  async function judge(result: "correct" | "hinted" | "failed" | "skipped") {
    if (!q) return;
    await invoke("submit_review", { id: q.card.id, result }).catch(() => {});
    if (result === "correct") {
      score += 10;
      correctCount += 1;
    } else if (result === "hinted") {
      score += 5;
      correctCount += 1;
    } else {
      failedWords = [...failedWords, q.card];
      if (result === "skipped") score += 0;
    }
  }

  async function submitSpelling() {
    if (!q || q.answered !== "pending") return;
    const ok = input.trim().toLowerCase() === q.card.word.toLowerCase();
    q.answered = ok ? "correct" : "wrong";
    if (ok) sfxCorrect();
    else sfxWrong();
    await judge(ok ? (q.hintLevel > 0 ? "hinted" : "correct") : "failed");
  }

  async function answerMeaning(opt: string) {
    if (!q || q.answered !== "pending") return;
    const ok = opt === q.card.definition;
    q.answered = ok ? "correct" : "wrong";
    if (ok) sfxCorrect();
    else sfxWrong();
    await judge(ok ? (q.hintLevel > 0 ? "hinted" : "correct") : "failed");
  }

  async function skip() {
    if (!q || q.answered !== "pending") return;
    q.answered = "wrong";
    await judge("skipped");
  }

  async function next() {
    input = "";
    if (qi + 1 >= questions.length) {
      phase = "summary";
      await invoke("save_review_round", {
        total: questions.length,
        correct: correctCount,
        score,
      }).catch(() => {});
    } else {
      qi += 1;
      setTimeout(() => inputEl?.focus(), 50);
    }
  }
</script>

<div class="mx-auto max-w-3xl">
  <div class="mb-4 flex items-center gap-3">
    <a href="/words" class="text-sm text-zinc-400 hover:text-indigo-600">← 生词本</a>
    <h2 class="text-xl font-semibold text-zinc-800">生词复习</h2>
    {#if phase === "playing"}
      <span class="font-mono text-sm text-indigo-600">{score} 分</span>
    {/if}
    <button
      onclick={toggleSound}
      class="ml-auto flex h-7 w-7 items-center justify-center rounded-md text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600"
      title={soundOn ? "关闭音效" : "开启音效"}
      aria-label="音效开关"
    >
      {#if soundOn}
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="M15.54 8.46a5 5 0 0 1 0 7.07" /><path d="M19.07 4.93a10 10 0 0 1 0 14.14" /></svg>
      {:else}
        <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 5 6 9H2v6h4l5 4V5Z" /><path d="m22 9-6 6" /><path d="m16 9 6 6" /></svg>
      {/if}
    </button>
  </div>

  {#if phase === "loading"}
    <p class="text-sm text-zinc-400">正在出题…</p>

  {:else if phase === "empty"}
    <div class="rounded-xl border border-zinc-200 bg-white p-8 text-center shadow-sm">
      <p class="text-3xl">🎉</p>
      <p class="mt-2 text-sm text-zinc-600">没有待复习的生词，全部掌握！</p>
      <a href="/words" class="mt-4 inline-block text-sm text-indigo-600 hover:underline">返回生词本</a>
    </div>

  {:else if phase === "playing" && q}
    <!-- 进度条 -->
    <div class="mb-3 flex items-center gap-3">
      <div class="h-2 flex-1 overflow-hidden rounded-full bg-zinc-200">
        <div
          class="h-full rounded-full bg-indigo-500 transition-all"
          style="width: {((qi + 1) / questions.length) * 100}%"
        ></div>
      </div>
      <span class="font-mono text-xs text-zinc-400">{qi + 1}/{questions.length}</span>
    </div>

    {#if q.answered === "pending"}
      <!-- 30s 倒计时：最后 5s 变红 + 滴答音效 -->
      {@const urgent = timeLeft <= 5}
      <div class="mb-4 flex items-center gap-2">
        <svg class="h-4 w-4 shrink-0 {urgent ? 'text-red-500' : 'text-zinc-400'}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><path d="M12 6v6l4 2" /></svg>
        <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-zinc-200">
          <div
            class="h-full rounded-full transition-[width] duration-100 {urgent ? 'bg-red-500' : 'bg-indigo-500'}"
            style="width: {(timeLeft / QUESTION_SECS) * 100}%"
          ></div>
        </div>
        <span class="w-8 text-right font-mono text-xs {urgent ? 'font-semibold text-red-500' : 'text-zinc-500'}">{Math.ceil(timeLeft)}s</span>
      </div>
    {/if}

    <div class="rounded-xl border border-zinc-200 bg-white p-8 shadow-sm">
      <!-- 出处：视频封面 + 标题 + 跳转 -->
      {#if q.card.video_title || q.card.video_thumb}
        <div class="mb-5 flex items-center gap-2.5 border-b border-zinc-100 pb-4">
          {#if q.card.video_thumb}
            <img
              src={convertFileSrc(q.card.video_thumb)}
              alt=""
              class="h-9 w-16 shrink-0 rounded-md object-cover"
            />
          {/if}
          <span class="min-w-0 truncate text-xs text-zinc-400" title={q.card.video_title ?? ""}>
            {q.card.video_title ?? "未知视频"}
          </span>
          {#if q.card.video_id}
            <a
              href="/player/{q.card.video_id}?t={Math.floor(q.card.start_secs ?? 0)}"
              class="ml-auto shrink-0 rounded-md p-1.5 text-zinc-300 hover:bg-indigo-50 hover:text-indigo-600"
              title="跳转到出处"
              aria-label="跳转到出处"
            >
              <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6" /></svg>
            </a>
          {/if}
        </div>
      {/if}
      {#if q.kind === "spelling"}
        <p class="mb-1.5 text-sm text-zinc-400">根据释义和例句拼出单词</p>
        {#if q.card.definition}
          <p class="mb-4 whitespace-pre-wrap text-base leading-relaxed text-zinc-700">
            {q.card.definition}
          </p>
        {/if}
        {#if q.card.context}
          <p class="mb-5 border-l-2 border-indigo-200 pl-3 text-base text-zinc-500 italic">
            “{blankedContext(q.card)}”
          </p>
        {/if}
        {#if q.hintLevel > 0}
          <p class="mb-3 font-mono text-lg tracking-widest text-indigo-600">
            {hintText(q.card, 2)}
          </p>
        {/if}

        {#if q.answered === "pending"}
          <form
            onsubmit={(e) => {
              e.preventDefault();
              submitSpelling();
            }}
          >
            <input
              bind:this={inputEl}
              bind:value={input}
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              placeholder="输入单词…"
              class="w-full rounded-lg border border-zinc-300 px-4 py-4 text-center text-2xl font-medium tracking-wide outline-none focus:border-indigo-500 focus:ring-2 focus:ring-indigo-100"
            />
          </form>
        {:else}
          <div
            class="rounded-lg px-4 py-4 text-center {q.answered === 'correct'
              ? 'bg-green-50'
              : 'bg-red-50'}"
          >
            <p class="text-2xl font-bold {q.answered === 'correct' ? 'text-green-600' : 'text-red-500'}">
              {q.answered === "correct" ? "✓ 答对了" : "✗ 正确答案"}
            </p>
            <p class="mt-1.5 font-mono text-xl text-zinc-800">
              {q.card.word}
              {#if q.card.phonetic}
                <span class="text-sm text-zinc-400">/{q.card.phonetic}/</span>
              {/if}
            </p>
          </div>
        {/if}
      {:else}
        <p class="mb-1.5 text-sm text-zinc-400">选择正确的释义</p>
        <p class="mb-2 text-4xl font-bold text-zinc-900">
          {q.card.word}
          {#if q.card.phonetic}
            <span class="text-base font-normal text-zinc-400">/{q.card.phonetic}/</span>
          {/if}
        </p>
        {#if q.card.context}
          <p class="mb-5 border-l-2 border-indigo-200 pl-3 text-sm text-zinc-500 italic">
            “{q.card.context}”
          </p>
        {/if}
        <div class="space-y-2.5">
          {#each q.options as opt, i}
            {@const isAnswer = opt === q.card.definition}
            <button
              onclick={() => answerMeaning(opt)}
              disabled={q.answered !== "pending"}
              class="block w-full rounded-lg border px-5 py-3.5 text-left text-base transition-colors {q.answered ===
              'pending'
                ? 'border-zinc-200 text-zinc-700 hover:border-indigo-400 hover:bg-indigo-50'
                : isAnswer
                  ? 'border-green-400 bg-green-50 text-green-700'
                  : 'border-zinc-100 text-zinc-400'}"
            >
              <span class="mr-2 font-mono text-sm text-zinc-400">{String.fromCharCode(65 + i)}</span>
              <span class="line-clamp-2 inline">{opt}</span>
            </button>
          {/each}
        </div>
      {/if}

      <!-- 操作行 -->
      <div class="mt-6 flex items-center gap-2">
        {#if q.answered === "pending"}
          <button
            onclick={useHint}
            disabled={q.hintLevel > 0}
            class="rounded-md border border-amber-400 px-3.5 py-2 text-sm font-medium text-amber-600 hover:bg-amber-50 disabled:opacity-40"
            >💡 提示</button
          >
          <button
            onclick={skip}
            class="rounded-md border border-zinc-300 px-3.5 py-2 text-sm text-zinc-500 hover:bg-zinc-50"
            >跳过 ⏭</button
          >
        {:else}
          <button
            onclick={next}
            class="ml-auto rounded-md bg-indigo-600 px-6 py-2.5 text-base font-medium text-white hover:bg-indigo-500"
            >{qi + 1 >= questions.length ? "查看成绩" : "下一题 →"}</button
          >
        {/if}
      </div>
    </div>

  {:else if phase === "summary"}
    <div class="rounded-xl border border-zinc-200 bg-white p-8 text-center shadow-sm">
      <p class="text-4xl">{correctCount >= questions.length * 0.8 ? "🏆" : correctCount >= questions.length / 2 ? "💪" : "📖"}</p>
      <p class="mt-3 text-3xl font-bold text-indigo-600">{score} 分</p>
      <p class="mt-1 text-sm text-zinc-500">
        本轮答对 {correctCount}/{questions.length}
        （{Math.round((correctCount / questions.length) * 100)}%）
      </p>

      {#if failedWords.length > 0}
        <div class="mt-5 rounded-lg bg-red-50 p-4 text-left">
          <p class="mb-2 text-xs font-semibold text-red-500">
            需要再巩固（{failedWords.length}）
          </p>
          <ul class="space-y-1.5">
            {#each failedWords as w (w.id)}
              <li class="text-sm">
                <span class="font-medium text-zinc-800">{w.word}</span>
                <span class="ml-2 line-clamp-1 inline text-xs text-zinc-500">{w.definition ?? ""}</span>
              </li>
            {/each}
          </ul>
          <p class="mt-2 text-xs text-zinc-400">连续 3 次答错/跳过的生词会被标记为「未掌握」</p>
        </div>
      {/if}

      <div class="mt-6 flex justify-center gap-3">
        <button
          onclick={() => goto("/words")}
          class="rounded-md border border-zinc-300 px-4 py-2 text-sm text-zinc-600 hover:bg-zinc-50"
          >返回生词本</button
        >
        <button
          onclick={() => location.reload()}
          class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
          >再来一轮</button
        >
      </div>
    </div>
  {/if}
</div>
