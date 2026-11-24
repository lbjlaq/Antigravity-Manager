# 本机 Antigravity 反代（WSL / Docker）

`docker-compose.lan.yml` 使用官方 `v5.0.3` 镜像，不在本地编译 Rust。
宿主机端口由 `PORT` 控制，默认 `8045`。本机与 Windows 原生网关共存时改为
`PORT=18046`，由原生网关独占 `127.0.0.1:8045`，详见
[`docs/windows-codex-gateway.md`](../docs/windows-codex-gateway.md)。

## 本机入口与凭据

- 管理页面：`http://localhost:8045`，使用 `WEB_PASSWORD` 登录。
- OpenAI 兼容 Base URL：`http://localhost:8045/v1`，调用密钥使用 `API_KEY`。
- Anthropic 兼容 Base URL：`http://localhost:8045`，调用密钥同上。
- 运行凭据：项目下的 `docker/.env`，权限 `600`，已被 Git 忽略。
- 持久化账号和配置：`~/.antigravity_tools`；可用 `ABV_HOST_DATA_DIR` 更改。
- `0.0.0.0` 是监听地址，客户端连接时使用 `localhost` 或实际主机 IP。

WSL 的 IP 可用 `hostname -I` 查看。在 WSL2 NAT 模式下，Windows 本机可用
`localhost:8045`；其他局域网设备访问 Windows IP 还取决于 Windows 端的转发和防火墙配置。

## 首次部署

在仓库根目录执行；若 `docker/.env` 已存在，应保留已有密钥。

```bash
# 仅在尚未创建 docker/.env 时复制模板。
cp -n docker/lan.env.example docker/.env
chmod 600 docker/.env
# 分别运行两次 openssl rand -hex 32，并将结果填入 API_KEY 和 WEB_PASSWORD。
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml config --quiet
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml up -d
```

`HTTP_PROXY` / `HTTPS_PROXY` 控制容器的出站连接，与网关监听地址无关。
需要时在本地 `.env` 中配置，并在管理页面的「API 反代 → 上游代理」保存同一地址。
容器中 `127.0.0.1` 指向容器本身；访问宿主机代理可使用
`host.docker.internal`（Compose 已配置 host-gateway），或实际可达的宿主机 IP。

首次启动后在管理页面导入账号，刷新账号配额，再启用「API 反代」和自动启动。
健康接口正常仅表示管理 HTTP 服务已运行，不能代替这个启用步骤。

本次 WSL 部署已通过管理器自带数据库导入接口接入 Windows Antigravity 当前登录。
导入源只读，临时 OAuth 数据库副本已在成功导入后删除；Windows 原始登录数据未改动。

## 启停与验证

```bash
# 本机已缓存镜像，重建时可显式禁止拉取与编译。
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml up -d --pull never --no-build
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml ps
curl --noproxy '*' http://127.0.0.1:8045/health
ss -ltn 'sport = :8045'

# 验证鉴权，并要求真实模型生成 ANTIGRAVITY_PROXY_OK。
python3 scripts/smoke-local-proxy.py
python3 scripts/smoke-local-proxy.py --stream

# 重启后复测持久化。
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml restart
python3 scripts/smoke-local-proxy.py --stream

# 停止服务，保留账号和配置。
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml stop
```

检查脚本使用 Python 3 标准库，从本地文件读取 API Key，不输出密钥。
可用 `--model` 选择账号实际支持的模型，用 `--base-url` 测试其他可达网卡地址。
流式检查还要求服务正常发出 `[DONE]`，避免把提前断开的流误判为成功。

## Windows 原生网关共存（端口规划）

同一台 Windows 上可能同时存在两套网关：

- 原生 Antigravity Tools：监听 `127.0.0.1:8045`，密钥在
  `%USERPROFILE%\.antigravity_tools\gui_config.json` 的 `proxy.api_key`。
- WSL/Docker 网关：经 `wslrelay.exe` 发布到 Windows，密钥在 `docker/.env`。

两者都绑定 8045 时，Windows 允许共享绑定，但新连接落到哪个进程没有严格保证，
重启后可能交换，表现为 Codex 间歇性 401/403 或 502。因此本机把 Docker 映射
改为 `18046`：

```bash
sed -i 's/^PORT=8045$/PORT=18046/' docker/.env
docker compose --env-file docker/.env -f docker/docker-compose.lan.yml up -d
docker port antigravity-manager   # 0.0.0.0:18046->8045/tcp
```

Codex 继续使用 `http://127.0.0.1:8045/v1`（原生网关）。若改指 Docker，
需同时替换 base URL 与 `API_KEY`，两套密钥不通用。

Codex 读取 Windows 系统代理但忽略 `ProxyOverride` 中的 `127.*`，必须为当前用户
持久化 `NO_PROXY=127.0.0.1,localhost,::1`；诊断与修复脚本见
`scripts/windows/Repair-CodexGateway.ps1`。

## 故障定位

- `503 Proxy service is currently disabled`：在管理页面启动 API 反代。
- 已导入账号但提示 `no quota accounts`：刷新配额后，重新启用反代以重载账号池。
- Google 返回 `UNSUPPORTED_LOCATION`：先比较官方客户端和管理器的出站代理、
  账号项目与客户端配置；保留 Google 的原始错误原因，不能仅凭管理器泛化提示判为封号。
- Docker daemon 访问镜像源超时：本机镜像已通过使用当前 shell 代理的 registry 客户端
  拉取并经 `docker load` 载入，可继续使用 `--pull never` 启动。

上述部署说明只描述本机环境，不改变上游默认端口与默认行为。

## 本次验收记录（2026-11-20）

- Docker 容器 `antigravity-manager` 为 `running / healthy`，重启策略为 `unless-stopped`。
- Docker 发布端口及 WSL 监听均为 `0.0.0.0:8045`。
- Windows 的 `localhost:8045` 及到 WSL 非回环 IP 的健康检查均成功。
- 已导入 1 个本地账号；容器重启后账号、出站代理和反代自动启动配置仍有效。
- 无 API Key 的模型列表请求返回 `401`；API Key 访问管理账号接口也返回 `401`。
- 真实 OpenAI 兼容普通请求返回 `ANTIGRAVITY_PROXY_OK`。
- 重启后，WSL 非回环地址的流式请求和 Windows `curl.exe` 的普通请求均通过；
  流式请求包含正常结束标记 `[DONE]`。
- 排查时的完整报文记录已恢复为默认精简记录，密钥未写入 Git。
