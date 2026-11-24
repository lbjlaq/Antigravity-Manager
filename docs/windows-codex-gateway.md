# Windows Codex 连接本机 Antigravity 网关

本文记录 Windows 上 Codex（CLI/桌面）调用 `http://127.0.0.1:8045/v1` 失败时的
定位过程与可复用修复方式。适用于 Codex 0.171.0 + 原生 Antigravity Tools +
WSL/Docker 网关共存的环境。

## 症状

- `codex exec` 重试 5 次后报 `unexpected status 502 Bad Gateway: Unknown error`。
- 或网关返回 `403 Invalid token. Please check your API key.`。
- `curl.exe http://127.0.0.1:8045/health` 正常，浏览器打开管理页也正常，只有 Codex 失败。

## 根因

### 1. Codex 不应用 Windows 代理例外（502 的来源）

Windows 系统代理（Clash 等）开启在 `127.0.0.1:7890` 时，Codex 的 HTTP 客户端会
读取系统代理。`Internet 设置` 中虽然写了：

```text
ProxyOverride = localhost;127.*;192.168.*;...
```

但 Codex 0.171.0 **不会**应用 `ProxyOverride` 里的 `127.*` 例外，于是
`127.0.0.1:8045` 的请求被送到 7890，代理无法回源，表现为 502。

必须用环境变量显式绕过：

```powershell
[Environment]::SetEnvironmentVariable('NO_PROXY', '127.0.0.1,localhost,::1', 'User')
```

`NO_PROXY`（以及 `no_proxy`）对 Codex 生效；仅设置 `ProxyOverride` 不够。

### 2. Codex 与网关的密钥不一致（403 的来源）

直连原生网关时，以下三处必须是同一个值：

| 位置 | 字段 |
| --- | --- |
| `%USERPROFILE%\.codex\config.toml` | `[model_providers.<provider>] experimental_bearer_token` |
| `%USERPROFILE%\.codex\auth.json` | `OPENAI_API_KEY` |
| `%USERPROFILE%\.antigravity_tools\gui_config.json` | `proxy.api_key` |

只改其中一处会出现“网络已通但 403”。`auth.json` 的 `OPENAI_BASE_URL`
也应为 `http://127.0.0.1:8045/v1`。

### 3. Windows 8045 双监听（重启后随机失败）

Docker 网关通过 WSL 的 `wslrelay.exe` 把容器端口发布到 Windows，原生
`antigravity-tools.exe` 也会监听 `127.0.0.1:8045`。两个进程都能 bind 成功，
但新连接落到谁没有严格保证，且 Windows 重启、WSL 重启、容器重建后可能交换。

判断方法要看 **Established 连接的 OwningProcess**，只看 `LISTEN` 不够：

```powershell
Get-NetTCPConnection -LocalPort 8045 -State Listen |
  ForEach-Object { Get-Process -Id $_.OwningProcess }
Get-NetTCPConnection -LocalPort 8045 -State Established |
  ForEach-Object { Get-Process -Id $_.OwningProcess }
```

本机采用的端口规划：

- `127.0.0.1:8045`：原生 Antigravity Tools，Codex 直连；`API_KEY` 为原生
  `gui_config.json` 中的 `proxy.api_key`。
- `127.0.0.1:18046`：WSL/Docker 网关（`docker/.env` 中 `PORT=18046`），
  使用 Docker 自己的 `API_KEY`，两者互不复用端口。

两套 key 互不相通：用原生 key 调 18046 会 403，用 Docker key 调 8045 也会 403。

## 诊断与修复

仓库自带只读诊断脚本，默认不写任何文件：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Repair-CodexGateway.ps1
```

检查项：

- 三处 token 的指纹（长度 + SHA-256 前 12 位，不输出明文）；
- `config.toml` / `auth.json` 的 base URL；
- 当前用户 `NO_PROXY` 与系统代理状态；
- 8045 的监听者与 Established 连接归属；
- `/health`、匿名 `/v1/models`（应为 401）、带 token 的 `/v1/models`（应为 200）。

确认后执行修复（会先备份 `config.toml` 与 `auth.json`）：

```powershell
# 用原生网关的 key 覆盖 Codex 两处 token
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Repair-CodexGateway.ps1 -Apply -UseNativeGateway

# 只同步 base URL / NO_PROXY，保留 Codex 当前 token
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\windows\Repair-CodexGateway.ps1 -Apply
```

脚本写入时保留原文件的 BOM/行尾与注释，只替换目标 section 的键值；
离线自检可运行 `-SelfTest`。

修复后新开终端（让 `NO_PROXY` 生效）再验证：

```powershell
codex exec --skip-git-repo-check --sandbox read-only -c 'model_reasoning_effort="low"' 'Reply with exactly: PONG'
```

## 2026-11-24 验证记录

- 不设置 `NO_PROXY`：Codex 对 `http://127.0.0.1:8045/v1/responses` 连续重试后
  返回 `502 Bad Gateway`，确认请求被系统代理截走。
- 设置 `NO_PROXY=127.0.0.1,localhost,::1`：同一命令返回 `PONG`，退出码 0。
- `Get-NetTCPConnection` 显示 8045 仅由 `antigravity-tools` 监听，
  Docker 网关映射为 `0.0.0.0:18046->8045`。
- 诊断脚本对真实配置输出：token 三处一致、`NO_PROXY` 通过、
  `/health` 200、匿名 `/v1/models` 401、带 token 200。

## 注意

- 修改 `NO_PROXY` 后，已经在运行的 Codex/终端不会自动继承，需要重启进程。
- PowerShell 5.1 读取无 BOM 的 UTF-8 文件时默认按 ANSI 解码；
  脚本已显式使用 `-Encoding UTF8`，否则 `config.toml` 中的中文路径会乱码。
- 若改用 Docker 网关，请同时把 `config.toml`、`auth.json` 的 base URL 指向
  `http://127.0.0.1:18046/v1`，并换成 Docker 的 `API_KEY`。
