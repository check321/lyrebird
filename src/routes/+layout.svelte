<script lang="ts">
  import "../app.css";
  import { invoke } from "@tauri-apps/api/core";
  import { page } from "$app/stores";
  import { ui } from "$lib/ui.svelte";
  import { onMount } from "svelte";

  let { children } = $props();

  type VideoBrief = { id: number; title: string | null; url: string; video_path: string | null };

    let recent = $state<VideoBrief[]>([]);

  // 每次路由变化都刷新侧边栏的媒体记录（下载中断的条目不出现在快捷列表）
  $effect(() => {
    $page.url.pathname;
    invoke<VideoBrief[]>("list_videos")
      .then((v) => (recent = v.filter((x) => x.video_path != null).slice(0, 8)))
      .catch(() => {});
  });
</script>

<div class="flex h-screen bg-zinc-50 text-zinc-800">
  {#if !ui.sidebarCollapsed}
    <nav
      class="flex w-56 shrink-0 flex-col gap-1 border-r border-zinc-200 bg-white p-4"
    >
      <div class="mb-4 flex items-center justify-between px-2">
        <div class="flex items-center gap-2">
          <img
            src="/logo.jpg"
            alt="Lyrebird"
            class="h-7 w-7 rounded-md object-cover"
          />
          <h1 class="text-lg font-semibold tracking-wide text-zinc-800">
            Lyrebird
          </h1>
        </div>
        <button
          onclick={() => (ui.sidebarCollapsed = true)}
          class="rounded-md p-1 text-zinc-400 hover:bg-zinc-100 hover:text-zinc-600"
          title="收起侧边栏"
          aria-label="收起侧边栏"
        >
          <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <rect width="18" height="18" x="3" y="3" rx="2" /><path d="M9 3v18" /><path d="m14 9-3 3 3 3" />
          </svg>
        </button>
      </div>

      <a
        href="/"
        class="rounded-md px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
        >媒体库</a
      >
      {#if recent.length > 0}
        <div class="mb-2 ml-2 space-y-0.5 border-l border-zinc-100 pl-2">
          {#each recent as v (v.id)}
            <a
              href="/player/{v.id}"
              title={v.title ?? v.url}
              class="block truncate rounded px-2 py-1 text-xs {$page.url
                .pathname === `/player/${v.id}`
                ? 'bg-indigo-50 font-medium text-indigo-700'
                : 'text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800'}"
              >{v.title ?? v.url}</a
            >
          {/each}
        </div>
      {/if}

      <a
        href="/words"
        class="rounded-md px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
        >生词本</a
      >

      <a
        href="/settings"
        class="rounded-md px-3 py-2 text-sm font-medium text-zinc-700 hover:bg-zinc-100"
        >设置</a
      >
    </nav>
  {:else}
    <button
      onclick={() => (ui.sidebarCollapsed = false)}
      class="absolute top-3 left-3 z-30 rounded-md border border-zinc-200 bg-white p-1.5 text-zinc-400 shadow-sm hover:text-zinc-700"
      title="展开侧边栏"
      aria-label="展开侧边栏"
    >
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <rect width="18" height="18" x="3" y="3" rx="2" /><path d="M9 3v18" /><path d="m14 9 3 3-3 3" />
      </svg>
    </button>
  {/if}
  <main class="flex-1 overflow-y-auto p-6">
    {@render children()}
  </main>
</div>
