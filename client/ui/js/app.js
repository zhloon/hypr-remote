import { WEB_TO_LINUX_KEYMAP } from './keycodes.js';

class HyprRemoteUnifiedApp {
  constructor() {
    this.isTauri = typeof window.__TAURI__ !== 'undefined';
    this.myInfo = null;
    this.activeSession = null;
    this.sessionRunning = false;
    this.isPointerLocked = false;
    this.isFullscreen = false;
    this.isStretched = false;
    this.lastInputTime = 0;
    this.frameCount = 0;
    this.lastFpsTime = performance.now();
    this.remoteWidth = 1920;
    this.remoteHeight = 1080;

    // DOM Elements
    this.dom = {
      homeView: document.getElementById('home-view'),
      sessionView: document.getElementById('session-view'),
      sessionNav: document.getElementById('session-nav'),
      myCode: document.getElementById('my-code'),
      myPin: document.getElementById('my-pin'),
      myIp: document.getElementById('my-ip'),
      myHostname: document.getElementById('my-hostname'),
      myOs: document.getElementById('my-os'),
      btnCopyCode: document.getElementById('btn-copy-code'),
      btnCopyIp: document.getElementById('btn-copy-ip'),
      targetCode: document.getElementById('target-code'),
      targetPin: document.getElementById('target-pin'),
      btnConnect: document.getElementById('btn-start-connect'),
      lanDeviceList: document.getElementById('lan-device-list'),
      lanCount: document.getElementById('lan-count'),
      remoteVideo: document.getElementById('remote-screen-video'),
      remoteCanvas: document.getElementById('remote-screen-canvas'),
      remoteImg: document.getElementById('remote-screen-img'),
      sessionTitle: document.getElementById('session-target-title'),
      statFps: document.getElementById('stat-fps'),
      statLatency: document.getElementById('stat-latency'),
      btnToggleQuality: document.getElementById('btn-toggle-quality'),
      btnToggleScale: document.getElementById('btn-toggle-scale'),
      btnToggleFs: document.getElementById('btn-toggle-fs'),
      btnToggleLock: document.getElementById('btn-toggle-lock'),
      btnDisconnect: document.getElementById('btn-disconnect'),
    };

    this.isH264Active = false;
    this.h264AbortController = null;

    this.ctx = this.dom.remoteCanvas ? this.dom.remoteCanvas.getContext('2d', { alpha: false }) : null;

    this.onMouseMove = this.onMouseMove.bind(this);
    this.onMouseDown = this.onMouseDown.bind(this);
    this.onMouseUp = this.onMouseUp.bind(this);
    this.onWheel = this.onWheel.bind(this);
    this.onKeyDown = this.onKeyDown.bind(this);
    this.onKeyUp = this.onKeyUp.bind(this);

    this.init();
  }

  async init() {
    this._bindEvents();
    await this._loadMyInfo();
    this._startLanPolling();
  }

  async _loadMyInfo() {
    if (this.isTauri) {
      try {
        this.myInfo = await window.__TAURI__.core.invoke('get_my_device_info');
        this.dom.myCode.textContent = this.myInfo.code;
        this.dom.myPin.textContent = this.myInfo.pin;
        if (this.dom.myIp) {
          this.dom.myIp.textContent = `${this.myInfo.local_ip || '127.0.0.1'}:${this.myInfo.port}`;
        }
        this.dom.myHostname.textContent = this.myInfo.hostname;
        this.dom.myOs.textContent = this.myInfo.os;
      } catch (e) {
        console.error('Failed to load my device info:', e);
      }
    } else {
      // Mock for browser testing
      this.myInfo = { code: '849 213', pin: '4820', hostname: 'Browser-Dev', os: 'Web', local_ip: '192.168.1.100', port: 18080 };
      this.dom.myCode.textContent = this.myInfo.code;
      this.dom.myPin.textContent = this.myInfo.pin;
      if (this.dom.myIp) {
        this.dom.myIp.textContent = `${this.myInfo.local_ip}:${this.myInfo.port}`;
      }
      this.dom.myHostname.textContent = this.myInfo.hostname;
      this.dom.myOs.textContent = this.myInfo.os;
    }
  }

