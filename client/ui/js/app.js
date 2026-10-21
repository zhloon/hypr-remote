import { WEB_TO_LINUX_KEYMAP } from './keycodes.js';

class HyprRemoteUnifiedApp {
  constructor() {
    this.isTauri = typeof window.__TAURI__ !== 'undefined';
    this.myInfo = null;
    this.activeSession = null;
    this.isPointerLocked = false;
    this.isFullscreen = false;
    this.lastInputTime = 0;

    // DOM Elements
    this.dom = {
      homeView: document.getElementById('home-view'),
      sessionView: document.getElementById('session-view'),
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
      remoteImg: document.getElementById('remote-screen-img'),
      sessionTitle: document.getElementById('session-target-title'),
      btnDisconnect: document.getElementById('btn-disconnect'),
      btnToggleFs: document.getElementById('btn-toggle-fs'),
      btnToggleLock: document.getElementById('btn-toggle-lock'),
    };

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

    document.addEventListener('pointerlockchange', () => {
      this.isPointerLocked = document.pointerLockElement === this.dom.remoteImg;
      this.dom.btnToggleLock.classList.toggle('active', this.isPointerLocked);
      this.dom.btnToggleLock.textContent = this.isPointerLocked ? '释放光标 (Esc)' : '锁定光标';
    });

    // Click on remote screen activates pointer lock
    this.dom.remoteImg.addEventListener('click', () => {
      if (this.activeSession && !this.isPointerLocked) {
        this.dom.remoteImg.requestPointerLock?.();
      }
    });

    this.dom.remoteImg.addEventListener('contextmenu', (e) => e.preventDefault());
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
      alert('请输入对方的 6 位连接码');
      return;
    }

    this.dom.btnConnect.disabled = true;
    this.dom.btnConnect.textContent = '⏳ 正在寻址连接...';

    try {
      let res;
      if (this.isTauri) {
        res = await window.__TAURI__.core.invoke('resolve_connection_code', { code });
      } else {
        // Mock fallback
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

      // Switch to remote screen
      this.dom.homeView.style.display = 'none';
      this.dom.sessionView.style.display = 'flex';
      this.dom.sessionTitle.textContent = `${code} (${this.activeSession.ip})`;

      // Load MJPEG screen stream directly into <img>
      this.dom.remoteImg.src = res.stream_url + '?t=' + Date.now();

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

  disconnect() {
    this._detachInput();
    this.dom.remoteImg.src = '';
    this.activeSession = null;
    this.dom.sessionView.style.display = 'none';
    this.dom.homeView.style.display = 'flex';
  }

  toggleFullscreen() {
    this.isFullscreen = !this.isFullscreen;
    if (this.isTauri) {
      window.__TAURI__.core.invoke('toggle_fullscreen', { fullscreen: this.isFullscreen });
    } else {
      if (this.isFullscreen) document.documentElement.requestFullscreen?.();
      else document.exitFullscreen?.();
    }
  }

  togglePointerLock() {
    if (this.isPointerLocked) {
      document.exitPointerLock?.();
    } else {
      this.dom.remoteImg.requestPointerLock?.();
    }
  }

  // ==========================================
  // Input Handling
  // ==========================================
  _attachInput() {
    const img = this.dom.remoteImg;
    img.addEventListener('mousemove', this.onMouseMove);
    img.addEventListener('mousedown', this.onMouseDown);
    img.addEventListener('mouseup', this.onMouseUp);
    img.addEventListener('wheel', this.onWheel, { passive: false });

    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
  }

  _detachInput() {
    const img = this.dom.remoteImg;
    img.removeEventListener('mousemove', this.onMouseMove);
    img.removeEventListener('mousedown', this.onMouseDown);
    img.removeEventListener('mouseup', this.onMouseUp);
    img.removeEventListener('wheel', this.onWheel);

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
    const img = this.dom.remoteImg;
    const rect = img.getBoundingClientRect();
    const nw = img.naturalWidth || 1920;
    const nh = img.naturalHeight || 1080;
    const rectRatio = rect.width / rect.height;
    const imgRatio = nw / nh;

    let rw = rect.width;
    let rh = rect.height;
    let ox = 0;
    let oy = 0;

    if (rectRatio > imgRatio) {
      rw = rect.height * imgRatio;
      ox = (rect.width - rw) / 2;
    } else {
      rh = rect.width / imgRatio;
      oy = (rect.height - rh) / 2;
    }

    const cx = e.clientX - rect.left - ox;
    const cy = e.clientY - rect.top - oy;

    if (cx < 0 || cx > rw || cy < 0 || cy > rh) {
      return null;
    }

    return {
      x: parseFloat(Math.max(0, Math.min(1, cx / rw)).toFixed(4)),
      y: parseFloat(Math.max(0, Math.min(1, cy / rh)).toFixed(4))
    };
  }

  onMouseMove(e) {
    const now = performance.now();
    if (now - this.lastInputTime < 16) return;
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
    if (e.ctrlKey && e.shiftKey && e.altKey && e.code === 'Escape') {
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
