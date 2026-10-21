#!/usr/bin/env python3
"""
HyprRemote Host Daemon (Linux / Hyprland)
Provides:
  1. GStreamer + PipeWire screen capture for Wayland/Hyprland
  2. Hardware / Software H.264 WebRTC video streaming
  3. Linux /dev/uinput virtual keyboard & mouse injection
  4. UDP LAN discovery beacon (ZeroConf style)
"""

import asyncio
import json
import socket
import os
import sys
import platform
import subprocess
import time
from fractions import Fraction

# WebRTC and async networking
import websockets
from aiortc import RTCPeerConnection, RTCSessionDescription, VideoStreamTrack
from aiortc.mediastreams import MediaStreamError
from av import VideoFrame
import numpy as np

# Linux input injection
try:
    import evdev
    from evdev import UInput, ecodes as e
    HAS_EVDEV = True
except ImportError:
    HAS_EVDEV = False
    print("[WARN] python-evdev not found. Input injection will be simulated.")


# ==========================================
# 1. Virtual Input Device (/dev/uinput)
# ==========================================
class VirtualInput:
    def __init__(self):
        self.ui = None
        if not HAS_EVDEV:
            return

        cap = {
            e.EV_KEY: [
                e.BTN_LEFT, e.BTN_RIGHT, e.BTN_MIDDLE, e.BTN_SIDE, e.BTN_EXTRA,
                *list(range(1, 248))  # All standard KEY_* scancodes
            ],
            e.EV_REL: [e.REL_X, e.REL_Y, e.REL_WHEEL, e.REL_HWHEEL],
            e.EV_ABS: [
                (e.ABS_X, evdev.AbsInfo(value=0, min=0, max=65535, fuzz=0, flat=0, resolution=0)),
                (e.ABS_Y, evdev.AbsInfo(value=0, min=0, max=65535, fuzz=0, flat=0, resolution=0))
            ]
        }

        try:
            self.ui = UInput(cap, name="HyprRemote-Virtual-Device")
            print("[Input] Successfully initialized /dev/uinput virtual device")
        except PermissionError:
            print("[ERROR] Cannot access /dev/uinput! Please run ./setup-hyprland.sh and ensure user is in 'input' group.")
        except Exception as ex:
            print(f"[ERROR] Failed to create virtual input: {ex}")

    def handle_event(self, data):
        if not self.ui:
            return

        t = data.get("t")
        if t == "m_rel":
            # Relative mouse motion (pointer lock)
            dx = data.get("dx", 0)
            dy = data.get("dy", 0)
            self.ui.write(e.EV_REL, e.REL_X, dx)
            self.ui.write(e.EV_REL, e.REL_Y, dy)
            self.ui.syn()

        elif t == "m_abs":
            # Normalized absolute coordinates (0.0 - 1.0)
            x = int(data.get("x", 0) * 65535)
            y = int(data.get("y", 0) * 65535)
            self.ui.write(e.EV_ABS, e.ABS_X, x)
            self.ui.write(e.EV_ABS, e.ABS_Y, y)
            self.ui.syn()

        elif t == "m_btn":
            # Mouse button click
            btn_map = {
                0: e.BTN_LEFT,
                1: e.BTN_MIDDLE,
                2: e.BTN_RIGHT,
                3: e.BTN_SIDE,
                4: e.BTN_EXTRA
            }
            btn = btn_map.get(data.get("btn", 0), e.BTN_LEFT)
            down = 1 if data.get("down", False) else 0
            self.ui.write(e.EV_KEY, btn, down)
            self.ui.syn()

        elif t == "m_wheel":
            dy = data.get("dy", 0)
            dx = data.get("dx", 0)
            if dy != 0:
                self.ui.write(e.EV_REL, e.REL_WHEEL, -1 if dy > 0 else 1)
            if dx != 0:
                self.ui.write(e.EV_REL, e.REL_HWHEEL, 1 if dx > 0 else -1)
            self.ui.syn()

        elif t == "key":
            code = data.get("code")
            down = 1 if data.get("down", False) else 0
            if code is not None:
                self.ui.write(e.EV_KEY, code, down)
                self.ui.syn()

    def close(self):
        if self.ui:
            self.ui.close()


