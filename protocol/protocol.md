# HyprRemote LAN Communication Protocol Specification (v1.0)

This document defines the communication protocol between the **HyprRemote Client** (Windows / macOS / Linux) and the **HyprRemote Host** (Wayland / Hyprland).

---

## 1. Overview

Communication is split into two channels:
1. **Signaling Channel (WebSocket / HTTP)**: Used for initial connection negotiation, SDP exchange, and ICE candidates.
2. **Media & Data Channel (WebRTC Direct)**:
   - **Media Track (Video + Audio)**: RTP over UDP/SRTP with H.264 / HEVC / AV1 hardware encoded stream.
   - **DataChannel `control`**: Low-latency, unordered/ordered bidirectional binary or JSON channel for user input, clipboard, and metrics.

---

## 2. Signaling Phase (WebSocket: `ws://<host_ip>:<port>/ws`)

### 2.1 Handshake & Capabilities
Client connects and sends hello:
```json
{
  "type": "hello",
  "client_name": "HyprRemote-macOS",
  "version": "1.0.0",
  "requested_fps": 60,
  "requested_bitrate_kbps": 25000,
  "resolution": { "width": 1920, "height": 1080 }
}
```

Host responds with host capabilities:
```json
{
  "type": "host_info",
  "hostname": "arch-hyprland",
  "compositor": "Hyprland",
  "display": { "width": 2560, "height": 1440, "refresh_rate": 165 },
  "codecs": ["H264", "HEVC", "AV1"],
  "encoder": "VAAPI (AMD Radeon 780M)"
}
```

### 2.2 SDP Exchange
* Client sends WebRTC Offer:
```json
{
  "type": "sdp_offer",
  "sdp": "v=0\r\no=..."
}
```
* Host responds with WebRTC Answer:
```json
{
  "type": "sdp_answer",
  "sdp": "v=0\r\no=..."
}
```

### 2.3 ICE Candidates (LAN Direct)
```json
{
  "type": "ice_candidate",
  "candidate": {
    "candidate": "candidate:1 1 UDP ... <host_ip> 50000 typ host ...",
    "sdpMid": "0",
    "sdpMLineIndex": 0
  }
}
```

---

## 3. DataChannel Input Protocol (`control` channel)

All control messages sent via WebRTC DataChannel are lightweight JSON packets (or binary packed in v2).

### 3.1 Mouse Events

#### Absolute Mouse Move (Standard Desktop Mode)
Coordinates normalized from `0.0` to `1.0` (independent of local client window size):
```json
{
  "t": "m_abs",
  "x": 0.48512,
  "y": 0.32045
}
```

#### Relative Mouse Move (Pointer Lock / Gaming / FPS Mode)
Delta pixels from mouse hardware sensor:
```json
{
  "t": "m_rel",
  "dx": -4,
  "dy": 2
}
```

#### Mouse Button
`btn`: 0 = Left (BTN_LEFT: 272), 1 = Middle (BTN_MIDDLE: 274), 2 = Right (BTN_RIGHT: 273), 3 = Back (BTN_SIDE: 275), 4 = Forward (BTN_EXTRA: 276).
`down`: `true` for press, `false` for release.
```json
{
  "t": "m_btn",
  "btn": 0,
  "down": true
}
```

#### Mouse Scroll / Wheel
```json
{
  "t": "m_wheel",
  "dx": 0,
  "dy": -120
}
```

### 3.2 Keyboard Events
Uses standard Linux `input-event-codes` (`KEY_*` numeric code) so the host can directly feed `/dev/uinput`:
```json
{
  "t": "key",
  "code": 125,
  "down": true
}
```
*Key codes of interest:*
* `125`: `KEY_LEFTMETA` (Super / Windows / Command 键)
* `126`: `KEY_RIGHTMETA`
* `29`: `KEY_LEFTCTRL`
* `56`: `KEY_LEFTALT`
* `15`: `KEY_TAB`
* `1`: `KEY_ESC`
* `28`: `KEY_ENTER`
* `14`: `KEY_BACKSPACE`

### 3.3 Clipboard Sync
Bidirectional clipboard synchronization:
```json
{
  "t": "clip",
  "text": "Hello from client clipboard!"
}
```

### 3.4 Ping / Heartbeat & RTT Measurement
Client sends periodic ping (every 1000ms):
```json
{
  "t": "ping",
  "ts": 1729524000123
}
```
Host replies immediately:
```json
{
  "t": "pong",
  "client_ts": 1729524000123,
  "host_ts": 1729524000127
}
```
Client computes `RTT = Date.now() - client_ts`.
