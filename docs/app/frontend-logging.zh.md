# 前端日志（UI 排障）

应用 UI 由前端渲染，但大部分真实数据（账号、配置、代理控制）来自桌面端后端。

如果 UI 出现白屏/空白页，或按钮无法响应，请在反馈问题前先收集以下信息：

## 1）确认你打开的是桌面窗口（不是浏览器 Dev Server）
- 浏览器访问 `http://localhost:1420/` 看到的是 Vite dev server 页面，通常无法加载真实后端数据。
- 请使用 `npm run tauri dev` 启动后弹出的桌面窗口（或安装包应用）进行验证。

## 2）打开桌面端开发者工具
- 在桌面窗口中打开 Developer Tools，查看：
  - **Console** 是否有运行时错误
  - **Network** 是否有失败的 `invoke` 调用

如果看到错误，请复制：
- 完整错误信息
- 对应调用名（例如 `save_config`、`start_proxy`、`get_proxy_runtime_status`）

## 3）查看后端日志
后端日志由 Rust/Tauri 进程输出。

从源码运行时：
- 在终端运行 `npm run tauri dev`，保存终端输出。

使用安装包时：
- 查看系统标准的应用日志位置（不同平台路径不同）。

## 4）隐私说明
- 不要分享你的 `gui_config.json`、`accounts/*.json` 或任何 API key。
- 分享日志时请脱敏 token 与你认为敏感的本地路径。