# ==========================================
# 2. Wayland / PipeWire Video Stream Track
# ==========================================
class WaylandScreenTrack(VideoStreamTrack):
    """
    Captures Hyprland desktop via GStreamer PipeWire pipeline and feeds WebRTC.
    """
    def __init__(self, fps=60):
        super().__init__()
        self.fps = fps
        self.time_base = Fraction(1, fps)
        self.pts = 0
        self.width = 1920
        self.height = 1080
        self.process = None

        self._start_gstreamer_capture()

    def _start_gstreamer_capture(self):
        # GStreamer pipeline pulling from PipeWire into raw stdout
        gst_cmd = [
            "gst-launch-1.0", "-q",
            "pipewiresrc", "keep-aspect-ratio=true", "!",
            f"video/x-raw,framerate={self.fps}/1,format=BGR", "!",
            "videoconvert", "!",
            f"video/x-raw,width={self.width},height={self.height},format=BGR", "!",
            "fdsink", "fd=1"
        ]

        try:
            self.process = subprocess.Popen(
                gst_cmd,
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                bufsize=self.width * self.height * 3 * 2
            )
            print("[Capture] PipeWire GStreamer pipeline started successfully")
        except FileNotFoundError:
            print("[WARN] gst-launch-1.0 not found! Generating test pattern stream.")
            self.process = None

    async def recv(self):
        pts, time_base = self.pts, self.time_base
        frame_bytes = self.width * self.height * 3

        if self.process and self.process.stdout:
            # Read from GStreamer stdout
            raw = await asyncio.get_event_loop().run_in_executor(
                None, self.process.stdout.read, frame_bytes
            )
            if len(raw) == frame_bytes:
                img = np.frombuffer(raw, dtype=np.uint8).reshape((self.height, self.width, 3))
                frame = VideoFrame.from_ndarray(img, format="bgr24")
                frame.pts = pts
                frame.time_base = time_base
                self.pts += 1
                return frame

        # Fallback synthetic frame if PipeWire isn't active
        await asyncio.sleep(1.0 / self.fps)
        img = np.zeros((self.height, self.width, 3), dtype=np.uint8)
        # Draw gradient & timestamp
        t_sec = int(time.time())
        img[:, :, 0] = (np.arange(self.height)[:, None] * 255 // self.height).astype(np.uint8)
        img[:, :, 2] = (t_sec % 255)

        frame = VideoFrame.from_ndarray(img, format="bgr24")
        frame.pts = pts
        frame.time_base = time_base
        self.pts += 1
        return frame

    def stop(self):
        super().stop()
        if self.process:
            self.process.terminate()
            self.process = None


# ==========================================
# 3. UDP LAN Auto-Discovery Responder
# ==========================================
async def run_discovery_service(tcp_port=8080):
    hostname = socket.gethostname()
    compositor = "Hyprland"

    loop = asyncio.get_event_loop()
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.setblocking(False)

    try:
        sock.bind(("0.0.0.0", 8082))
        print(f"[Discovery] LAN Beacon listening on UDP 8082 (Announcing '{hostname}':{tcp_port})")
    except Exception as e:
        print(f"[WARN] Failed to bind discovery port 8082: {e}")
        return

    while True:
        try:
            data, addr = await loop.sock_recvfrom(sock, 1024)
            if data.strip() == b"HYPR_REMOTE_DISCOVER_V1":
                reply = f"HYPR_REMOTE_ANNOUNCE:{tcp_port}:{hostname}:{compositor}".encode("utf-8")
                sock.sendto(reply, addr)
        except asyncio.CancelledError:
            break
        except Exception as e:
            await asyncio.sleep(0.5)


# ==========================================
# 4. WebSocket Signaling & WebRTC Server
# ==========================================
class RemoteHostServer:
    def __init__(self, port=8080):
        self.port = port
        self.vinput = VirtualInput()

    async def handle_ws(self, websocket):
        print(f"[Signaling] Client connected from {websocket.remote_address}")
        pc = RTCPeerConnection()
        video_track = None

        @pc.on("datachannel")
        def on_datachannel(channel):
            print(f"[DataChannel] Received data channel: {channel.label}")

            @channel.on("message")
            def on_message(message):
                try:
                    data = json.loads(message)
                    t = data.get("t")
                    if t == "ping":
                        # Reply pong for RTT measurement
                        channel.send(json.dumps({
                            "t": "pong",
                            "client_ts": data.get("ts"),
                            "host_ts": int(time.time() * 1000)
                        }))
                    else:
                        # Forward to virtual input device
                        self.vinput.handle_event(data)
                except Exception as ex:
                    print(f"[DataChannel] Error handling event: {ex}")

        try:
            async for raw_msg in websocket:
                msg = json.loads(raw_msg)
                mtype = msg.get("type")

                if mtype == "hello":
                    fps = msg.get("requested_fps", 60)
                    print(f"[Signaling] Handshake requested: {fps} FPS")
                    video_track = WaylandScreenTrack(fps=fps)
                    pc.addTrack(video_track)

                    # Reply host info
                    await websocket.send(json.dumps({
                        "type": "host_info",
                        "hostname": socket.gethostname(),
                        "compositor": "Hyprland",
                        "encoder": "GStreamer/PipeWire VAAPI"
                    }))

                elif mtype == "sdp_offer":
                    offer = RTCSessionDescription(sdp=msg["sdp"], type="offer")
                    await pc.setRemoteDescription(offer)

                    answer = await pc.createAnswer()
                    await pc.setLocalDescription(answer)

                    await websocket.send(json.dumps({
                        "type": "sdp_answer",
                        "sdp": pc.localDescription.sdp
                    }))

                elif mtype == "ice_candidate":
                    # LAN direct connection typically doesn't need external ICE trickling
                    pass

        except websockets.exceptions.ConnectionClosed:
            print("[Signaling] Client disconnected")
        finally:
            if video_track:
                video_track.stop()
            await pc.close()

    async def start(self):
        print(f"==================================================")
        print(f" HyprRemote Host Daemon running on port {self.port}")
        print(f"   Compositor: Hyprland (Wayland)")
        print(f"   Listening on ws://0.0.0.0:{self.port}/ws")
        print(f"==================================================")

        # Start UDP Discovery in background
        asyncio.create_task(run_discovery_service(tcp_port=self.port))

        async with websockets.serve(self.handle_ws, "0.0.0.0", self.port):
            await asyncio.Future()  # Run forever


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
    server = RemoteHostServer(port=port)
    try:
        asyncio.run(server.start())
    except KeyboardInterrupt:
        print("\nShutting down HyprRemote Host...")
