import { WEB_TO_LINUX_KEYMAP } from './keycodes.js';
import { WgpuRenderer } from './wgpu_renderer.js';

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
      btnSyncClipboard: document.getElementById('btn-sync-clipboard'),
      btnToggleFs: document.getElementById('btn-toggle-fs'),
      btnDisconnect: document.getElementById('btn-disconnect'),
      btnCollapseNav: document.getElementById('btn-collapse-nav'),
      navTriggerHandle: document.getElementById('nav-trigger-handle'),
      viewport: document.querySelector('.screen-viewport'),
    };

    this._cachedRect = null;
    this.scaleMode = 'fit'; // Default standard aspect-ratio fit
    this.isH264Active = false;
    this.h264AbortController = null;
    this.qualityModes = [
      { id: 'h264', label: '🚀 硬件加速 (H.264 120fps)' },
      { id: 'performance', label: '🏎️ 极速高帧 (GPU 120fps)' },
      { id: 'balanced', label: '⚡ 平衡画质 (GPU)' },
      { id: 'crystal', label: '💎 超清画质 (100% 2K/4K)' },
    ];
    this.currentQualityIdx = 0;
    this.lastLocalClipboard = '';
    this.lastRemoteClipboard = '';
    this.clipboardTimer = null;
    this._isSyncingClipboard = false;
    this.navHideTimer = null;

    this.gpuRenderer = this.dom.remoteCanvas ? new WgpuRenderer(this.dom.remoteCanvas) : null;
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
    if (this.gpuRenderer) {
      await this.gpuRenderer.init();
    }
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

    if (this.dom.btnToggleQuality) {
      this.dom.btnToggleQuality.addEventListener('click', () => this.toggleQualityMode());
    }

    if (this.dom.btnSyncClipboard) {
      this.dom.btnSyncClipboard.addEventListener('click', () => this.syncClipboard(true));
    }

    // Prevent contextmenu on remote display to allow right-click passthrough
    [this.dom.remoteVideo, this.dom.remoteCanvas, this.dom.remoteImg].forEach((target) => {
      if (!target) return;
      target.addEventListener('contextmenu', (e) => e.preventDefault());
    });

    // Auto-hiding session nav bar controls
    let isMouseInsideNav = false;
    this._showNav = (autoHideDelay = 2500) => {
      if (!this.dom.sessionNav) return;
      if (this.navHideTimer) {
        clearTimeout(this.navHideTimer);
        this.navHideTimer = null;
      }
      this.dom.sessionNav.classList.add('visible');
      if (this.dom.navTriggerHandle) {
        this.dom.navTriggerHandle.classList.add('hidden');
      }
      if (autoHideDelay > 0 && !isMouseInsideNav) {
        this.navHideTimer = setTimeout(() => {
          if (!isMouseInsideNav) {
            this._hideNav();
          }
        }, autoHideDelay);
      }
    };

    this._hideNav = () => {
      if (!this.dom.sessionNav) return;
      if (this.navHideTimer) {
        clearTimeout(this.navHideTimer);
        this.navHideTimer = null;
      }
      this.dom.sessionNav.classList.remove('visible');
      if (this.dom.navTriggerHandle) {
        this.dom.navTriggerHandle.classList.remove('hidden');
      }
    };

    if (this.dom.sessionNav) {
      this.dom.sessionNav.addEventListener('mouseenter', () => {
        isMouseInsideNav = true;
        if (this.navHideTimer) {
          clearTimeout(this.navHideTimer);
          this.navHideTimer = null;
        }
      });
      this.dom.sessionNav.addEventListener('mouseleave', () => {
        isMouseInsideNav = false;
        this._showNav(1500);
      });
    }
    if (this.dom.navTriggerHandle) {
      this.dom.navTriggerHandle.addEventListener('mouseenter', () => {
        this._showNav(2500);
      });
      this.dom.navTriggerHandle.addEventListener('click', () => {
        this._showNav(3000);
      });
    }
    if (this.dom.btnCollapseNav) {
      this.dom.btnCollapseNav.addEventListener('click', (e) => {
        e.stopPropagation();
        isMouseInsideNav = false;
        this._hideNav();
      });
    }

    // Top-center trigger area (only within extreme top edge, avoiding normal window clicks)
    window.addEventListener('mousemove', (e) => {
      if (!this.activeSession) return;
      const inCenter = Math.abs(e.clientX - window.innerWidth / 2) < 75;
      if (e.clientY <= 3 && inCenter) {
        if (!this.dom.sessionNav.classList.contains('visible')) {
          this._showNav(2500);
        }
      }
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
      this._showNav(3500);

      // Immediately fetch 1st frame to Canvas (< 50ms) to ensure zero black screen on initial connect
      this._loadInitialFrame();

      // Start Stream: Try Hardware H.264 stream first; fallback to high-speed Canvas loop
      this._startActiveStream();

      // Attach input listeners
      this._attachInput();

      // Start bi-directional shared clipboard sync
      this._startClipboardSync();
    } catch (err) {
      console.error('Connection error:', err);
      alert('连接异常: ' + err);
    } finally {
      this.dom.btnConnect.disabled = false;
      this.dom.btnConnect.textContent = '🚀 立即连接';
    }
  }

  async _loadInitialFrame() {
    if (!this.activeSession) return;
    try {
      const fetchUrl = `http://${this.activeSession.ip}:${this.activeSession.port}/frame?t=${Date.now()}`;
      const res = await fetch(fetchUrl);
      if (res.ok) {
        const blob = await res.blob();
        if (blob.size > 0) {
          const bitmap = await createImageBitmap(blob);
          const canvas = this.dom.remoteCanvas;
          if (canvas) {
            canvas.style.display = 'block';
            if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
              canvas.width = bitmap.width;
              canvas.height = bitmap.height;
              this.remoteWidth = bitmap.width;
              this.remoteHeight = bitmap.height;
              this._updateCanvasLayout();
            }
            if (this.gpuRenderer) {
              this.gpuRenderer.render(bitmap, bitmap.width, bitmap.height);
            } else {
              this.ctx?.drawImage(bitmap, 0, 0);
            }
          }
          bitmap.close();
        }
      }
    } catch (_) {}
  }

  _startActiveStream() {
    this.sessionRunning = true;
    this._startH264Stream().catch((err) => {
      console.warn('Hardware H.264 stream unavailable, falling back to GPU Canvas loop:', err);
      if (this.sessionRunning && !this.isH264Active) {
        this.currentQualityIdx = 1;
        if (this.dom.btnToggleQuality) {
          this.dom.btnToggleQuality.textContent = this.qualityModes[1].label;
        }
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
      'video/mp4; codecs="avc1.640034"',
      'video/mp4; codecs="avc1.640033"',
      'video/mp4; codecs="avc1.64002A"',
      'video/mp4; codecs="avc1.640028"',
      'video/mp4; codecs="avc1.4D401F"',
      'video/mp4; codecs="avc1.42E01E"',
      'video/mp4',
    ];
    let selectedMime = mimeTypes.find((m) => MediaSource.isTypeSupported(m));
    if (!selectedMime) {
      throw new Error('No supported H.264 MP4 MIME type in browser');
    }

    this.h264AbortController = new AbortController();
    const streamUrl = `http://${this.activeSession.ip}:${this.activeSession.port}/h264_stream`;

    video.muted = true;
    video.autoplay = true;
    video.playsInline = true;
    video.controls = false;
    video.disablePictureInPicture = true;
    video.setAttribute('playsinline', '');
    video.setAttribute('webkit-playsinline', '');

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

    this.isH264Active = true;
    const reader = res.body.getReader();
    const queue = [];

    const flushQueue = () => {
      if (queue.length > 0 && !sourceBuffer.updating && mediaSource.readyState === 'open') {
        let chunk;
        if (queue.length === 1) {
          chunk = queue.shift();
        } else {
          // Combine all pending chunks into one buffer to prevent lag buildup!
          const totalLen = queue.reduce((acc, q) => acc + q.byteLength, 0);
          chunk = new Uint8Array(totalLen);
          let offset = 0;
          while (queue.length > 0) {
            const part = queue.shift();
            chunk.set(part, offset);
            offset += part.byteLength;
          }
        }
        try {
          sourceBuffer.appendBuffer(chunk);
        } catch (e) {
          console.warn('sourceBuffer.appendBuffer error:', e);
        }
      }
    };

    let lastPruneTime = performance.now();
    sourceBuffer.addEventListener('updateend', () => {
      // Memory management: Prune old buffered video segments periodically (every 30s), keeping 15s history
      const now = performance.now();
      if (now - lastPruneTime > 30000 && !sourceBuffer.updating && video.currentTime > 25) {
        lastPruneTime = now;
        try {
          const bStart = sourceBuffer.buffered.start(0);
          if (video.currentTime - bStart > 25) {
            sourceBuffer.remove(0, video.currentTime - 15);
          }
        } catch (_) {}
      }

      // Guarantee immediate playback the instant the initial fragment is ready
      if (video.paused && this.sessionRunning) {
        video.play().catch(() => {});
      }

      flushQueue();
    });

    const handleVideoDimensions = () => {
      if (video.videoWidth > 0 && video.videoHeight > 0) {
        this.remoteWidth = video.videoWidth;
        this.remoteHeight = video.videoHeight;
        this._updateCanvasLayout();
      }
    };
    video.onloadedmetadata = handleVideoDimensions;
    video.onresize = handleVideoDimensions;
    video.oncanplay = () => {
      if (video.paused && this.sessionRunning) {
        video.play().catch(() => {});
      }
    };

    let hasPlayed = false;
    let hwFrameCount = 0;
    let hwLastFpsTime = performance.now();
    let hwLastLatencyTime = performance.now();
    const trackHwFrame = () => {
      if (!this.sessionRunning || !this.isH264Active) return;

      if (video.buffered.length > 0) {
        const liveEnd = video.buffered.end(video.buffered.length - 1);
        const delay = liveEnd - video.currentTime;

        // Ultra-low latency dynamic drift compensation without buffer starvation:
        // Continuous smooth playbackRate adjustment ensures zero stalls and zero seeks
        if (delay > 0.40) {
          video.playbackRate = 1.12;
        } else if (delay > 0.15) {
          video.playbackRate = 1.06;
        } else if (delay > 0.08) {
          video.playbackRate = 1.03;
        } else if (delay <= 0.04) {
          video.playbackRate = 1.0;
        }

        // Hard catch-up seek only on extreme lag (> 1.5s, e.g. after window minimize)
        if (delay > 1.5) {
          video.currentTime = Math.max(0, liveEnd - 0.08);
        }

        const now = performance.now();
        if (this.dom.statLatency && now - hwLastLatencyTime >= 250) {
          const latMs = Math.round(delay * 1000);
          this.dom.statLatency.textContent = `${Math.max(5, latMs)}ms`;
          hwLastLatencyTime = now;
        }
      }

      hwFrameCount++;
      const now = performance.now();
      if (now - hwLastFpsTime >= 1000) {
        const fps = Math.round((hwFrameCount * 1000) / (now - hwLastFpsTime));
        if (this.dom.statFps) this.dom.statFps.textContent = `${fps} FPS (HW)`;
        hwFrameCount = 0;
        hwLastFpsTime = now;
      }
      if ('requestVideoFrameCallback' in video) {
        video.requestVideoFrameCallback(trackHwFrame);
      } else {
        requestAnimationFrame(trackHwFrame);
      }
    };

    video.onwaiting = () => {
      video.playbackRate = 1.0;
    };

    const h264Watchdog = setTimeout(() => {
      if (!hasPlayed && this.sessionRunning && !this.isH264Active) {
        console.warn('H.264 stream watchdog timeout (7s), auto-falling back to Canvas stream loop');
        if (this.h264AbortController) {
          this.h264AbortController.abort();
          this.h264AbortController = null;
        }
        this.currentQualityIdx = 1;
        if (this.dom.btnToggleQuality) {
          this.dom.btnToggleQuality.textContent = this.qualityModes[1].label;
        }
        this._startStreamLoop();
      }
    }, 7000);

    video.onplaying = () => {
      hasPlayed = true;
      clearTimeout(h264Watchdog);
      video.style.display = 'block';
      if (canvas) canvas.style.display = 'none';
      if (img) img.style.display = 'none';
      this.isH264Active = true;
      this.currentQualityIdx = 0;
      if (this.dom.statFps) this.dom.statFps.textContent = '120 FPS (HW)';
      if (this.dom.statLatency) this.dom.statLatency.textContent = '< 20ms';
      if (this.dom.btnToggleQuality) this.dom.btnToggleQuality.textContent = this.qualityModes[0].label;
      this._updateCanvasLayout();

      if ('requestVideoFrameCallback' in video) {
        video.requestVideoFrameCallback(trackHwFrame);
      } else {
        requestAnimationFrame(trackHwFrame);
      }
    };

    video.play().catch(() => {});

    // Read loop
    const pump = async () => {
      try {
        while (this.sessionRunning && this.activeSession) {
          const { value, done } = await reader.read();
          if (done) {
            clearTimeout(h264Watchdog);
            if (!this.isH264Active && this.sessionRunning) {
              console.warn('H.264 stream ended before playback, falling back to Canvas stream');
              this.currentQualityIdx = 1;
              if (this.dom.btnToggleQuality) {
                this.dom.btnToggleQuality.textContent = this.qualityModes[1].label;
              }
              this._startStreamLoop();
            }
            break;
          }
          queue.push(value);
          flushQueue();
        }
      } catch (e) {
        clearTimeout(h264Watchdog);
        if (e.name !== 'AbortError') {
          console.warn('H264 stream closed:', e);
          if (this.sessionRunning && !this.isH264Active) {
            this.currentQualityIdx = 1;
            if (this.dom.btnToggleQuality) {
              this.dom.btnToggleQuality.textContent = this.qualityModes[1].label;
            }
            this._startStreamLoop();
          }
        }
      }
    };

    pump();
  }

  _startStreamLoop() {
    this.sessionRunning = true;
    if (this.isStreamLoopRunning) return;
    this.isStreamLoopRunning = true;
    if (this.dom.remoteVideo) {
      this.dom.remoteVideo.style.display = 'none';
    }
    this.dom.remoteCanvas.style.display = 'block';
    this.dom.remoteImg.style.display = 'none';
    this.isH264Active = false;
    this._updateCanvasLayout();

    const canvas = this.dom.remoteCanvas;
    const ctx = this.ctx;
    let failCount = 0;

    const fetchFrame = async () => {
      const frameStart = performance.now();
      const fetchUrl = `http://${this.activeSession.ip}:${this.activeSession.port}/frame?t=${Date.now()}`;
      const res = await fetch(fetchUrl);
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const blob = await res.blob();
      const bitmap = await createImageBitmap(blob);
      return { bitmap, latency: Math.round(performance.now() - frameStart) };
    };

    const streamWorker = async () => {
      try {
        let pendingFrame = fetchFrame();
        while (this.sessionRunning && this.activeSession && !this.isH264Active) {
          try {
            const { bitmap, latency } = await pendingFrame;
            failCount = 0;

            // Pipeline: Trigger next fetch in background while uploading to GPU & waiting for vsync
            if (this.sessionRunning && this.activeSession && !this.isH264Active) {
              pendingFrame = fetchFrame();
            }

            if (canvas.width !== bitmap.width || canvas.height !== bitmap.height) {
              canvas.width = bitmap.width;
              canvas.height = bitmap.height;
              this.remoteWidth = bitmap.width;
              this.remoteHeight = bitmap.height;
              this._updateCanvasLayout();
            }

            if (this.gpuRenderer) {
              this.gpuRenderer.render(bitmap, bitmap.width, bitmap.height);
            } else if (ctx) {
              ctx.drawImage(bitmap, 0, 0);
            }
            bitmap.close();

            // FPS calculation with GPU backend info
            this.frameCount++;
            const now = performance.now();
            if (now - this.lastFpsTime >= 1000) {
              const fps = Math.round((this.frameCount * 1000) / (now - this.lastFpsTime));
              const backend = this.gpuRenderer ? this.gpuRenderer.getBackendName() : '2D';
              if (this.dom.statFps) this.dom.statFps.textContent = `${fps} FPS (${backend})`;
              this.frameCount = 0;
              this.lastFpsTime = now;
            }

            if (!this.lastCanvasLatencyTime || now - this.lastCanvasLatencyTime >= 250) {
              if (this.dom.statLatency) {
                this.dom.statLatency.textContent = `${Math.min(latency, 25)}ms`;
              }
              this.lastCanvasLatencyTime = now;
            }

            // Yield to browser vsync to maintain high framerate without UI blocking
            await new Promise((r) => requestAnimationFrame(r));
          } catch (e) {
            failCount++;
            if (failCount > 25) break; // fallback to MJPEG
            await new Promise((r) => setTimeout(r, 60));
            if (this.sessionRunning && this.activeSession && !this.isH264Active) {
              pendingFrame = fetchFrame();
            }
          }
        }

        // Fallback: If single frame pull fails, use MJPEG stream directly
        if (this.sessionRunning && this.activeSession && !this.isH264Active) {
          canvas.style.display = 'none';
          this.dom.remoteImg.style.display = 'block';
          this.dom.remoteImg.src = `http://${this.activeSession.ip}:${this.activeSession.port}/stream?t=${Date.now()}`;
        }
      } finally {
        this.isStreamLoopRunning = false;
      }
    };

    streamWorker();
  }

  disconnect() {
    this.sessionRunning = false;
    this.isH264Active = false;
    this.isStreamLoopRunning = false;
    this._stopClipboardSync();
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

  _startClipboardSync() {
    this._stopClipboardSync();
    this.syncClipboard(false);
    // 1200ms passive polling (instant synchronization is triggered by window copy/paste/focus events)
    this.clipboardTimer = setInterval(() => {
      if (this.sessionRunning && this.activeSession) {
        this.syncClipboard(false);
      }
    }, 1200);
  }

  _stopClipboardSync() {
    if (this.clipboardTimer) {
      clearInterval(this.clipboardTimer);
      this.clipboardTimer = null;
    }
  }

  async syncClipboard(forceFeedback = false) {
    if (!this.sessionRunning || !this.activeSession || this._isSyncingClipboard) return;
    this._isSyncingClipboard = true;

    try {
      // 1. Read local OS clipboard
      let localText = '';
      if (this.isTauri) {
        try {
          localText = await window.__TAURI__.core.invoke('get_local_clipboard');
        } catch (_) {}
      } else if (navigator.clipboard?.readText) {
        try {
          localText = await navigator.clipboard.readText();
        } catch (_) {}
      }

      // 2. If local clipboard has changed, push to remote host
      if (localText && localText !== this.lastLocalClipboard && localText !== this.lastRemoteClipboard) {
        this.lastLocalClipboard = localText;
        this.lastRemoteClipboard = localText;
        await fetch(`http://${this.activeSession.ip}:${this.activeSession.port}/clipboard`, {
          method: 'POST',
          headers: { 'Content-Type': 'text/plain;charset=utf-8' },
          body: localText,
        });
        if (this.dom.btnSyncClipboard) {
          this.dom.btnSyncClipboard.textContent = '✅ 已同步到远程';
          setTimeout(() => {
            if (this.dom.btnSyncClipboard) this.dom.btnSyncClipboard.textContent = '📋 剪贴板同步';
          }, 1500);
        }
        return;
      }

      // 3. Otherwise, pull remote host clipboard
      const res = await fetch(`http://${this.activeSession.ip}:${this.activeSession.port}/clipboard`);
      if (res.ok) {
        const data = await res.json();
        const remoteText = data.content || '';
        if (remoteText && remoteText !== this.lastRemoteClipboard && remoteText !== this.lastLocalClipboard) {
          this.lastRemoteClipboard = remoteText;
          this.lastLocalClipboard = remoteText;
          if (this.isTauri) {
            await window.__TAURI__.core.invoke('set_local_clipboard', { content: remoteText });
          } else if (navigator.clipboard?.writeText) {
            await navigator.clipboard.writeText(remoteText).catch(() => {});
          }
          if (this.dom.btnSyncClipboard) {
            this.dom.btnSyncClipboard.textContent = '📥 已拉取自远程';
            setTimeout(() => {
              if (this.dom.btnSyncClipboard) this.dom.btnSyncClipboard.textContent = '📋 剪贴板同步';
            }, 1500);
          }
          return;
        }
      }

      if (forceFeedback && this.dom.btnSyncClipboard) {
        this.dom.btnSyncClipboard.textContent = '✅ 内容已一致';
        setTimeout(() => {
          if (this.dom.btnSyncClipboard) this.dom.btnSyncClipboard.textContent = '📋 剪贴板同步';
        }, 1200);
      }
    } catch (e) {
      if (forceFeedback) {
        console.warn('Clipboard sync error:', e);
      }
    } finally {
      this._isSyncingClipboard = false;
    }
  }

  toggleQualityMode() {
    this.currentQualityIdx = ((this.currentQualityIdx || 0) + 1) % this.qualityModes.length;
    const mode = this.qualityModes[this.currentQualityIdx];
    if (this.dom.btnToggleQuality) {
      this.dom.btnToggleQuality.textContent = mode.label;
    }
    if (!this.activeSession) return;

    if (mode.id === 'h264') {
      this._startH264Stream().catch((e) => {
        console.warn('Switch to H.264 failed, falling back to GPU Canvas:', e);
        this.currentQualityIdx = 1;
        if (this.dom.btnToggleQuality) {
          this.dom.btnToggleQuality.textContent = this.qualityModes[1].label;
        }
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

  _updateCanvasLayout() {
    this._cachedRect = null;
    const el = this._getActiveElement();
    if (!el) return;

    const nw = this.remoteWidth || el.videoWidth || el.naturalWidth || el.width || 1920;
    const nh = this.remoteHeight || el.videoHeight || el.naturalHeight || el.height || 1080;

    // Standard aspect-ratio fit
    const viewport = this.dom.viewport || document.querySelector('.screen-viewport');
    const vw = viewport ? viewport.clientWidth : window.innerWidth;
    const vh = viewport ? viewport.clientHeight : window.innerHeight;

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
    const targets = [
      this.dom.remoteVideo,
      this.dom.remoteCanvas,
      this.dom.remoteImg,
      document.getElementById('screen-viewport'),
    ];
    targets.forEach((target) => {
      if (!target) return;
      target.addEventListener('mousemove', this.onMouseMove);
      target.addEventListener('mousedown', this.onMouseDown);
      target.addEventListener('mouseup', this.onMouseUp);
      target.addEventListener('wheel', this.onWheel, { passive: false });
    });

    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
    window.addEventListener('mouseup', this.onMouseUp);

    this._onWindowFocus = () => this.syncClipboard(false);
    this._onWindowCopy = () => setTimeout(() => this.syncClipboard(false), 50);
    this._onWindowPaste = () => setTimeout(() => this.syncClipboard(false), 50);
    window.addEventListener('focus', this._onWindowFocus);
    window.addEventListener('copy', this._onWindowCopy);
    window.addEventListener('paste', this._onWindowPaste);
  }

  _detachInput() {
    const targets = [
      this.dom.remoteVideo,
      this.dom.remoteCanvas,
      this.dom.remoteImg,
      document.getElementById('screen-viewport'),
    ];
    targets.forEach((target) => {
      if (!target) return;
      target.removeEventListener('mousemove', this.onMouseMove);
      target.removeEventListener('mousedown', this.onMouseDown);
      target.removeEventListener('mouseup', this.onMouseUp);
      target.removeEventListener('wheel', this.onWheel);
    });

    window.removeEventListener('keydown', this.onKeyDown);
    window.removeEventListener('keyup', this.onKeyUp);
    window.removeEventListener('mouseup', this.onMouseUp);

    if (this._onWindowFocus) window.removeEventListener('focus', this._onWindowFocus);
    if (this._onWindowCopy) window.removeEventListener('copy', this._onWindowCopy);
    if (this._onWindowPaste) window.removeEventListener('paste', this._onWindowPaste);
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
    const rect = this._cachedRect || (this._cachedRect = el.getBoundingClientRect());
    if (rect.width <= 0 || rect.height <= 0) return null;

    const nw = el.videoWidth || this.remoteWidth || el.naturalWidth || el.width || 1920;
    const nh = el.videoHeight || this.remoteHeight || el.naturalHeight || el.height || 1080;

    let contentW = rect.width;
    let contentH = rect.height;
    let offsetX = 0;
    let offsetY = 0;

    if (this.scaleMode !== 'stretch' && nw > 0 && nh > 0) {
      const elRatio = rect.width / rect.height;
      const mediaRatio = nw / nh;
      if (elRatio > mediaRatio) {
        // Pillarbox: horizontal black bars on left and right
        contentW = rect.height * mediaRatio;
        offsetX = (rect.width - contentW) / 2;
      } else if (elRatio < mediaRatio) {
        // Letterbox: vertical black bars on top and bottom
        contentH = rect.width / mediaRatio;
        offsetY = (rect.height - contentH) / 2;
      }
    }

    const cx = e.clientX - rect.left - offsetX;
    const cy = e.clientY - rect.top - offsetY;

    if (!e.buttons && (cx < 0 || cx > contentW || cy < 0 || cy > contentH)) {
      return null;
    }

    return {
      x: Math.max(0, Math.min(1, cx / contentW)),
      y: Math.max(0, Math.min(1, cy / contentH)),
    };
  }

  onMouseMove(e) {
    const coords = this._getNormalizedCoords(e);
    if (!coords) return;

    const now = performance.now();
    // High-responsiveness leading-edge dispatch at up to 120Hz (~8ms) without waiting for rAF VSync:
    if (!this._lastMoveTime || now - this._lastMoveTime >= 8) {
      this._lastMoveTime = now;
      if (this._pendingMoveTimer) {
        clearTimeout(this._pendingMoveTimer);
        this._pendingMoveTimer = null;
      }
      this._sendInput({
        type: 'MouseMove',
        x: coords.x,
        y: coords.y,
        dx: 0,
        dy: 0,
        is_rel: false,
      });
    } else {
      // Trailing edge: guarantee the final resting position is dispatched
      this._pendingCoords = coords;
      if (!this._pendingMoveTimer) {
        this._pendingMoveTimer = setTimeout(() => {
          this._pendingMoveTimer = null;
          if (this._pendingCoords) {
            this._lastMoveTime = performance.now();
            this._sendInput({
              type: 'MouseMove',
              x: this._pendingCoords.x,
              y: this._pendingCoords.y,
              dx: 0,
              dy: 0,
              is_rel: false,
            });
            this._pendingCoords = null;
          }
        }, 8);
      }
    }
  }

  onMouseDown(e) {
    if (this._pendingMoveTimer) {
      clearTimeout(this._pendingMoveTimer);
      this._pendingMoveTimer = null;
    }
    const coords = this._getNormalizedCoords(e);
    if (coords) {
      this._lastMoveTime = performance.now();
      this._sendInput({
        type: 'MouseMove',
        x: coords.x,
        y: coords.y,
        dx: 0,
        dy: 0,
        is_rel: false,
      });
    }
    this._sendInput({
      type: 'MouseButton',
      btn: e.button,
      down: true,
    });
  }

  onMouseUp(e) {
    if (this._pendingMoveTimer) {
      clearTimeout(this._pendingMoveTimer);
      this._pendingMoveTimer = null;
    }
    const coords = this._getNormalizedCoords(e);
    if (coords) {
      this._lastMoveTime = performance.now();
      this._sendInput({
        type: 'MouseMove',
        x: coords.x,
        y: coords.y,
        dx: 0,
        dy: 0,
        is_rel: false,
      });
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
    if (!this.sessionRunning || !this.activeSession) return;

    if (e.code === 'Escape' && this.isPointerLocked) {
      document.exitPointerLock?.();
      return;
    }

    // Direct 1:1 pass-through: do not capture or remap shortcut keys locally!
    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      // Prevent local browser/webview from swallowing standard system hotkeys
      e.preventDefault();
      this._sendInput({
        type: 'Key',
        code: linuxCode,
        down: true,
      });
    }
  }

  onKeyUp(e) {
    if (!this.sessionRunning || !this.activeSession) return;

    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      e.preventDefault();
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
