# JeikCode 接入 Antigravity-Manager 指南

[JeikCode](https://github.com/jeikl/JeikCode) 是由本项目核心维护者打造的现代终端 AI Coding Agent 工具（基于原生 Rust 内核），深度兼容 Antigravity-Manager 网关协议。通过两者的深度协同，可实现 **95%+ 的超高 KV-Cache 缓存命中率**、全协议多模态交互与极致的推理流式体验。

---

## 🚀 方式一：GUI 界面一键同步（最推荐）

如果您正在运行 Antigravity-Manager 桌面客户端或 Web 管理后台，可通过自带的一键同步功能**零配置秒级接入**：

1. **启动服务**：
   - 打开 Antigravity-Manager，在 **API 反代** 页面确保代理服务已启动（右上角显示绿色运行状态）。
2. **进入一键配置**：
   - 点击顶部导航标签页 **`>_ Agent工具一键配置`**。
3. **一键同步 JeikCode**：
   - 在第一个卡片 **JeikCode**（标注有 `🌟 推荐使用 · Best Matched`）中：
     - 系统已自动侦测本地安装的 JeikCode 路径及当前运行的网关地址（如 `http://127.0.0.1:8045/v1` 或自定义端口）。
     - 在 **「选择同步模型」** 下拉列表中选择您希望默认使用的模型（例如 `gemini-3.8-flash-high` 或 `claude-sonnet-4-6-thinking`）。
     - 点击 **`🔄 立即同步配置`**。
4. **启动与使用 JeikCode**：
   - 打开终端直接运行 `jeikcode`，即可启动原生 TUI 交互界面。
   - **🌐 强烈推荐体验 WebUI**：在终端 TUI 中输入 **`/webui`**，即可一键在浏览器打开精美的网页端，享受更加直观舒适的可视化多轮编程与 Diff 体验，开箱即用！

> [!TIP]
> **自动配置内容说明**：
> 点击“立即同步”后，系统会自动更新 `~/.jeikcode/config.toml`，自动注入 `antigravity-manager` 提供商账号、预先配置好网关核心模型列表（包含上下文窗口、思考档位、图片支持等优化参数），并生成备份配置，支持随时一键还原（点击卡片左下角的还原图标）。

---

## 🛠️ 方式二：手动修改配置文件（Headless / Linux / Docker 推荐）

对于无桌面环境、远程服务器、Docker 部署或希望手动微调配置的用户，可直接编辑 JeikCode 配置文件：

- **配置文件路径**：
  - Linux / macOS：`~/.jeikcode/config.toml`
  - Windows：`C:\Users\<你的用户名>\.jeikcode\config.toml`
  - （或由环境变量 `JEIKCODE_HOME` 指定的目录下的 `config.toml`）

在 `config.toml` 中添加或替换以下内容（将 `http://127.0.0.1:8045` 和 `sk-your-api-key` 替换为你的真实网关地址与 API Key）：

```toml
# 默认使用的模型
default_model = "gemini-3.8-flash-high"
default_provider = "gemini-3.8-flash-high"

# ─── 提供商账号定义 ───
[provider_accounts.antigravity-manager]
provider = "anthropic"               # 采用网关深度优化的 Anthropic 协议接入
base_url = "http://127.0.0.1:8045/v1" # Antigravity-Manager 监听端口及路径
api_key = "sk-your-api-key"          # 网关配置的 API Key

# ─── 模型定义 (可按需添加多个) ───

# Gemini 3.8 Flash (高思考档位，推荐日常主力)
[models."gemini-3.8-flash-high"]
account = "antigravity-manager"
model = "gemini-3.8-flash-high"
context_window = 1048576
image_input = true
reasoning_model = true
reasoning_history = "exclude"
reasoning_effort = "high"
reasoning_levels = ["low", "medium", "high"]

# Claude 3.7 Sonnet Thinking (深度逻辑与重构利器)
[models."claude-sonnet-4-6-thinking"]
account = "antigravity-manager"
model = "claude-sonnet-4-6-thinking"
context_window = 200000
image_input = true
reasoning_model = true
reasoning_history = "exclude"
reasoning_effort = "high"
reasoning_levels = ["low", "medium", "high"]

# Claude 3.7 Opus Thinking (超高复杂度架构规划)
[models."claude-opus-4-6-thinking"]
account = "antigravity-manager"
model = "claude-opus-4-6-thinking"
context_window = 200000
image_input = true
reasoning_model = true
reasoning_history = "exclude"
reasoning_effort = "high"
reasoning_levels = ["low", "medium", "high"]
```

---

## ⚡ 方式三：终端环境变量临时启动

如果您想快速临时测试或在脚本中调用，可通过环境变量直接覆盖启动：

```bash
# 设置网关地址与鉴权 Key（Anthropic 兼容协议）
export ANTHROPIC_BASE_URL="http://127.0.0.1:8045"
export ANTHROPIC_API_KEY="sk-your-api-key"

# 启动并指定模型
jeikcode --model claude-sonnet-4-6-thinking
```

---

## 🌟 核心特性与深度调优

### 1. 深度协同：95%+ KV-Cache 缓存命中
Antigravity-Manager 具备**前缀稳定性保证（Prefix Stability）**与**思考历史净化（Dynamic Thinking Stripping）**机制，配合 JeikCode 原生 Rust 上下文管理与只读工具并发调用，能将大型项目的代码提示词缓存命中率提升至 **95% 以上**，大幅缩短首字响应延迟（TTFT）并降低 Token 消耗。

### 2. 思考强度调节 (`/effort` & `Ctrl+T`)
在 JeikCode 交互式界面（TUI）中：
- 输入 `/effort` 或直接按下快捷键 **`Ctrl + T`**，即可在 `low`、`medium`、`high` 各个思考档位之间快速切换。
- Antigravity-Manager 网关会自动将请求体中的思考预算（Thinking Budget / Thinking Level）规范化后派发至对应的上游模型。

### 3. 多模型与提供商交互式切换
- 输入 **`/model`**：在当前网关下快速切换不同模型（如从 `gemini-3.8-flash-high` 秒切到 `claude-opus-4-6-thinking`）。
- 输入 **`/provider`**：查看当前网关的连接详情与上下文窗口状态。

### 4. 🌐 强烈推荐：使用 JeikCode WebUI 获得最佳编程体验
JeikCode 除了极速响应的原生 Rust 终端 TUI 外，还内置了极具科技感的现代化 WebUI：
- **一键开启**：终端直接运行 `jeikcode` 启动 TUI 后，在输入框键入 **`/webui`**，系统将自动在默认浏览器中打开可视化界面。
- **全景可视化**：相比纯字符终端，WebUI 提供更丰富直观的代码 Diff 高亮对比、文件树展开、多轮对话折叠与 Markdown 渲染，大幅提升复杂项目代码阅读与审查的舒适度与工作效率！

---

## ❓ 常见问题排查 (FAQ)

### Q1: 提示 `Connection Refused` 或无法连接？
- 检查 Antigravity-Manager 客户端中 API 反代服务是否处于“运行中”状态。
- 确认端口号是否匹配：客户端默认常用端口为 `8045` 或 `8046`，请以客户端界面或 Docker 映射端口为准。
- 若在 Docker / WSL 中运行 JeikCode，需将 `127.0.0.1` 替换为主机 LAN IP 或 `host.docker.internal`。

### Q2: 提示 `401 Unauthorized`？
- 确认 `api_key` 与 Antigravity-Manager 的 **API 反代** 页面中设置的 `API_KEY` 完全一致。
- 注意：如果您启用了独立管理后台登录密码 `WEB_PASSWORD`，API 请求仍需使用 `API_KEY` 进行鉴权。

### Q3: 如何自定义添加其他实验性模型？
- 在 `config.toml` 的 `[models."<你的别名>"]` 下添加新的模型块，将 `account` 指向 `"antigravity-manager"`，`model` 填写上游支持的模型名（例如 `gemini-3-flash`、`gemini-2.5-pro`），保存后即刻生效，无需重启 JeikCode 守护进程。
