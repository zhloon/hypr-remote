#!/usr/bin/env bash
set -e

echo "============================================="
echo " HyprRemote Host Environment Setup (Hyprland)"
echo "============================================="

# 1. Configure /dev/uinput permissions for non-root input injection
echo "[1/3] Configuring /dev/uinput permissions..."
sudo usermod -aG input "$USER" 2>/dev/null || true

UDEV_RULE='KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"'
if [ ! -f /etc/udev/rules.d/99-uinput.rules ]; then
    echo "$UDEV_RULE" | sudo tee /etc/udev/rules.d/99-uinput.rules > /dev/null
    sudo udevadm control --reload-rules
    sudo udevadm trigger
    echo "  -> udev rules installed. Note: You may need to log out and back in once."
else
    echo "  -> udev rules already present."
fi

# Ensure uinput kernel module is loaded
sudo modprobe uinput 2>/dev/null || true

# 2. Check distro and install necessary dependencies
echo "[2/3] Checking dependencies..."
if command -v pacman &>/dev/null; then
    echo "  -> Detected Arch Linux / Manjaro"
    echo "  Installing required packages: pipewire, xdg-desktop-portal-hyprland, gst-plugins..."
    sudo pacman -S --needed --noconfirm \
        pipewire wireplumber xdg-desktop-portal-hyprland \
        gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad \
        gst-plugin-pipewire gst-plugins-ugly python python-pip
elif command -v dnf &>/dev/null; then
    echo "  -> Detected Fedora"
    sudo dnf install -y \
        pipewire gstreamer1-plugins-base gstreamer1-plugins-good \
        gstreamer1-plugin-pipewire python3 python3-pip
elif command -v apt-get &>/dev/null; then
    echo "  -> Detected Ubuntu / Debian"
    sudo apt-get update
    sudo apt-get install -y \
        pipewire libgstreamer1.0-dev gstreamer1.0-pipewire \
        gstreamer1.0-plugins-good python3 python3-pip
fi

# 3. Install Python requirements
echo "[3/3] Installing Python WebRTC & evdev dependencies..."
python3 -m pip install --user aiortc websockets evdev 2>/dev/null || pip install aiortc websockets evdev

echo "============================================="
echo " Setup complete! Start the host with:"
echo "   python3 host.py"
echo "============================================="
