<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { page } from "$app/stores";
  import { fmtCount } from "$lib/youtube";
  import { onMount } from "svelte";

  type ChannelInfo = {
    channel_id: string;
    title: string;
    url: string;
    avatar: string | null;
    description: string | null;
    follower_count: number | null;
    verified: boolean;
  };
  type ChannelVideo = {
    id: string;
    url: string;
    title: string;
    duration: number | null;
    view_count: number | null;
    thumbnail: string | null;
    live_status: string | null;
  };
  type ChannelPage = {
    channel: ChannelInfo;
    videos: ChannelVideo[];
    page: number;
    has_more: boolean;
  };
  type LibraryVideo = { id: number; url: string };
  type Subscription = { channel_id: string };

  const STEP_LABELS = ["获取视频信息", "下载视频", "处理字幕/音轨", "完成"];

  let data = $state<ChannelPage | null>(null);
  let loading = $state(false);
  let error = $state("");
  let library = $state(new Map<string, number>()); // url → 本地视频 id
  let subscribed = $state(false);
  let importingUrl = $state<string | null>(null);
  let progress = $state<{
    step: number;
    total_steps: number;
    label: string;
    percent: number | null;
    speed: string | null;
    detail: string | null;
  } | null>(null);

  const channelUrl = $derived($page.url.searchParams.get("url") ?? "");
  const overallPct = $derived(
    progress == null
      ? 0
      : Math.min(
          100,
          ((progress.step + (progress.percent ?? 0) / 100) /
            progress.total_steps) *
            100,
        ),
  );

  async function loadPage(p: number) {
    if (!channelUrl) {
      error = "缺少频道链接";
      return;
    }
    loading = true;
    error = "";
    try {
      data = await invoke<ChannelPage>("fetch_channel", { url: channelUrl, page: p });
      subscribed = await invoke<Subscription[]>("list_subscriptions")
        .then((subs) => subs.some((s) => s.channel_id === data!.channel.channel_id))
        .catch(() => false);
      window.scrollTo({ top: 0 });
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function toggleSubscribe() {
    const ch = data?.channel;
    if (!ch) return;
    try {
      if (subscribed) {
        await invoke("unsubscribe_channel", { channelId: ch.channel_id });
        subscribed = false;
      } else {
        await invoke("subscribe_channel", { channel: ch });
        subscribed = true;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function importOne(v: ChannelVideo) {
    if (importingUrl) return;
    error = "";
    importingUrl = v.url;
    progress = { step: 0, total_steps: 4, label: "准备中…", percent: null, speed: null, detail: null };
    try {
      const video = await invoke<LibraryVideo>("import_video", { url: v.url });
      const next = new Map(library);
      next.set(v.url, video.id);
      library = next;
    } catch (e) {
      error = String(e);
    } finally {
      importingUrl = null;
      progress = null;
    }
  }

  function fmtDuration(secs: number | null): string {
    if (secs == null) return "";
    const s = Math.round(secs);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const r = s % 60;
    return h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(r).padStart(2, "0")}`
      : `${m}:${String(r).padStart(2, "0")}`;
  }

  onMount(() => {
    invoke<LibraryVideo[]>("list_videos")
      .then((vs) => (library = new Map(vs.map((v) => [v.url, v.id]))))
      .catch(() => {});
    loadPage(1);
    const unlisten = listen<{
      step: number;
      total_steps: number;
      label: string;
      percent: number | null;
      speed: string | null;
      detail: string | null;
    }>("import-progress", (e) => {
      if (importingUrl) progress = e.payload;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

<div class="mx-auto max-w-4xl">
  <!-- 面包屑 -->
  <nav class="mb-3 flex items-center gap-1.5 text-xs text-zinc-400">
    <a href="/" class="hover:text-indigo-600">媒体库</a>
    <span>/</span>
    <span class="text-zinc-500">频道</span>
    {#if data}
      <span>/</span>
      <span class="max-w-64 truncate text-zinc-600" title={data.channel.title}>
        {data.channel.title}
      </span>
    {/if}
  </nav>

  {#if error}
    <p class="mb-4 rounded-md bg-red-50 px-3 py-2 text-sm whitespace-pre-wrap text-red-600">
      {error}
    </p>
    {#if !data}
      <button
        onclick={() => loadPage(1)}
        class="rounded-md border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-50"
      >重试</button
      >
    {/if}
  {/if}

  {#if data}
    <!-- 频道头 -->
    <div class="mb-5 flex items-start gap-4 rounded-xl border border-zinc-200 bg-white p-4 shadow-sm">
      {#if data.channel.avatar}
        <img
          src={data.channel.avatar}
          alt=""
          class="h-16 w-16 shrink-0 rounded-full object-cover"
        />
      {:else}
        <span class="flex h-16 w-16 shrink-0 items-center justify-center rounded-full bg-indigo-100 text-xl font-semibold text-indigo-600">
          {data.channel.title.slice(0, 1)}
        </span>
      {/if}
      <div class="min-w-0 flex-1">
        <h2 class="flex items-center gap-1.5 text-base font-semibold text-zinc-900">
          <span class="truncate">{data.channel.title}</span>
          {#if data.channel.verified}
            <svg class="h-4 w-4 shrink-0 text-indigo-500" viewBox="0 0 24 24" fill="currentColor" aria-label="已认证"><path d="M12 2 9.9 4.6l-3.4-.4-.5 3.4L2.6 9.4l1.6 3-1.6 3 3.4.8.5 3.4 3.4-.4L12 22l2.1-2.6 3.4.4.5-3.4 3.4-.8-1.6-3 1.6-3-3.4-.8-.5-3.4-3.4.4Z" /><path d="m10.6 13.4-1.8-1.8-1 1 2.8 2.8 4.8-4.8-1-1z" fill="#fff" /></svg>
          {/if}
        </h2>
        <p class="mt-0.5 text-xs text-zinc-500">
          {#if data.channel.follower_count != null}
            {fmtCount(data.channel.follower_count)} 订阅
          {/if}
        </p>
        {#if data.channel.description}
          <p class="mt-1.5 line-clamp-2 text-xs leading-relaxed text-zinc-500" title={data.channel.description}>
            {data.channel.description}
          </p>
        {/if}
      </div>
      <button
        onclick={toggleSubscribe}
        class="shrink-0 rounded-md px-3.5 py-1.5 text-xs font-medium {subscribed
          ? 'border border-zinc-200 text-zinc-500 hover:bg-zinc-50'
          : 'bg-indigo-600 text-white hover:bg-indigo-500'}"
      >{subscribed ? "已订阅 ✓" : "订阅"}</button
      >
    </div>

    <!-- 视频列表 -->
    {#if loading}
      <ul class="space-y-2">
        {#each Array(10) as _}
          <li class="flex animate-pulse gap-4 rounded-xl border border-zinc-200 bg-white p-3">
            <div class="h-20 w-36 shrink-0 rounded-lg bg-zinc-100"></div>
            <div class="flex-1 space-y-2 py-1">
              <div class="h-4 w-2/3 rounded bg-zinc-100"></div>
              <div class="h-3 w-1/3 rounded bg-zinc-100"></div>
            </div>
          </li>
        {/each}
      </ul>
    {:else if data.videos.length === 0}
      <p class="text-sm text-zinc-400">该频道暂无视频。</p>
    {:else}
      <ul class="space-y-2">
        {#each data.videos as v (v.id)}
          {@const importedId = library.get(v.url)}
          {@const importing = importingUrl === v.url}
          <li
            class="flex gap-4 rounded-xl border border-zinc-200 bg-white p-3 shadow-sm {importedId
              ? ''
              : 'transition-shadow hover:shadow-md'}"
          >
            <div class="relative block shrink-0">
              {#if v.thumbnail}
                <img
                  src={v.thumbnail}
                  alt=""
                  class="h-20 w-36 rounded-lg object-cover"
                  loading="lazy"
                />
              {:else}
                <div class="flex h-20 w-36 items-center justify-center rounded-lg bg-zinc-100 text-xs text-zinc-400">
                  无封面
                </div>
              {/if}
              {#if v.duration}
                <span class="absolute right-1 bottom-1 rounded bg-black/70 px-1 py-0.5 font-mono text-[10px] text-white">
                  {fmtDuration(v.duration)}
                </span>
              {/if}
            </div>

            <div class="min-w-0 flex-1">
              {#if importedId}
                <a
                  href="/player/{importedId}"
                  class="block truncate text-sm font-semibold text-zinc-900 hover:text-indigo-600"
                  title={v.title}>{v.title}</a
                >
              {:else}
                <p class="truncate text-sm font-semibold text-zinc-900" title={v.title}>
                  {v.title}
                </p>
              {/if}
              <div class="mt-1.5 flex items-center gap-2 text-xs">
                {#if v.view_count != null}
                  <span class="text-zinc-500">{fmtCount(v.view_count)} 次观看</span>
                {/if}
                {#if v.live_status === "is_upcoming"}
                  <span class="rounded bg-amber-50 px-1.5 py-0.5 text-[11px] font-medium text-amber-600">预告</span>
                {/if}
                {#if importedId}
                  <span class="rounded bg-green-50 px-1.5 py-0.5 text-[11px] font-medium text-green-600">已导入</span>
                {/if}
              </div>
              {#if importing && progress}
                <!-- 导入迷你进度：总进度条 + 当前步骤 + 字节进度/网速 -->
                <div class="mt-2">
                  <div class="mb-1 flex items-center justify-between text-[11px]">
                    <span class="text-zinc-500">{progress.label}</span>
                    <span class="flex items-center gap-2 font-mono text-zinc-400">
                      {#if progress.detail}
                        <span>{progress.detail}</span>
                      {/if}
                      {#if progress.speed}
                        <span>{progress.speed}</span>
                      {/if}
                      <span>{overallPct.toFixed(0)}%</span>
                    </span>
                  </div>
                  <div class="h-1 w-full overflow-hidden rounded-full bg-zinc-100">
                    <div
                      class="h-full rounded-full bg-indigo-500 transition-all duration-300"
                      style="width: {overallPct}%"
                    ></div>
                  </div>
                </div>
              {/if}
            </div>

            <div class="flex shrink-0 items-center">
              {#if importedId}
                <a
                  href="/player/{importedId}"
                  class="rounded-md p-1.5 text-zinc-400 hover:bg-indigo-50 hover:text-indigo-600"
                  title="打开播放器"
                  aria-label="打开播放器"
                >
                  <svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5.14v13.72a1 1 0 0 0 1.52.86l11-6.86a1 1 0 0 0 0-1.72l-11-6.86A1 1 0 0 0 8 5.14Z" /></svg>
                </a>
              {:else if v.live_status === "is_upcoming"}
                <span class="px-2 text-xs text-zinc-300">不可导入</span>
              {:else}
                <button
                  onclick={() => importOne(v)}
                  disabled={importingUrl != null}
                  class="flex items-center gap-1.5 rounded-md bg-indigo-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
                  title="下载并导入媒体库"
                >
                  {#if importing}
                    导入中…
                  {:else}
                    <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="m7 10 5 5 5-5" /><path d="M12 15V3" /></svg>
                    导入
                  {/if}
                </button>
              {/if}
            </div>
          </li>
        {/each}
      </ul>

      <!-- 翻页 -->
      {@const cur = data.page}
      <div class="mt-5 flex items-center justify-center gap-3">
        <button
          onclick={() => loadPage(cur - 1)}
          disabled={cur <= 1 || loading}
          class="rounded-md border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-50 disabled:opacity-40"
        >上一页</button
        >
        <span class="text-xs text-zinc-400">第 {cur} 页</span>
        <button
          onclick={() => loadPage(cur + 1)}
          disabled={!data.has_more || loading}
          class="rounded-md border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 hover:bg-zinc-50 disabled:opacity-40"
        >下一页</button
        >
      </div>
    {/if}
  {:else if loading}
    <div class="flex items-center justify-center p-12">
      <p class="thinking-text thinking-dots text-sm font-medium">拉取频道信息</p>
    </div>
  {/if}
</div>