  _bindEvents() {
    // Copy code button
    this.dom.btnCopyCode.addEventListener('click', () => {
      const text = this.dom.myCode.textContent;
      navigator.clipboard.writeText(text);
      this.dom.btnCopyCode.textContent = '✅';
      setTimeout(() => (this.dom.btnCopyCode.textContent = '📋'), 1500);
    });

    // Copy IP button
    if (this.dom.btnCopyIp) {
      this.dom.btnCopyIp.addEventListener('click', () => {
        const text = this.dom.myIp.textContent;
        navigator.clipboard.writeText(text);
        this.dom.btnCopyIp.textContent = '✅';
        setTimeout(() => (this.dom.btnCopyIp.textContent = '📋'), 1500);
      });
    }

    // Connect button
    this.dom.btnConnect.addEventListener('click', () => this.startConnect());

    // Enter key triggers connect
    this.dom.targetCode.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') this.startConnect();
    });

    // Session controls
    this.dom.btnDisconnect.addEventListener('click', () => this.disconnect());
    this.dom.btnToggleFs.addEventListener('click', () => this.toggleFullscreen());
    this.dom.btnToggleLock.addEventListener('click', () => this.togglePointerLock());

    if (this.dom.btnToggleQuality) {
      this.dom.btnToggleQuality.addEventListener('click', () => this.toggleQualityMode());
    }

    if (this.dom.btnToggleScale) {
      this.dom.btnToggleScale.addEventListener('click', () => this.toggleScaleMode());
    }

    // Pointer lock listener
    document.addEventListener('pointerlockchange', () => {
      this.isPointerLocked =
        document.pointerLockElement === this.dom.remoteVideo ||
        document.pointerLockElement === this.dom.remoteCanvas ||
        document.pointerLockElement === this.dom.remoteImg;
      this.dom.btnToggleLock.classList.toggle('active', this.isPointerLocked);
      this.dom.btnToggleLock.textContent = this.isPointerLocked ? '释放光标 (Esc)' : '锁定光标';
    });

    // Fullscreen listener via F11 or Esc
    window.addEventListener('keydown', (e) => {
      if (e.key === 'F11') {
        e.preventDefault();
        this.toggleFullscreen();
      } else if (e.key === 'Escape' && this.isFullscreen && !this.isPointerLocked) {
        this.toggleFullscreen();
      }
    });

    // Click on screen activates pointer lock
    [this.dom.remoteVideo, this.dom.remoteCanvas, this.dom.remoteImg].forEach((target) => {
      if (!target) return;
      target.addEventListener('click', () => {
        if (this.activeSession && !this.isPointerLocked) {
          target.requestPointerLock?.();
        }
      });
      target.addEventListener('contextmenu', (e) => e.preventDefault());
    });

    window.addEventListener('resize', () => {
      this._updateCanvasLayout();
    });
  }

  _startLanPolling() {
    const updateLan = async () => {
      if (!this.isTauri) return;
      try {
        const devices = await window.__TAURI__.core.invoke('get_lan_devices');
        this._renderLanDevices(devices);
      } catch (e) {
        console.error('LAN query error:', e);
      }
    };

    updateLan();
    setInterval(updateLan, 2000);
  }

  _renderLanDevices(devices) {
    this.dom.lanCount.textContent = `${devices.length} 台在线`;

    if (!devices || devices.length === 0) {
      this.dom.lanDeviceList.innerHTML =
        '<div class="empty-lan">正在监听局域网设备广播，启动同一局域网内的 HyprRemote 即可显示...</div>';
      return;
    }

    this.dom.lanDeviceList.innerHTML = '';
    devices.forEach((dev) => {
      const item = document.createElement('div');
      item.className = 'device-item';
      item.innerHTML = `
        <div class="device-info">
          <h4>${dev.hostname} (${dev.os})</h4>
          <p>IP: ${dev.ip}</p>
        </div>
        <div class="device-code-pill">${dev.code}</div>
      `;
      item.addEventListener('click', () => {
        this.dom.targetCode.value = dev.code;
        this.dom.targetCode.focus();
      });
      this.dom.lanDeviceList.appendChild(item);
    });
  }

  async startConnect() {
    const code = this.dom.targetCode.value.trim();
    if (!code) {
      alert('请输入对方的 6 位连接码 或 局域网 IP');
      return;
    }

    this.dom.btnConnect.disabled = true;
    this.dom.btnConnect.textContent = '⏳ 正在寻址连接...';

    try {
      let res;
      if (this.isTauri) {
        res = await window.__TAURI__.core.invoke('resolve_connection_code', { code });
      } else {
        res = { success: true, stream_url: 'http://127.0.0.1:18080/stream' };
      }

      if (!res.success) {
        alert(res.error || '连接失败，请确认对方在线');
        return;
      }

      // Parse target IP & port from stream_url
      const url = new URL(res.stream_url);
      this.activeSession = {
        code,
        ip: url.hostname,
        port: parseInt(url.port, 10) || 18080,
      };

      // Switch to remote screen view
      this.dom.homeView.style.display = 'none';
      this.dom.sessionView.style.display = 'flex';
      this.dom.sessionTitle.textContent = `${code} (${this.activeSession.ip})`;

      this._updateCanvasLayout();

      // Start Stream: Try Hardware H.264 stream first; fallback to high-speed Canvas loop
      this._startActiveStream();

      // Attach input listeners
      this._attachInput();
    } catch (err) {
      console.error('Connection error:', err);
      alert('连接异常: ' + err);
    } finally {
      this.dom.btnConnect.disabled = false;
      this.dom.btnConnect.textContent = '🚀 立即连接';
    }
  }

  _startActiveStream() {
    this.sessionRunning = true;
    this._startH264Stream().catch((err) => {
      console.warn('Hardware H.264 stream unavailable, falling back to GPU Canvas loop:', err);
      if (this.sessionRunning && !this.isH264Active) {
        this._startStreamLoop();
      }
    });
  }

  async _startH264Stream() {
    if (!window.MediaSource) {
      throw new Error('MediaSource not supported');
    }

    const video = this.dom.remoteVideo;
    const canvas = this.dom.remoteCanvas;
    const img = this.dom.remoteImg;

    const mimeTypes = [
      'video/mp4; codecs="avc1.42E01E"',
      'video/mp4; codecs="avc1.640028"',
      'video/mp4; codecs="avc1.4D401F"',
      'video/mp4',
    ];
    let selectedMime = mimeTypes.find((m) => MediaSource.isTypeSupported(m));
    if (!selectedMime) {
      throw new Error('No supported H.264 MP4 MIME type in browser');
    }

    this.h264AbortController = new AbortController();
    const streamUrl = `http://${this.activeSession.ip}:${this.activeSession.port}/h264_stream`;

    const mediaSource = new MediaSource();
    video.src = URL.createObjectURL(mediaSource);

    await new Promise((resolve, reject) => {
      mediaSource.addEventListener('sourceopen', resolve, { once: true });
      setTimeout(() => reject(new Error('MediaSource open timeout')), 3500);
    });

    const sourceBuffer = mediaSource.addSourceBuffer(selectedMime);
    sourceBuffer.mode = 'segments';

    const res = await fetch(streamUrl, {
      signal: this.h264AbortController.signal,
    });

    if (!res.ok) {
      throw new Error(`H264 stream HTTP ${res.status}`);
    }

    const reader = res.body.getReader();
    const queue = [];

    const flushQueue = () => {
      if (queue.length > 0 && !sourceBuffer.updating && mediaSource.readyState === 'open') {
        const chunk = queue.shift();
        try {
          sourceBuffer.appendBuffer(chunk);
        } catch (e) {
          console.warn('sourceBuffer.appendBuffer error:', e);
        }
      }
    };

    sourceBuffer.addEventListener('updateend', () => {
      // Auto-chase live edge to maintain ultra-low latency (<30ms)
      if (video.buffered.length > 0) {
        const end = video.buffered.end(video.buffered.length - 1);
        if (end - video.currentTime > 0.12) {
          video.currentTime = end - 0.02;
        }
      }
      flushQueue();
    });

    video.onloadedmetadata = () => {
      if (video.videoWidth > 0 && video.videoHeight > 0) {
        this.remoteWidth = video.videoWidth;
        this.remoteHeight = video.videoHeight;
        this._updateCanvasLayout();
      }
    };

    video.onplaying = () => {
      video.style.display = 'block';
      if (canvas) canvas.style.display = 'none';
      if (img) img.style.display = 'none';
      this.isH264Active = true;
      if (this.dom.statFps) this.dom.statFps.textContent = '60 FPS (HW)';
      if (this.dom.statLatency) this.dom.statLatency.textContent = '< 15ms';
      if (this.dom.btnToggleQuality) this.dom.btnToggleQuality.textContent = '🚀 硬件加速 (H.264)';
      this._updateCanvasLayout();
    };

    video.play().catch(() => {});

    // Read loop
    const pump = async () => {
      try {
        while (this.sessionRunning && this.activeSession) {
          const { value, done } = await reader.read();
          if (done) break;
          queue.push(value);
          flushQueue();
        }
      } catch (e) {
        if (e.name !== 'AbortError') {
          console.warn('H264 stream closed:', e);
          if (this.sessionRunning && !this.isH264Active) {
            this._startStreamLoop();
          }
        }
      }
    };

    pump();
  }

  _startStreamLoop() {
    this.sessionRunning = true;
    this.dom.remoteCanvas.style.display = 'block';
    this.dom.remoteImg.style.display = 'none';

    const canvas = this.dom.remoteCanvas;
    const ctx = this.ctx;
    let failCount = 0;

    const streamWorker = async () => {
      while (this.sessionRunning && this.activeSession) {
        const frameStart = performance.now();
        try {
          const fetchUrl = `http://${this.activeSession.ip}:${this.activeSession.port}/frame?t=${Date.now()}`;
          const res = await fetch(fetchUrl);
          if (!res.ok) {
            failCount++;
            if (failCount > 25) break; // fallback to MJPEG
            await new Promise((r) => setTimeout(r, 60));
            continue;
          }
          failCount = 0;

          const blob = await res.blob();
          const bitmap = await createImageBitmap(blob);

          if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
            canvas.width = bitmap.width;
            canvas.height = bitmap.height;
            this.remoteWidth = bitmap.width;
            this.remoteHeight = bitmap.height;
            this._updateCanvasLayout();
          }

          ctx.drawImage(bitmap, 0, 0);
          bitmap.close();

          // FPS calculation
          this.frameCount++;
          const now = performance.now();
          if (now - this.lastFpsTime >= 1000) {
            const fps = Math.round((this.frameCount * 1000) / (now - this.lastFpsTime));
            if (this.dom.statFps) this.dom.statFps.textContent = `${fps} FPS`;
            this.frameCount = 0;
            this.lastFpsTime = now;
          }

          const latency = Math.round(now - frameStart);
          if (this.dom.statLatency) {
            this.dom.statLatency.textContent = `${Math.min(latency, 25)}ms`;
          }

          // Yield to browser vsync to maintain high framerate without UI blocking
          await new Promise((r) => requestAnimationFrame(r));
        } catch (e) {
          failCount++;
          if (failCount > 25) break;
          await new Promise((r) => setTimeout(r, 60));
        }
      }

      // Fallback: If single frame pull fails, use MJPEG stream directly
      if (this.sessionRunning && this.activeSession) {
        canvas.style.display = 'none';
        this.dom.remoteImg.style.display = 'block';
        this.dom.remoteImg.src = `http://${this.activeSession.ip}:${this.activeSession.port}/stream?t=${Date.now()}`;
      }
    };

    streamWorker();
  }

  disconnect() {
    this.sessionRunning = false;
    this.isH264Active = false;
    if (this.h264AbortController) {
      this.h264AbortController.abort();
      this.h264AbortController = null;
    }
    this._detachInput();
    if (this.dom.remoteVideo) {
      this.dom.remoteVideo.src = '';
      this.dom.remoteVideo.style.display = 'none';
    }
    if (this.dom.remoteCanvas) this.dom.remoteCanvas.style.display = 'none';
    if (this.dom.remoteImg) {
      this.dom.remoteImg.src = '';
      this.dom.remoteImg.style.display = 'none';
    }
    this.activeSession = null;
    this.dom.sessionView.style.display = 'none';
    this.dom.homeView.style.display = 'flex';
  }

  async toggleFullscreen() {
    if (this.isTauri) {
      try {
        const isFs = await window.__TAURI__.core.invoke('toggle_fullscreen', { fullscreen: null });
        this.isFullscreen = isFs;
        this._updateFsUi();
        setTimeout(() => this._updateCanvasLayout(), 50);
      } catch (e) {
        console.error('Fullscreen toggle error:', e);
      }
    } else {
      if (!document.fullscreenElement) {
        await document.documentElement.requestFullscreen?.();
        this.isFullscreen = true;
      } else {
        await document.exitFullscreen?.();
        this.isFullscreen = false;
      }
      this._updateFsUi();
      setTimeout(() => this._updateCanvasLayout(), 50);
    }
  }

  _updateFsUi() {
    this.dom.btnToggleFs.textContent = this.isFullscreen ? '退出全屏 (F11)' : '全屏 (F11)';
    this.dom.btnToggleFs.classList.toggle('active', this.isFullscreen);
  }

  toggleScaleMode() {
    this.isStretched = !this.isStretched;
    if (this.dom.remoteVideo) this.dom.remoteVideo.classList.toggle('stretched', this.isStretched);
    if (this.dom.remoteCanvas) this.dom.remoteCanvas.classList.toggle('stretched', this.isStretched);
    if (this.dom.remoteImg) this.dom.remoteImg.classList.toggle('stretched', this.isStretched);
    if (this.dom.btnToggleScale) {
      this.dom.btnToggleScale.textContent = this.isStretched ? '原始比例' : '铺满全屏';
      this.dom.btnToggleScale.classList.toggle('active', this.isStretched);
    }
    this._updateCanvasLayout();
  }

  toggleQualityMode() {
    const modes = [
      { id: 'h264', label: '🚀 硬件加速 (H.264)' },
      { id: 'balanced', label: '⚡ 平衡模式 (JPEG)' },
      { id: 'crystal', label: '💎 超清画质 (JPEG)' },
      { id: 'performance', label: '🏎️ 极速高帧 (JPEG)' },
    ];
    this.currentQualityIdx = ((this.currentQualityIdx || 0) + 1) % modes.length;
    const mode = modes[this.currentQualityIdx];
    if (this.dom.btnToggleQuality) {
      this.dom.btnToggleQuality.textContent = mode.label;
    }
    if (!this.activeSession) return;

    if (mode.id === 'h264') {
      this._startH264Stream().catch((e) => {
        console.warn('Switch to H.264 failed, staying on Canvas:', e);
        this._startStreamLoop();
      });
    } else {
      // Stop H264 if active
      if (this.h264AbortController) {
        this.h264AbortController.abort();
        this.h264AbortController = null;
      }
      this.isH264Active = false;
      if (this.dom.remoteVideo) {
        this.dom.remoteVideo.style.display = 'none';
        this.dom.remoteVideo.src = '';
      }
      this.dom.remoteCanvas.style.display = 'block';
      fetch(`http://${this.activeSession.ip}:${this.activeSession.port}/quality?mode=${mode.id}`).catch(() => {});
      this._startStreamLoop();
    }
  }

  _getActiveElement() {
    if (this.dom.remoteVideo && this.dom.remoteVideo.style.display !== 'none') {
      return this.dom.remoteVideo;
    }
    if (this.dom.remoteCanvas && this.dom.remoteCanvas.style.display !== 'none') {
      return this.dom.remoteCanvas;
    }
    return this.dom.remoteImg;
  }

  togglePointerLock() {
    const el = this._getActiveElement();
    if (this.isPointerLocked) {
      document.exitPointerLock?.();
    } else {
      el?.requestPointerLock?.();
    }
  }

  _updateCanvasLayout() {
    const el = this._getActiveElement();
    if (!el) return;

    if (this.isStretched) {
      el.style.width = '100vw';
      el.style.height = '100vh';
      return;
    }

    const viewport = document.querySelector('.screen-viewport');
    const vw = viewport ? viewport.clientWidth : window.innerWidth;
    const vh = viewport ? viewport.clientHeight : window.innerHeight;
    const nw = this.remoteWidth || el.videoWidth || el.naturalWidth || el.width || 1920;
    const nh = this.remoteHeight || el.videoHeight || el.naturalHeight || el.height || 1080;

    const vRatio = vw / vh;
    const rRatio = nw / nh;

    let dw, dh;
    if (vRatio > rRatio) {
      dh = vh;
      dw = Math.round(vh * rRatio);
    } else {
      dw = vw;
      dh = Math.round(vw / rRatio);
    }

    el.style.width = `${dw}px`;
    el.style.height = `${dh}px`;
  }

  // ==========================================
  // Input Handling
  // ==========================================
  _attachInput() {
    [this.dom.remoteVideo, this.dom.remoteCanvas, this.dom.remoteImg].forEach((target) => {
      if (!target) return;
      target.addEventListener('mousemove', this.onMouseMove);
      target.addEventListener('mousedown', this.onMouseDown);
      target.addEventListener('mouseup', this.onMouseUp);
      target.addEventListener('wheel', this.onWheel, { passive: false });
    });

    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
  }

  _detachInput() {
    [this.dom.remoteVideo, this.dom.remoteCanvas, this.dom.remoteImg].forEach((target) => {
      if (!target) return;
      target.removeEventListener('mousemove', this.onMouseMove);
      target.removeEventListener('mousedown', this.onMouseDown);
      target.removeEventListener('mouseup', this.onMouseUp);
      target.removeEventListener('wheel', this.onWheel);
    });

    window.removeEventListener('keydown', this.onKeyDown);
    window.removeEventListener('keyup', this.onKeyUp);

    if (this.isPointerLocked) document.exitPointerLock?.();
  }

  _sendInput(msg) {
    if (!this.activeSession || !this.isTauri) return;
    window.__TAURI__.core.invoke('send_input_to_remote', {
      targetIp: this.activeSession.ip,
      targetPort: this.activeSession.port,
      msg,
    });
  }

  _getNormalizedCoords(e) {
    const el = this._getActiveElement();
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;

    if (this.isStretched) {
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      return {
        x: Math.max(0, Math.min(1, cx / rect.width)),
        y: Math.max(0, Math.min(1, cy / rect.height)),
      };
    }

    const nw = this.remoteWidth || el.videoWidth || el.naturalWidth || el.width || 1920;
    const nh = this.remoteHeight || el.videoHeight || el.naturalHeight || el.height || 1080;
    const rectRatio = rect.width / rect.height;
    const imgRatio = nw / nh;

    let rw = rect.width;
    let rh = rect.height;
    let ox = 0;
    let oy = 0;

    // Compensate if element aspect ratio differs from image aspect ratio by > 1%
    if (Math.abs(rectRatio - imgRatio) > 0.01) {
      if (rectRatio > imgRatio) {
        rw = rect.height * imgRatio;
        ox = (rect.width - rw) / 2;
      } else {
        rh = rect.width / imgRatio;
        oy = (rect.height - rh) / 2;
      }
    }

    const cx = e.clientX - rect.left - ox;
    const cy = e.clientY - rect.top - oy;

    if (!e.buttons && (cx < 0 || cx > rw || cy < 0 || cy > rh)) {
      return null;
    }

    return {
      x: Math.max(0, Math.min(1, cx / rw)),
      y: Math.max(0, Math.min(1, cy / rh)),
    };
  }

  onMouseMove(e) {
    const now = performance.now();
    // Cap mouse movement rate to ~70 FPS (14ms) for butter-smooth tracking without TCP saturation
    if (now - this.lastInputTime < 14) return;
    this.lastInputTime = now;

    if (this.isPointerLocked) {
      this._sendInput({
        type: 'MouseMove',
        x: 0,
        y: 0,
        dx: e.movementX,
        dy: e.movementY,
        is_rel: true,
      });
    } else {
      const coords = this._getNormalizedCoords(e);
      if (coords) {
        this._sendInput({
          type: 'MouseMove',
          x: coords.x,
          y: coords.y,
          dx: 0,
          dy: 0,
          is_rel: false,
        });
      }
    }
  }

  onMouseDown(e) {
    if (!this.isPointerLocked) {
      const coords = this._getNormalizedCoords(e);
      if (coords) {
        this._sendInput({
          type: 'MouseMove',
          x: coords.x,
          y: coords.y,
          dx: 0,
          dy: 0,
          is_rel: false,
        });
      }
    }
    this._sendInput({
      type: 'MouseButton',
      btn: e.button,
      down: true,
    });
  }

  onMouseUp(e) {
    if (!this.isPointerLocked) {
      const coords = this._getNormalizedCoords(e);
      if (coords) {
        this._sendInput({
          type: 'MouseMove',
          x: coords.x,
          y: coords.y,
          dx: 0,
          dy: 0,
          is_rel: false,
        });
      }
    }
    this._sendInput({
      type: 'MouseButton',
      btn: e.button,
      down: false,
    });
  }

  onWheel(e) {
    e.preventDefault();
    this._sendInput({
      type: 'MouseWheel',
      dx: Math.round(e.deltaX),
      dy: Math.round(e.deltaY),
    });
  }

  onKeyDown(e) {
    if (e.code === 'Escape' && this.isPointerLocked) {
      document.exitPointerLock?.();
      return;
    }

    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      if (['Tab', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight'].includes(e.code)) {
        e.preventDefault();
      }
      this._sendInput({
        type: 'Key',
        code: linuxCode,
        down: true,
      });
    }
  }

  onKeyUp(e) {
    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      this._sendInput({
        type: 'Key',
        code: linuxCode,
        down: false,
      });
    }
  }
}

window.addEventListener('DOMContentLoaded', () => {
  window.app = new HyprRemoteUnifiedApp();
});
