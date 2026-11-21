# Antigravity Manager (账号管理工具)

> 🚀 专为 Antigravity 设计的高性能、轻量级 Google AI 账号管理与配额监控工具，支持桌面 GUI、桌面 Mini 悬浮窗以及终端 CLI（`am`）。

[English](README.md) | [简体中文](README_ZH.md)

---

## ✨ 核心特性

1. **一键添加与 OAuth 登录**：内置本地 Callback 监听与手动 Code 输入备选方案，快速完成 Google OAuth 授权绑定。
2. **账号热切换**：无感切换当前激活账号，支持经典款与 IDE 宿主客户端无缝适配。
3. **配额与模型监控**：实时拉取与轮询账号额度、模型分组使用率、额度条直观展示与自动刷新。
4. **账号池与标签管理**：支持批量导入/导出、账号标签、优先级排序、异常账号检测。
5. **设备指纹隔离**：为每个账号独立分配和持久化设备特征指纹，规避多账号关联风控。
6. **宿主数据共用**：与 Antigravity 主分支共享 `accounts.json` 及数据库，两边数据实时同步，双向免冲突。
7. **Mini 悬浮窗**：置顶小巧悬浮窗口，实时展示当前账号剩余额度，支持一键切换。
8. **终端 CLI 支持 (`am`)**：原生提供 `am` 命令行程序，支持无头环境（CLI）下的列出、查询、切换与导出。

---

## 🛠 命令行使用 (`am`)

编译生成或安装后，可以直接在终端使用 `am` 命令：

```bash
# 查看所有账号列表与额度
am list

# 查看当前使用的账号
am current

# 切换到指定账号
am switch <account_id>

# 导出账号数据
am export -o accounts_backup.json

# 查看帮助说明
am --help
```

---

## 📁 模块架构

```
src/
├── components/
│   ├── accounts/          # 账号列表、卡片、切换与添加交互
│   ├── dashboard/         # 配额概览与多环仪表盘
│   ├── layout/            # 页面骨架与 Mini 悬浮窗
│   ├── navbar/            # 导航栏
│   └── settings/          # 自动热切与系统配置
├── services/              # 账号和配置服务
├── stores/                # Zustand 状态管理
└── utils/                 # IPC 封装与通用工具

src-tauri/
├── src/
│   ├── bin/am.rs          # am 命令行别名程序
│   ├── commands/          # Tauri IPC 命令处理层
│   ├── models/            # 账号、额度与配置实体模型
│   └── modules/           # 账号持久化、OAuth、指纹生成、宿主数据同步
```

---

## 📄 开源许可

MIT License.
