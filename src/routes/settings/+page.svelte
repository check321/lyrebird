<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { onMount } from "svelte";

  type FieldType = "text" | "password" | "file" | "dir" | "select" | "color";
  type Field = {
    key: string;
    label: string;
    type: FieldType;
    placeholder?: string;
    hint?: string;
    /** 文件选择对话框的过滤器 */
    dialogFilter?: { name: string; extensions: string[] }[];
    /** select 类型的选项 */
    options?: { value: string; label: string }[];
  };
  type Group = {
    key: string;
    title: string;
    description?: string;
    fields: Field[];
  };

  const groups: Group[] = [
    {
      key: "llm",
      title: "LLM 服务",
      description: "翻译、TLDR、单词解析都走这里配置的 OpenAI 兼容接口。",
      fields: [
        {
          key: "llm_base_url",
          label: "Base URL",
          type: "text",
          placeholder: "https://api.deepseek.com",
          hint: "留空则使用默认 DeepSeek 官方接口",
        },
        { key: "llm_api_key", label: "API Key", type: "password" },
        {
          key: "llm_model",
          label: "模型",
          type: "text",
          placeholder: "deepseek-chat",
        },
      ],
    },
    {
      key: "asr",
      title: "本地语音识别（ASR）",
      description:
        "视频没有自带字幕时，用 moss-transcribe-diarize 在本地转写英文字幕。",
      fields: [
        {
          key: "asr_python_path",
          label: "Python 解释器",
          type: "file",
          placeholder: "留空则使用 PATH 中的 python3",
          hint: "安装好 moss-transcribe-diarize 依赖的那个 Python 环境",
        },
        {
          key: "asr_model_path",
          label: "模型权重目录",
          type: "dir",
          placeholder: "留空则使用默认缓存目录",
        },
      ],
    },
    {
      key: "tools",
      title: "外部工具",
      description: "下载视频与音频处理用的外部程序，留空则从 PATH 自动查找。",
      fields: [
        {
          key: "ytdlp_path",
          label: "yt-dlp",
          type: "file",
          placeholder: "留空则使用 PATH 中的 yt-dlp",
        },
        {
          key: "ytdlp_cookies_browser",
          label: "Cookies 来源：从浏览器读取",
          type: "select",
          options: [
            { value: "", label: "不使用" },
            { value: "safari", label: "Safari" },
            { value: "chrome", label: "Chrome" },
            { value: "edge", label: "Edge" },
            { value: "firefox", label: "Firefox" },
            { value: "brave", label: "Brave" },
          ],
          hint: "选择你日常登录了 YouTube 的浏览器（Arc/Dia 因 cookies 加密方式特殊，yt-dlp 不支持，请改用下方 cookies.txt 方式）",
        },
        {
          key: "ytdlp_cookies_file",
          label: "Cookies 来源：cookies.txt 文件",
          type: "file",
          placeholder: "未配置",
          hint: "在浏览器装「Get cookies.txt LOCALLY」插件，登录 YouTube 后导出 youtube.com 的 cookies 并在此选择该文件",
          dialogFilter: [{ name: "Cookies 文件", extensions: ["txt"] }],
        },
        {
          key: "ffmpeg_path",
          label: "ffmpeg",
          type: "file",
          placeholder: "留空则使用 PATH 中的 ffmpeg",
        },
      ],
    },
    {
      key: "dict",
      title: "词典",
      description: "点词查询用的词典。ECDict 提供简明释义；可再导入 MDX 词典（欧路同源 .mdx 格式，如朗文/柯林斯）获得详细释义与例句。",
      fields: [
        {
          key: "dict_path",
          label: "ECDict 词典文件",
          type: "file",
          placeholder: "未配置",
          dialogFilter: [{ name: "SQLite 数据库", extensions: ["sqlite", "db"] }],
        },
        {
          key: "mdx_path",
          label: "MDX 词典文件",
          type: "file",
          placeholder: "未配置",
          hint: "选择 .mdx 文件后点击下方「导入词典」建立索引",
          dialogFilter: [{ name: "MDX 词典", extensions: ["mdx"] }],
        },
      ],
    },
  ];

  let values = $state<Record<string, string>>({});
  let showKey = $state(false);
  let saving = $state(false);
  let savedOk = $state(false);
  let testing = $state(false);
  let testResult = $state<{ ok: boolean; msg: string } | null>(null);
  let asrChecking = $state(false);
  let asrResult = $state<{ ok: boolean; msg: string } | null>(null);
  let mdxImporting = $state(false);
  let mdxProgress = $state<string | null>(null);
  let mdxSources = $state<[string, string | null, number][]>([]);

  onMount(() => {
    let unlisten: (() => void) | undefined;
    (async () => {
      values = await invoke<Record<string, string>>("get_settings");
      mdxSources = await invoke("list_mdx_sources");
      unlisten = await listen<{ stage: string; entries: number }>(
        "mdx-import-progress",
        (e) => {
          mdxProgress =
            e.payload.stage === "完成"
              ? `导入完成：${e.payload.entries} 词条`
              : `${e.payload.stage}（已导入 ${e.payload.entries}）`;
        },
      );
    })();
    return () => unlisten?.();
  });

  async function importMdx() {
    if (!values.mdx_path?.trim()) {
      mdxProgress = "请先选择 .mdx 文件";
      return;
    }
    mdxImporting = true;
    mdxProgress = null;
    await invoke("save_settings", { values });
    try {
      const n = await invoke<number>("import_mdx", { path: values.mdx_path.trim() });
      mdxProgress = `导入完成：${n} 词条`;
      mdxSources = await invoke("list_mdx_sources");
    } catch (e) {
      mdxProgress = String(e);
    } finally {
      mdxImporting = false;
    }
  }

  async function browse(f: Field) {
    const selected = await openDialog({
      directory: f.type === "dir",
      multiple: false,
      filters: f.dialogFilter,
    });
    if (typeof selected === "string") {
      values[f.key] = selected;
    }
  }

  async function save() {
    saving = true;
    savedOk = false;
    try {
      await invoke("save_settings", { values });
      savedOk = true;
      setTimeout(() => (savedOk = false), 2500);
    } finally {
      saving = false;
    }
  }

  async function checkAsr() {
    asrChecking = true;
    asrResult = null;
    // 先把表单里的最新值存进去，再检查
    await invoke("save_settings", { values });
    try {
      const msg = await invoke<string>("check_asr_env");
      asrResult = { ok: true, msg };
    } catch (e) {
      asrResult = { ok: false, msg: String(e) };
    } finally {
      asrChecking = false;
    }
  }

  async function testConnection() {
    testing = true;
    testResult = null;
    try {
      const msg = await invoke<string>("test_llm_connection", {
        baseUrl:
          values.llm_base_url?.trim() || "https://api.deepseek.com",
        apiKey: values.llm_api_key ?? "",
        model: values.llm_model?.trim() || "deepseek-chat",
      });
      testResult = { ok: true, msg };
    } catch (e) {
      testResult = { ok: false, msg: String(e) };
    } finally {
      testing = false;
    }
  }
