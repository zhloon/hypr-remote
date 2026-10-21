#!/usr/bin/env bash
set -e

echo "============================================="
echo "⚡ HyprRemote Linux 一键自动配置与启动"
echo "============================================="

# 1. 自动检测发行版并安装运行依赖
if command -v pacman &>/dev/null; then
    echo "[1/3] 检测到 Arch Linux / EndeavourOS，正在自动补全系统依赖..."
    sudo pacman -S --needed --noconfirm base-devel webkit2gtk-4.1 gtk3 libsoup3 grim ydotool rust || true
elif command -v dnf &>/dev/null; then
    echo "[1/3] 检测到 Fedora，正在自动补全系统依赖..."
    sudo dnf install -y gcc-c++ webkit2gtk4.1-devel gtk3-devel libsoup3-devel grim ydotool rust cargo || true
elif command -v apt-get &>/dev/null; then
    echo "[1/3] 检测到 Ubuntu / Debian，正在自动补全系统依赖..."
    sudo apt-get update -qq
    sudo apt-get install -y build-essential libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev grim ydotool cargo || true
fi

# 2. 自动配置 input 组与虚拟键鼠服务
echo "[2/3] 自动配置 Hyprland 键鼠控制权限与防火墙..."
sudo usermod -aG input "$USER" 2>/dev/null || true
sudo systemctl enable --now ydotool 2>/dev/null || systemctl --user enable --now ydotoold 2>/dev/null || true

# 自动放行局域网控制端口 (TCP 18080 + UDP 8082)
if command -v ufw &>/dev/null; then
    sudo ufw allow 18080/tcp 2>/dev/null || true
    sudo ufw allow 8082/udp 2>/dev/null || true
fi
if command -v firewall-cmd &>/dev/null; then
    sudo firewall-cmd --add-port=18080/tcp --permanent 2>/dev/null || true
    sudo firewall-cmd --add-port=8082/udp --permanent 2>/dev/null || true
    sudo firewall-cmd --reload 2>/dev/null || true
fi

# 3. 检查并加载 Rust 环境
if [ -f "$HOME/.cargo/env" ]; then
    source "$HOME/.cargo/env"
fi

if ! command -v cargo &>/dev/null; then
    echo "正在自动安装 Rust 运行环境..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# 4. 启动客户端
echo "[3/3] 正在启动 HyprRemote..."
cd "$(dirname "$0")/client/src-tauri"
cargo run
