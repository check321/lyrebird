# Lyrebird

通过 YouTube 播客学英语的 macOS 桌面 App（M 芯片）。

## 技术栈

- **壳/UI**：Tauri 2 + Svelte 5（SvelteKit，SPA 模式）+ TypeScript + Tailwind CSS 4
- **后端**：Rust（`src-tauri/src/`），Tauri commands + tokio 异步
- **数据库**：SQLite via sqlx（迁移在 `src-tauri/migrations/`，启动时自动执行）
- **下载**：yt-dlp 子进程（cookies 认证走设置页）；视频强制 H.264(avc1)+AAC+mp4（WKWebView 兼容性要求，勿改）；下载进度事件 `import-progress` 带实时网速 `speed` 与字节级进度 `detail`（解析 yt-dlp `--newline` 的 `[download]` 行；视频/音频/字幕分多趟下载各自从 0% 计，`DlTracker` 按字节聚合出真实总进度展示，百分比经单调钳制只供总进度条；字幕/封面小文件趟不计入）；**断点续传**：显式 `--continue` 保留 `.part` 文件，下载失败保留 `videos` 行（`video_path` 为空，媒体库显示「下载中断」卡 + 续传按钮，侧边栏快捷列表过滤此类条目），重新导入复用已存元数据跳过信息抓取、yt-dlp 自动从断点续传（完整文件自检跳过；字幕步骤重跑前清旧 cues 防重复）；导入框支持视频链接与**频道链接**（`@handle`/`/channel/`等，`src/lib/youtube.ts` 识别分流）——频道进独立频道页（`src/routes/channel/+page.svelte`）：频道资料 + 最新视频每页 10 条可翻页（`src-tauri/src/channel.rs` 用 `--flat-playlist --playlist-items` 分页拉取），卡片一键走 `import_video` 导入；频道可订阅存 `subscriptions` 表（`channel_id` 为去重键，yt-dlp 缺失时从 channel_url 提取或兜底用频道 URL）；订阅/退订入口在频道页，订阅频道统一显示在媒体库首页导入框下方的卡片区（无独立订阅页/侧边栏入口）
- **媒体播放**：内置 axum HTTP 服务器（127.0.0.1，`/media/`）提供视频流（Range 请求）；不要用 asset 协议播放大视频（有画面无声音的兼容问题）
- **ASR**：`python/asr_worker.py`（MOSS-Transcribe-Diarize）由 Rust 子进程调用；Python 环境在项目 `.venv-asr/`（`scripts/setup_asr.sh` 创建）
- **LLM**：OpenAI 兼容协议（默认 DeepSeek），`llm.rs` 统一入口，配置存 settings 表
- **字幕渲染**：播放器内嵌字幕由 libass-wasm（JavascriptSubtitlesOctopus）渲染 ASS（`src/lib/ass.ts` 负责 cue→ASS 生成，含 CapCut 式预设 `SUBTITLE_PRESETS`）；字幕样式**不进设置页表单**，由播放页右上「Aa」弹出的 `src/lib/SubtitleStylePanel.svelte` 就地调节、实时渲染并防抖持久化；字体文件在 `static/fonts/`，页面 `@font-face` 与 libass 必须用同一份
- **词典**：本地 ECDICT（stardict.db），按需 AI 上下文解析补充；阅读区支持单击查单词、**拖拽框选连续词查短语**（`src/lib/ReadingPanel.svelte`，短语词典未命中时自动 AI 解析，存卡后 `word_cards.word` 存带空格短语、字幕内整体高亮（**短语 emerald / 单词 amber 双色区分**，短语正则不叠加 g 标志且按长度降序防止长短语被抢配）；加入单词本后自动关闭词典面板，高亮即反馈）；查词发音统一走 `src/lib/speech.ts`（有道在线真人发音，失败/超时回退 Rust `speak_word`，即 `say -v` 英文语音离线兜底）；复习页（`src/routes/words/review/`）每题 30s 倒计时（最后 5s 变红+滴答）与答题音效（`src/lib/sfx.ts` Web Audio 合成，开关存 settings `review_sound`）
- **剪辑导出**：播放页头部剪刀按钮 → `src/lib/ExportDialog.svelte`（全片/片段 + 双语/英文/无字幕），片段用 `src/lib/ClipTimeline.svelte` 剪辑条（storyboard 胶片 + I/O 键标记 + 播放头联动）；ffmpeg 子进程导出（`src-tauri/src/export.rs`，`-progress` 解析发 `export-progress` 事件）；字幕烧录分三路：ffmpeg 有 libass → `ass` 滤镜烧 ASS（`buildAss` 同源样式，字体先 `ensure_export_fonts` 同步到 `export_fonts/`）；**无 libass 时**片段走 Canvas 渲染全帧透明 PNG + overlay 时间窗叠层（`src/lib/subtitleRender.ts`，≤200 条事件），全片降级为 mov_text 软字幕
- **章节与时间轴**：整理精校后自动调 `generate_chapters`（LLM 对段落再聚类命名，`chapters` 表）；阅读区 tab 固定为「章节 / 整理精校 / 中英对照」（章节置首，无原字幕 tab），章节标题同时作为分隔行内嵌在精校/对照文本流的章节边界处（仅阅读区展示，不进导出字幕）；每句行尾悬停出现铅笔按钮，可行内**人工修订字幕文本**（`update_sentence` 只改 `text_en`、不动已有译文，重新整理会覆盖修订；内嵌字幕与词高亮随 sentences 自动重建）；播放时间轴带章节刻度 + 悬停预览（缩略图+时间+章节标题，YouTube 式）；storyboard 等距缩略图由 `video_storyboard` 用 `-skip_frame nokey` 快速抽取、按视频缓存在 `app_data/storyboard/`（时间轴悬停与剪辑条共用）

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
- 播放体验：断点续播（位置存 `videos.position_secs`）、视频内嵌字幕走 libass（样式由播放页「Aa」面板控制，ASS 生成逻辑统一在 `src/lib/ass.ts`）、字幕与播放轴双向绑定；空格键 = 播放/暂停、←/→ 方向键 = 后退/快进 5s（控制条第二行有对应按钮，输入控件聚焦时不触发；滑杆拖动后自动 blur 归还焦点）；导出面板打开时 `I`/`O` 键 = 标记片段起点/终点；**沉浸模式下暂停时右侧渐显半透明阅读区**（覆盖视频，`bg-white/80 backdrop-blur`，复用 `src/lib/ReadingPanel.svelte`，普通右侧栏与其必须保持同一数据源）；**播放控制条布局固定为两行**——时间轴滑杆独占第一行（w-full，叠加章节刻度 + 悬停预览浮层），控件第二行（可 flex-wrap，含当前章节名标签），禁止把时间轴和控件挤在同一行
