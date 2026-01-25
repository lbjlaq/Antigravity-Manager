FROM debian:trixie-slim

LABEL maintainer="lbjlaq"

ENV DEBIAN_FRONTEND=noninteractive

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ARG USERNAME=antigravity
ARG USER_UID=1000
ARG USER_GID=1000

ARG USE_CHINA_MIRROR=false
RUN if [ "$USE_CHINA_MIRROR" = "true" ]; then \
    sed -i 's/deb.debian.org/mirrors.tuna.tsinghua.edu.cn/g' /etc/apt/sources.list.d/debian.sources 2>/dev/null || true && \
    sed -i 's/security.debian.org/mirrors.tuna.tsinghua.edu.cn\/debian-security/g' /etc/apt/sources.list.d/debian.sources 2>/dev/null || true && \
    sed -i 's/deb.debian.org/mirrors.tuna.tsinghua.edu.cn/g' /etc/apt/sources.list 2>/dev/null || true && \
    sed -i 's/security.debian.org/mirrors.tuna.tsinghua.edu.cn\/debian-security/g' /etc/apt/sources.list 2>/dev/null || true; \
    fi

RUN apt-get update && apt-get install -y --no-install-recommends \
    openbox \
    tigervnc-standalone-server \
    tigervnc-common \
    tigervnc-tools \
    novnc \
    websockify \
    wget \
    ca-certificates \
    dbus-x11 \
    fonts-noto-cjk \
    locales \
    firefox-esr \
    procps \
    sudo \
    xdg-utils \
    python3-xdg \
    libgl1-mesa-dri \
    pciutils \
    x11-xserver-utils \
    && echo "en_US.UTF-8 UTF-8" > /etc/locale.gen \
    && locale-gen \
    && groupadd --gid $USER_GID $USERNAME \
    && useradd --uid $USER_UID --gid $USER_GID -m $USERNAME \
    && echo "$USERNAME ALL=(ALL) NOPASSWD:/usr/bin/apt-get,/usr/bin/dpkg,/bin/rm" > /etc/sudoers.d/$USERNAME \
    && chmod 0440 /etc/sudoers.d/$USERNAME \
    && mkdir -p /home/$USERNAME/.vnc /home/$USERNAME/.mozilla /home/$USERNAME/.antigravity_tools /home/$USERNAME/.config/Antigravity/User/globalStorage \
    && echo "{}" > /home/$USERNAME/.config/Antigravity/User/globalStorage/storage.json \
    && chown -R $USERNAME:$USERNAME /home/$USERNAME \
    && apt-get autoremove -y \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /usr/share/doc/* \
    && rm -rf /usr/share/man/* \
    && rm -rf /usr/share/info/* \
    && find /usr/share/locale -mindepth 1 -maxdepth 1 ! -name 'en*' -exec rm -rf {} + \
    && rm -rf /var/cache/* \
    && rm -rf /tmp/* \
    && rm -rf /root/.cache/*

EXPOSE 6080 8045

ENV DISPLAY=:1 \
    HOME=/home/antigravity \
    USER=antigravity \
    LANG=en_US.UTF-8 \
    LC_ALL=en_US.UTF-8 \
    BROWSER=/usr/bin/firefox-esr \
    MOZ_DISABLE_CONTENT_SANDBOX=1

COPY --chown=antigravity:antigravity start.sh /home/antigravity/start.sh
RUN chmod +x /home/antigravity/start.sh

USER antigravity
WORKDIR /home/antigravity

HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD pgrep -x Xtigervnc > /dev/null && pgrep -f websockify > /dev/null && pgrep -f antigravity_tools > /dev/null || exit 1

CMD ["/home/antigravity/start.sh"]
