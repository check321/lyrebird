<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  type Video = {
    id: number;
    url: string;
    title: string | null;
    duration_secs: number | null;
    subtitle_source: string | null;
    tldr: string | null;
    created_at: string;
    channel: string | null;
    description: string | null;
    thumbnail_path: string | null;
    category: string;
  };

  let videos = $state<Video[]>([]);
  let url = $state("");
  let importing = $state(false);
  let progress = $state<{
    step: number;
    total_steps: number;
    label: string;
    percent: number | null;
  } | null>(null);
  let error = $state("");

  const STEP_LABELS = ["获取视频信息", "下载视频", "处理字幕/音轨", "完成"];

  /** 总进度：已完成步骤 + 当前步骤内百分比，不回退 */
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

  // 编辑弹窗
  let editing = $state<Video | null>(null);
  let editTitle = $state("");
  let editCategory = $state("");
  // 分类重命名
  let renamingCat = $state<string | null>(null);
  let newCatName = $state("");

  const groups = $derived(
    (() => {
      const map = new Map<string, Video[]>();
      for (const v of videos) {
        const list = map.get(v.category) ?? [];
        list.push(v);
        map.set(v.category, list);
      }
      return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]));
    })(),
  );
  const categories = $derived(groups.map(([name]) => name));

  async function refresh() {
    videos = await invoke<Video[]>("list_videos");
  }

  async function importVideo(event: Event) {
    event.preventDefault();
    error = "";
    importing = true;
    progress = { step: 0, total_steps: 4, label: "准备中…", percent: null };
    try {
      await invoke("import_video", { url });
      url = "";
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      importing = false;
      progress = null;
    }
  }

  function openEdit(v: Video) {
    editing = v;
    editTitle = v.title ?? "";
    editCategory = v.category;
  }

  async function saveEdit() {
    if (!editing) return;
    await invoke("update_video", {
      id: editing.id,
      title: editTitle.trim() || null,
      category: editCategory.trim() || null,
    });
    editing = null;
    await refresh();
  }

  async function removeVideo(v: Video) {
    if (!confirm(`确定删除「${v.title ?? v.url}」吗？本地文件会一并清除。`)) return;
    await invoke("delete_video", { id: v.id });
    videos = videos.filter((x) => x.id !== v.id);
  }

  async function saveRename(oldName: string) {
    const name = newCatName.trim();
    if (!name || name === oldName) {
      renamingCat = null;
      return;
    }
    await invoke("rename_category", { oldName, newName: name });
    renamingCat = null;
    await refresh();
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
    refresh();
    const unlisten = listen<{
      step: number;
      total_steps: number;
      label: string;
      percent: number | null;
    }>("import-progress", (e) => {
      progress = e.payload;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

<div class="mx-auto max-w-4xl">
  <h2 class="mb-4 text-xl font-semibold text-zinc-800">媒体库</h2>

  <form class="mb-4 flex gap-2" onsubmit={importVideo}>
    <input
      bind:value={url}
      placeholder="粘贴 YouTube 播客链接…"
      class="flex-1 rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500"
    />
    <button
      type="submit"
      disabled={importing}
      class="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
      >{importing ? "导入中…" : "导入"}</button
    >
  </form>

  {#if progress}
    <div class="mb-4 rounded-lg border border-zinc-200 bg-white p-4 shadow-sm">
      <!-- 总进度条：跨步骤单调递增 -->
      <div class="mb-3 flex items-center justify-between text-sm">
        <span class="font-medium text-zinc-700">导入中</span>
        <span class="text-zinc-400">{overallPct.toFixed(0)}%</span>
      </div>
      <div class="mb-3 h-2 w-full overflow-hidden rounded-full bg-zinc-100">
        <div
          class="h-full rounded-full bg-indigo-500 transition-all duration-300"
          style="width: {overallPct}%"
        ></div>
      </div>
      <!-- 子步骤列表 -->
      <ol class="space-y-1">
        {#each STEP_LABELS as label, i}
          {@const done = progress.step > i}
          {@const active = progress.step === i}
          <li class="flex items-center gap-2 text-xs">
            <span
              class="flex h-4 w-4 items-center justify-center rounded-full {done
                ? 'bg-green-100 text-green-600'
                : active
                  ? 'bg-indigo-100 text-indigo-600'
                  : 'bg-zinc-100 text-zinc-300'}"
            >
              {done ? "✓" : i + 1}
            </span>
            <span
              class={done
                ? "text-zinc-400"
                : active
                  ? "text-zinc-700"
                  : "text-zinc-300"}
            >
              {active ? progress.label : label}
              {#if active && progress.percent != null}
                <span class="text-zinc-400"> {progress.percent.toFixed(1)}%</span>
              {/if}
            </span>
          </li>
        {/each}
      </ol>
    </div>
  {/if}
  {#if error}
    <p class="mb-4 rounded-md bg-red-50 px-3 py-2 text-sm whitespace-pre-wrap text-red-600">
      {error}
    </p>
  {/if}

  {#if videos.length === 0}
    <p class="text-sm text-zinc-400">还没有视频，粘贴一个 YouTube 链接开始。</p>
  {:else}
    <div class="space-y-8">
      {#each groups as [category, list] (category)}
        <section>
          <div class="mb-3 flex items-center gap-2">
            {#if renamingCat === category}
              <input
                bind:value={newCatName}
                list="category-options"
                onkeydown={(e) => e.key === "Enter" && saveRename(category)}
                class="rounded-md border border-zinc-300 px-2 py-1 text-sm outline-none focus:border-indigo-500"
              />
              <button
                onclick={() => saveRename(category)}
                class="text-xs text-indigo-600 hover:underline">保存</button
              >
              <button
                onclick={() => (renamingCat = null)}
                class="text-xs text-zinc-400 hover:underline">取消</button
              >
            {:else}
              <h3 class="text-sm font-semibold text-zinc-700">{category}</h3>
              <span class="text-xs text-zinc-400">{list.length} 个视频</span>
              <button
                onclick={() => {
                  renamingCat = category;
                  newCatName = category;
                }}
                class="text-xs text-zinc-400 hover:text-indigo-600"
                title="重命名分类"
                >✎</button
              >
            {/if}
          </div>

          <ul class="space-y-2">
            {#each list as v (v.id)}
              <li
                class="flex gap-4 rounded-xl border border-zinc-200 bg-white p-3 shadow-sm"
              >
                <a href="/player/{v.id}" class="block shrink-0">
                  {#if v.thumbnail_path}
                    <img
                      src={convertFileSrc(v.thumbnail_path)}
                      alt=""
                      class="h-20 w-36 rounded-lg object-cover"
                    />
                  {:else}
                    <div
                      class="flex h-20 w-36 items-center justify-center rounded-lg bg-zinc-100 text-xs text-zinc-400"
                    >
                      无封面
                    </div>
                  {/if}
                </a>

                <div class="min-w-0 flex-1">
                  <a
                    href="/player/{v.id}"
                    class="block truncate text-sm font-semibold text-zinc-900 hover:text-indigo-600"
                    title={v.title ?? v.url}
                    >{v.title ?? v.url}</a
                  >
                  {#if v.description}
                    <p
                      class="mt-1 line-clamp-2 text-xs leading-relaxed text-zinc-500"
                      >{v.description}</p
                    >
                  {/if}
                  <div class="mt-2 flex items-center gap-2 text-xs">
                    <span class="font-medium text-indigo-600">
                      {v.channel ?? "未知频道"}
                    </span>
                    {#if v.duration_secs}
                      <span
                        class="rounded bg-zinc-100 px-1.5 py-0.5 font-mono text-[11px] text-zinc-500"
                        >{fmtDuration(v.duration_secs)}</span
                      >
                    {/if}
                    <span
                      class="rounded px-1.5 py-0.5 text-[11px] {v.subtitle_source ===
                      'asr'
                        ? 'bg-amber-50 text-amber-600'
                        : 'bg-green-50 text-green-600'}"
                      >{v.subtitle_source === "asr" ? "ASR 字幕" : "CC 字幕"}</span
                    >
                    <span class="ml-auto text-zinc-300">{v.created_at}</span>
                  </div>
                </div>

                <div class="flex shrink-0 flex-col justify-center gap-2">
                  <button
                    onclick={() => openEdit(v)}
                    class="rounded-md p-1.5 text-zinc-400 hover:bg-zinc-100 hover:text-indigo-600"
                    title="编辑"
                    aria-label="编辑"
                  >
                    <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
                    </svg>
                  </button>
                  <button
                    onclick={() => removeVideo(v)}
                    class="rounded-md p-1.5 text-zinc-400 hover:bg-red-50 hover:text-red-500"
                    title="删除"
                    aria-label="删除"
                  >
                    <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M3 6h18" /><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2" />
                    </svg>
                  </button>
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</div>

<!-- 编辑弹窗 -->
{#if editing}
  <div
    class="fixed inset-0 z-40 flex items-center justify-center bg-black/30"
    role="button"
    tabindex="0"
    onclick={(e) => e.target === e.currentTarget && (editing = null)}
    onkeydown={(e) => e.key === "Escape" && (editing = null)}
  >
    <div class="w-96 rounded-xl bg-white p-5 shadow-xl">
      <h3 class="mb-4 text-sm font-semibold text-zinc-800">编辑视频信息</h3>
      <label for="edit-title" class="mb-1 block text-xs text-zinc-500">标题</label>
      <input
        id="edit-title"
        bind:value={editTitle}
        class="mb-3 w-full rounded-md border border-zinc-300 px-3 py-2 text-sm outline-none focus:border-indigo-500"
      />
      <label for="edit-category" class="mb-1 block text-xs text-zinc-500">分类</label>
      <input
        id="edit-category"
        bind:value={editCategory}
        list="category-options"
        placeholder="输入新分类或选择已有分类"
        class="mb-4 w-full rounded-md border border-zinc-300 px-3 py-2 text-sm outline-none focus:border-indigo-500"
      />
      <div class="flex justify-end gap-2">
        <button
          onclick={() => (editing = null)}
          class="rounded-md px-3 py-1.5 text-sm text-zinc-500 hover:bg-zinc-100"
          >取消</button
        >
        <button
          onclick={saveEdit}
          class="rounded-md bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500"
          >保存</button
        >
      </div>
    </div>
  </div>
{/if}

<datalist id="category-options">
  {#each categories as c (c)}
    <option value={c}></option>
  {/each}
</datalist>
