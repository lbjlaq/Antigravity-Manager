# Antigravity Tools / Antigravity Manager 项目规则与持久记忆

## 1. 项目定位与权威文档优先准则 (Documentation-First Mandate)

- **项目定位**：本项目（Antigravity Tools / Antigravity Manager）是基于Tauri v2+Rust+React 19构建的专业级AI账号管理、协议转换与高性能本地AI调度网关系统。
- **权威文档优先阅读规范**：
  - 在开展本项目的任何功能研发、缺陷修复、架构调整、调试测试、配置变动或打包分发任务时，**必须优先阅读根目录下的 [README.md](README.md)**，全面掌握项目的系统全貌、界面导览、多协议转换逻辑、模型映射机制与版本演进脉络。
  - 严禁在未阅读主文档的情况下基于外部惯性或脱离实际上下文臆测业务逻辑。

---

## 2. 技术栈与架构基线

- **技术栈构成**：
  - **前端**：React 19、TypeScript、Vite、Tailwind CSS、Ant Design、DaisyUI、Zustand。
  - **桌面与底层**：Tauri v2、Rust（基于Axum提供高性能API网关、Token管理、OAuth认证与流式转换）。
- **模块与目录边界**：
  - `src-tauri/`：Rust原生核心，包含Axum网关服务、OAuth授权、账号池健康探测、403/429故障转移与协议转换逻辑。
  - `src/`：React前端渲染层，包含仪表盘监控、账号列表、模型路由配置、反代控制与国际化资源。
  - `scripts/`：分发构建、修复补丁与自动化维护脚本。
  - `deploy/` 与 `docker/`：容器化部署与特定发行版部署配置。

---

## 3. 本地构建与打包发布工作流

针对本地开发环境与打包交付，严格遵守以下构建链路标准：

- **前端依赖安装**：
  ```bash
  npm install --legacy-peer-deps
  ```
- **本地桌面应用编译（Tauri）**：
  ```bash
  npm run tauri build
  ```
- **macOS DMG一键打包（集成修复补丁）**：
  - 脚本路径：[scripts/package_dmg.sh](scripts/package_dmg.sh)
  - 规范流程：在执行该脚本前，必须确保已先完成`npm run tauri build`并成功产出`src-tauri/target/release/bundle/macos/Antigravity Tools.app`。该脚本会自动将App产物、修复macOS提示损坏的脚本（`scripts/Fix_Damaged.command`）以及`/Applications`快捷方式软链接打包进DMG镜像。
- **Windows本地构建提示**：
  - 参考根目录下的 [exe_build.txt](exe_build.txt)，必要时配置`$env:CARGO_TARGET_DIR`以优化编译盘符与缓存路径。

---

## 4. 核心业务与协议转换规范

- **多协议适配与中继**：
  - `/v1/chat/completions`（OpenAI格式）
  - `/v1/messages`（Anthropic格式，完整支持Claude Code CLI与思维链透传）
  - Gemini原生SDK直接调用
- **状态自愈与账号轮换**：
  - 涉及429限流与401失效时，必须遵循后端毫秒级静默轮换与重试设计，不得破坏既有故障自愈链路。
  - 涉及403禁止状态时，遵循账号状态自动标记并跳过机制。

---

## 5. 代码与文档协同演进准则

- **就地感知与发现即修**：在阅读、修改或验证代码时，若发现既有行内注释、接口定义或文档（如 [README.md](README.md)、[docs/](docs/) 下的相关技术手册）与最新实现存在偏差，顺手就地同步更新。
- **零破坏原则**：修改模型映射、路由调度或构建脚本时，保持既有平台兼容性，严禁破坏CI流水线（`.github/workflows/release.yml`）与既有打包逻辑。

---

## 6. 自研定制资产绝对保留与上游安全同步准则 (Custom Asset Preservation Mandate)

- **核心原则**：除非官方发布底层断代式重大重构且事先获得明确确认，否则在任何日常维护、功能扩展或上游同步中，**绝对禁止覆盖或回退用户的任何历史自研修改**。
- **神圣保留的自研核心清单**：
  1. **前端监控与统计看板**：[src/pages/Dashboard.tsx](src/pages/Dashboard.tsx)（倒计时、今日使用汇总）与 [src/pages/TokenStats.tsx](src/pages/TokenStats.tsx)（多维度趋势图表）；
  2. **配额关注系统**：[src/config/modelConfig.ts](src/config/modelConfig.ts) 中的虚拟配额组（`gemini-5h`、`gemini-weekly`、`3p-5h`、`3p-weekly`）及 [src/components/accounts/AccountCard.tsx](src/components/accounts/AccountCard.tsx) 中的动态注入提取逻辑，严禁引入强制劫持显示单一模型的代码；
  3. **纯净导航体验**：[src/components/navbar/Navbar.tsx](src/components/navbar/Navbar.tsx) 严禁强塞商业推广路由（如 `/apikey-fun` 中转站）；
  4. **自研错峰与保护调度**：相控阵错峰调度系统（[PhaseScheduler.tsx](src/components/settings/PhaseScheduler.tsx)）及反重力IDE活跃主号智能探测与API保底避让机制；
  5. **模型映射与协议修复**：`src-tauri/src/proxy/common/model_mapping.rs` 中全套自研的Gemini 3.8/3.7/3.6 Flash细粒度映射与流式终端状态修复。
- **增量合入规范**：同步上游主线时，必须始终以当前自研分支为主干基准，采取防御性单点补丁与三方合并方式合入后端底层修复，严禁使用 `git reset --hard` 或全量覆写前端目录。

