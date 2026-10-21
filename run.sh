#!/usr/bin/env bash

echo "============================================="
echo "⚡ HyprRemote Linux 一键自动配置与启动"
echo "============================================="

# 1. 自动检测发行版并安装运行依赖 (含 pkg-config, webkit2gtk-4.1 及 GPU 硬件编解码驱动)
if command -v pacman >/dev/null 2>&1; then
    echo "[1/3] 检测到 Arch Linux / EndeavourOS，正在检查系统依赖与 GPU 硬件编解码支持..."
    sudo pacman -S --needed --noconfirm base-devel pkgconf webkit2gtk-4.1 gtk3 libsoup3 grim ydotool wl-clipboard rust ffmpeg wf-recorder libva-mesa-driver intel-media-driver 2>/dev/null || true
elif command -v dnf >/dev/null 2>&1; then
    echo "[1/3] 检测到 Fedora，正在检查系统依赖与 GPU 硬件编解码支持..."
    sudo dnf install -y gcc-c++ pkgconf-pkg-config webkit2gtk4.1-devel gtk3-devel libsoup3-devel grim ydotool wl-clipboard rust cargo ffmpeg wf-recorder mesa-va-drivers libva-intel-media-driver 2>/dev/null || true
elif command -v apt-get >/dev/null 2>&1; then
    echo "[1/3] 检测到 Ubuntu / Debian，正在检查系统依赖与 GPU 硬件编解码支持..."
    sudo apt-get update -qq 2>/dev/null || true
    sudo apt-get install -y build-essential pkg-config libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev grim ydotool wl-clipboard cargo ffmpeg wf-recorder mesa-va-drivers intel-media-va-driver 2>/dev/null || true
fi

# 2. 自动配置 input 组与虚拟键鼠服务
echo "[2/3] 检查 Hyprland 键鼠控制权限与虚拟设备服务..."
sudo usermod -aG input "$USER" 2>/dev/null || true
sudo chmod 666 /dev/uinput 2>/dev/null || true

# 配置 udev 规则保证永久具备 uinput 读写权限
if [ ! -f /etc/udev/rules.d/99-uinput.rules ]; then
    echo 'KERNEL=="uinput", GROUP="input", MODE="0666"' | sudo tee /etc/udev/rules.d/99-uinput.rules >/dev/null 2>&1 || true
    sudo udevadm control --reload-rules 2>/dev/null || true
    sudo udevadm trigger 2>/dev/null || true
fi

# 确保 ydotoold 虚拟键鼠守护服务正在运行
if ! pgrep -x ydotoold >/dev/null 2>&1; then
    echo "⚡ 正在启动 ydotoold 虚拟键鼠后台服务..."
    sudo systemctl enable --now ydotool 2>/dev/null || systemctl --user enable --now ydotoold 2>/dev/null || true
    sleep 0.3
    if ! pgrep -x ydotoold >/dev/null 2>&1; then
        sudo ydotoold --socket-path=/tmp/.ydotool_socket --socket-perm=0666 >/dev/null 2>&1 &
        sleep 0.5
    fi
fi
export YDOTOOL_SOCKET="/tmp/.ydotool_socket"

# 自动放行局域网控制端口 (TCP 18080 + UDP 8082)
if command -v ufw >/dev/null 2>&1; then
    sudo ufw allow 18080/tcp 2>/dev/null || true
    sudo ufw allow 8082/udp 2>/dev/null || true
fi
if command -v firewall-cmd >/dev/null 2>&1; then
    sudo firewall-cmd --add-port=18080/tcp --permanent 2>/dev/null || true
    sudo firewall-cmd --add-port=8082/udp --permanent 2>/dev/null || true
    sudo firewall-cmd --reload 2>/dev/null || true
fi

# 3. 检查并加载 Rust 环境
if [ -f "$HOME/.cargo/env" ]; then
    . "$HOME/.cargo/env"
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "正在自动安装 Rust 编译环境..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    if [ -f "$HOME/.cargo/env" ]; then
        . "$HOME/.cargo/env"
    fi
fi

# 4. 启动客户端
echo "[3/3] 正在检查端口并启动 HyprRemote..."
fuser -k 18080/tcp 2>/dev/null || true
fuser -k 8082/udp 2>/dev/null || true
pkill -f hypr-remote-client 2>/dev/null || true

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR/client/src-tauri"

export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export GDK_BACKEND="wayland,x11"
export RUST_BACKTRACE=1

if ! cargo run; then
    echo ""
    echo "============================================="
    echo "❌ 启动失败，请将上方终端错误日志复制反馈。"
    echo "============================================="
    exit 1
fi
