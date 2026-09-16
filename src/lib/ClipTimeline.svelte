<script lang="ts">
  /** CapCut 式剪辑条：缩略图胶片 + 双把手选区 + 播放头联动 */
  let {
    duration,
    thumbs,
    start = $bindable(),
    end = $bindable(),
    currentTime,
    onseek,
  }: {
    duration: number;
    thumbs: string[];
    start: number;
    end: number;
    currentTime: number;
    onseek: (secs: number) => void;
  } = $props();

  const MIN_GAP = 0.5;
  let trackEl: HTMLDivElement | undefined = $state();
  let drag: {
    mode: "start" | "end" | "move";
    downTime: number;
    origStart: number;
    origEnd: number;
    moved: boolean;
  } | null = null;

  const startPct = $derived(duration > 0 ? (start / duration) * 100 : 0);
  const endPct = $derived(duration > 0 ? (end / duration) * 100 : 100);
  const playPct = $derived(
    duration > 0 ? Math.min(100, (currentTime / duration) * 100) : 0,
  );

  function timeAt(e: PointerEvent): number {
    const rect = trackEl!.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    return frac * duration;
  }

  function onPointerDown(e: PointerEvent) {
    if (!trackEl || duration <= 0) return;
    const rect = trackEl.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const sx = (start / duration) * rect.width;
    const ex = (end / duration) * rect.width;
    const t = timeAt(e);
    let mode: "start" | "end" | "move" | null = null;
    if (Math.abs(x - sx) <= 10) mode = "start";
    else if (Math.abs(x - ex) <= 10) mode = "end";
    else if (t > start && t < end) mode = "move";
    if (mode === null) {
      onseek(t); // 选区外点击：只移动播放头
      return;
    }
    drag = { mode, downTime: t, origStart: start, origEnd: end, moved: false };
    trackEl.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onPointerMove(e: PointerEvent) {
    if (!drag) return;
    const t = timeAt(e);
    if (Math.abs(t - drag.downTime) > 0.02) drag.moved = true;
    if (!drag.moved) return;
    if (drag.mode === "start") {
      start = Math.min(Math.max(0, t), end - MIN_GAP);
      onseek(start);
    } else if (drag.mode === "end") {
      end = Math.max(Math.min(duration, t), start + MIN_GAP);
      onseek(end);
    } else {
      const len = drag.origEnd - drag.origStart;
      const ns = Math.min(
        Math.max(0, drag.origStart + (t - drag.downTime)),
        duration - len,
      );
      start = ns;
      end = ns + len;
      onseek(start);
    }
  }

  function onPointerUp(e: PointerEvent) {
    if (!drag) return;
    // 选区内单击（未拖动）：移动播放头
    if (drag.mode === "move" && !drag.moved) onseek(timeAt(e));
    drag = null;
  }

  function fmt(secs: number): string {
    const m = Math.floor(Math.max(0, secs) / 60);
    const s = Math.max(0, secs) - m * 60;
    return `${m}:${s.toFixed(1).padStart(4, "0")}`;
  }
</script>

<div>
  <div
    bind:this={trackEl}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    class="relative h-16 cursor-pointer overflow-hidden rounded-lg bg-zinc-100 select-none"
    style="touch-action: none"
    role="slider"
    aria-label="剪辑区间"
    aria-valuemin={0}
    aria-valuemax={duration}
    aria-valuenow={start}
    tabindex="-1"
  >
    <!-- 胶片条 -->
    {#if thumbs.length > 0}
      <div class="flex h-full w-full">
        {#each thumbs as t (t)}
          <img
            src={t}
            alt=""
            draggable="false"
            class="h-full min-w-0 flex-1 object-cover"
          />
        {/each}
      </div>
    {:else}
      <div class="flex h-full items-center justify-center">
        <p class="thinking-text thinking-dots text-xs font-medium">生成预览图</p>
      </div>
    {/if}

    <!-- 选区外遮罩 -->
    <div
      class="pointer-events-none absolute inset-y-0 left-0 bg-zinc-900/45"
      style="width: {startPct}%"
    ></div>
    <div
      class="pointer-events-none absolute inset-y-0 bg-zinc-900/45"
      style="left: {endPct}%; width: {Math.max(0, 100 - endPct)}%"
    ></div>

    <!-- 选区边框 -->
    <div
      class="pointer-events-none absolute inset-y-0 rounded border-2 border-indigo-500"
      style="left: {startPct}%; width: {Math.max(0, endPct - startPct)}%"
    ></div>

    <!-- 左右把手（带 I/O 标记旗） -->
    <div
      class="absolute inset-y-0 z-10 flex w-3.5 -translate-x-1/2 cursor-ew-resize items-center justify-center rounded-md border border-indigo-500 bg-white shadow-sm"
      style="left: {startPct}%"
    >
      <span class="absolute top-0 left-1/2 -translate-x-1/2 rounded-b-[3px] bg-indigo-500 px-[3px] text-[8px] font-bold leading-[11px] text-white select-none">I</span>
      <svg class="h-3 w-3 text-indigo-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 5v14M15 5v14" /></svg>
    </div>
    <div
      class="absolute inset-y-0 z-10 flex w-3.5 -translate-x-1/2 cursor-ew-resize items-center justify-center rounded-md border border-indigo-500 bg-white shadow-sm"
      style="left: {endPct}%"
    >
      <span class="absolute top-0 left-1/2 -translate-x-1/2 rounded-b-[3px] bg-indigo-500 px-[3px] text-[8px] font-bold leading-[11px] text-white select-none">O</span>
      <svg class="h-3 w-3 text-indigo-500" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 5v14M15 5v14" /></svg>
    </div>

    <!-- 播放头 -->
    <div
      class="pointer-events-none absolute inset-y-0 z-10 w-0.5 -translate-x-1/2 bg-white shadow-[0_0_3px_rgba(0,0,0,0.8)]"
      style="left: {playPct}%"
    ></div>
  </div>

  <div class="mt-1.5 flex items-center gap-2 text-xs text-zinc-500">
    <span class="rounded bg-zinc-100 px-1.5 py-0.5 font-mono">{fmt(start)}</span>
    <span class="text-zinc-300">→</span>
    <span class="rounded bg-zinc-100 px-1.5 py-0.5 font-mono">{fmt(end)}</span>
    <span class="text-zinc-400">共 {fmt(end - start)}</span>
  </div>
</div>
