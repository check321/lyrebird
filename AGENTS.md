# Lyrebird

通过 YouTube 播客学英语的 macOS 桌面 App（M 芯片）。

## 技术栈

- **壳/UI**：Tauri 2 + Svelte 5（SvelteKit，SPA 模式）+ TypeScript + Tailwind CSS 4
- **后端**：Rust（`src-tauri/src/`），Tauri commands + tokio 异步
- **数据库**：SQLite via sqlx（迁移在 `src-tauri/migrations/`，启动时自动执行）
- **下载**：yt-dlp 子进程（cookies 认证走设置页）；视频强制 H.264(avc1)+AAC+mp4（WKWebView 兼容性要求，勿改）
- **媒体播放**：内置 axum HTTP 服务器（127.0.0.1，`/media/`）提供视频流（Range 请求）；不要用 asset 协议播放大视频（有画面无声音的兼容问题）
- **ASR**：`python/asr_worker.py`（MOSS-Transcribe-Diarize）由 Rust 子进程调用；Python 环境在项目 `.venv-asr/`（`scripts/setup_asr.sh` 创建）
- **LLM**：OpenAI 兼容协议（默认 DeepSeek），`llm.rs` 统一入口，配置存 settings 表
- **字幕渲染**：播放器内嵌字幕由 libass-wasm（JavascriptSubtitlesOctopus）渲染 ASS（`src/lib/ass.ts` 负责 cue→ASS 生成，含 CapCut 式预设 `SUBTITLE_PRESETS`）；字幕样式**不进设置页表单**，由播放页右上「Aa」弹出的 `src/lib/SubtitleStylePanel.svelte` 就地调节、实时渲染并防抖持久化；字体文件在 `static/fonts/`，页面 `@font-face` 与 libass 必须用同一份
- **词典**：本地 ECDICT（stardict.db），按需 AI 上下文解析补充；查词发音统一走 `src/lib/speech.ts`（有道在线真人发音，失败/超时回退 Rust `speak_word`，即 `say -v` 英文语音离线兜底）

## 常用命令

- 开发：`npm run tauri dev`
- 前端检查：`npm run check`；后端：`cd src-tauri && cargo check` / `cargo test`
- 安装 ASR 环境：`scripts/setup_asr.sh`

## 设计规范（所有 UI 变更必须遵守）

**色彩**：浅色系。页面底 `bg-zinc-50`，卡片/侧边栏 `bg-white` + `border-zinc-200` + `shadow-sm`；主色 `indigo-600`（主按钮/链接 hover/激活态），成功 `green-600`，警示 `amber`，错误 `red-500/600` + `bg-red-50` 错误条。

**文字层级**（以媒体卡片为基准）：
- 标题：`text-sm font-semibold text-zinc-900`，超长 `truncate`（title 属性兜底）
- 简介/次级描述：`text-xs text-zinc-500`，最多 `line-clamp-2`
- 元信息：`text-xs`；频道名 `font-medium text-indigo-600`；时长用 `font-mono` + `bg-zinc-100` 徽章；状态类徽章带语义底色（如 `bg-green-50 text-green-600`）
- 弱化信息（时间戳等）：`text-zinc-300/400`

**间距/对齐**：卡片 `rounded-xl border p-3 gap-4`，区块间距 `space-y-2`（列表）/ `space-y-8`（分组）；表单控件 `rounded-md border-zinc-300 px-3 py-2 text-sm`，focus 态 `border-indigo-500 ring-1 ring-indigo-500`。

**图标**：操作一律用内联 SVG 图标按钮（Lucide 风格，24 viewBox、stroke-width 2、`h-4 w-4`），配 `title` + `aria-label`；禁止纯文字操作按钮出现在列表项里。hover 给 `rounded-md` 底色反馈，危险操作 hover `bg-red-50 text-red-500`。

**交互约定**：
- 外观/样式类调节优先「就地、所见即所得」：在使用场景旁放图标按钮弹出紧凑面板（参照播放页 `SubtitleStylePanel`），改动实时生效并防抖持久化；禁止把样式项罗列成设置页长表单
- 开关/档位类选项用图标按钮（如加粗 B）或分段控件（segmented control），颜色用色板圆点 + 自定义取色「+」，不用宽下拉框
- 长任务必须有实时进度：多步骤任务用「总进度条（跨步骤单调递增，不回退）+ 子步骤列表（✓/序号/当前步骤标签）」
- 弹窗：点击遮罩或 Esc 关闭；删除等破坏性操作先 `confirm` 确认
- 表单类配置全部进设置页：路径类字段必须带「浏览…」系统对话框选择，密钥类默认密码框可显隐，能在线验证的配置提供「测试连接/检查」按钮
- 播放体验：断点续播（位置存 `videos.position_secs`）、视频内嵌字幕走 libass（样式由播放页「Aa」面板控制，ASS 生成逻辑统一在 `src/lib/ass.ts`）、字幕与播放轴双向绑定；空格键 = 播放/暂停（输入控件聚焦时不触发）；**沉浸模式下暂停时右侧渐显半透明阅读区**（覆盖视频，`bg-white/80 backdrop-blur`，复用 `src/lib/ReadingPanel.svelte`，普通右侧栏与其必须保持同一数据源）；**播放控制条布局固定为两行**——时间轴滑杆独占第一行（w-full），控件第二行（可 flex-wrap），禁止把时间轴和控件挤在同一行