</script>

<div class="mx-auto max-w-2xl">
  <h2 class="mb-6 text-xl font-semibold text-zinc-800">设置</h2>

  <div class="space-y-6 pb-24">
    {#each groups as g (g.key)}
      <section
        class="rounded-xl border border-zinc-200 bg-white p-5 shadow-sm"
      >
        <h3 class="text-sm font-semibold text-zinc-800">{g.title}</h3>
        {#if g.description}
          <p class="mt-1 mb-4 text-xs leading-relaxed text-zinc-400">
            {g.description}
          </p>
        {:else}
          <div class="mb-4"></div>
        {/if}

        <div class="space-y-4">
          {#each g.fields as f (f.key)}
            <div>
              <label for={f.key} class="mb-1.5 block text-sm text-zinc-600"
                >{f.label}</label
              >
              <div class="flex gap-2">
                {#if f.type === "select"}
                  <select
                    id={f.key}
                    bind:value={values[f.key]}
                    class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-800 outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500"
                  >
                    {#each f.options ?? [] as opt (opt.value)}
                      <option value={opt.value}>{opt.label}</option>
                    {/each}
                  </select>
                {:else if f.type === "color"}
                  <div class="flex items-center gap-2">
                    <input
                      id={f.key}
                      type="color"
                      value={values[f.key] || "#000000"}
                      oninput={(e) =>
                        (values[f.key] = (e.target as HTMLInputElement).value)}
                      class="h-9 w-14 cursor-pointer rounded-md border border-zinc-300 bg-white p-1"
                    />
                    <span class="font-mono text-xs text-zinc-400">
                      {values[f.key] || "默认"}
                    </span>
                  </div>
                {:else}
                  <div class="relative flex-1">
                    <input
                      id={f.key}
                      type={f.type === "password" && !showKey
                        ? "password"
                        : "text"}
                      placeholder={f.placeholder ?? ""}
                      bind:value={values[f.key]}
                      spellcheck="false"
                      class="w-full rounded-md border border-zinc-300 bg-white px-3 py-2 pr-9 text-sm text-zinc-800 outline-none focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500"
                    />
                    {#if f.type === "password"}
                      <button
                        type="button"
                        onclick={() => (showKey = !showKey)}
                        class="absolute top-1/2 right-2.5 -translate-y-1/2 text-xs text-zinc-400 hover:text-zinc-600"
                        >{showKey ? "隐藏" : "显示"}</button
                      >
                    {/if}
                  </div>
                {/if}
                {#if f.type === "file" || f.type === "dir"}
                  <button
                    type="button"
                    onclick={() => browse(f)}
                    class="shrink-0 rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm text-zinc-600 hover:bg-zinc-50"
                    >浏览…</button
                  >
                {/if}
              </div>
              {#if f.hint}
                <p class="mt-1 text-xs text-zinc-400">{f.hint}</p>
              {/if}
            </div>
          {/each}

          {#if g.key === "llm"}
            <div class="flex items-center gap-3 border-t border-zinc-100 pt-4">
              <button
                type="button"
                onclick={testConnection}
                disabled={testing}
                class="rounded-md border border-indigo-600 px-3 py-1.5 text-sm font-medium text-indigo-600 hover:bg-indigo-50 disabled:opacity-50"
                >{testing ? "测试中…" : "测试连接"}</button
              >
              {#if testResult}
                <span
                  class="text-sm {testResult.ok
                    ? 'text-green-600'
                    : 'text-red-500'}">{testResult.msg}</span
                >
              {/if}
            </div>
          {/if}

          {#if g.key === "dict"}
            <div class="border-t border-zinc-100 pt-4">
              <div class="flex items-center gap-3">
                <button
                  type="button"
                  onclick={importMdx}
                  disabled={mdxImporting}
                  class="rounded-md border border-indigo-600 px-3 py-1.5 text-sm font-medium text-indigo-600 hover:bg-indigo-50 disabled:opacity-50"
                  >{mdxImporting ? "导入中…" : "导入 MDX 词典"}</button
                >
                {#if mdxProgress}
                  <span class="text-sm text-zinc-500">{mdxProgress}</span>
                {/if}
              </div>
              {#if mdxSources.length > 0}
                <ul class="mt-3 space-y-1">
                  {#each mdxSources as [path, title, entries] (path)}
                    <li class="text-xs text-zinc-500">
                      <span class="font-medium text-zinc-700">{title || path.split("/").pop()}</span>
                      · {entries} 词条
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}

          {#if g.key === "asr"}
            <div class="flex items-center gap-3 border-t border-zinc-100 pt-4">
              <button
                type="button"
                onclick={checkAsr}
                disabled={asrChecking}
                class="rounded-md border border-indigo-600 px-3 py-1.5 text-sm font-medium text-indigo-600 hover:bg-indigo-50 disabled:opacity-50"
                >{asrChecking ? "检查中…" : "检查 ASR 环境"}</button
              >
              {#if asrResult}
                <span
                  class="whitespace-pre-wrap text-sm {asrResult.ok
                    ? 'text-green-600'
                    : 'text-red-500'}">{asrResult.msg}</span
                >
              {/if}
            </div>
          {/if}
        </div>
      </section>
    {/each}
  </div>

  <div
    class="sticky bottom-0 -mx-6 flex items-center justify-end gap-3 border-t border-zinc-200 bg-zinc-50/90 px-6 py-4 backdrop-blur"
  >
    {#if savedOk}
      <span class="text-sm text-green-600">已保存</span>
    {/if}
    <button
      onclick={save}
      disabled={saving}
      class="rounded-md bg-indigo-600 px-5 py-2 text-sm font-medium text-white shadow-sm hover:bg-indigo-500 disabled:opacity-50"
      >{saving ? "保存中…" : "保存设置"}</button
    >
  </div>
</div>
