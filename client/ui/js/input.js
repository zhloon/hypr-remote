import { WEB_TO_LINUX_KEYMAP } from './keycodes.js';

export class InputManager {
  /**
   * @param {HTMLVideoElement} videoElement
   * @param {(msg: object) => void} sendCallback
   */
  constructor(videoElement, sendCallback) {
    this.video = videoElement;
    this.send = sendCallback;
    this.isPointerLocked = false;
    this.isActive = false;

    this.onMouseMove = this.onMouseMove.bind(this);
    this.onMouseDown = this.onMouseDown.bind(this);
    this.onMouseUp = this.onMouseUp.bind(this);
    this.onWheel = this.onWheel.bind(this);
    this.onKeyDown = this.onKeyDown.bind(this);
    this.onKeyUp = this.onKeyUp.bind(this);
    this.onPointerLockChange = this.onPointerLockChange.bind(this);
    this.onContextMenu = (e) => e.preventDefault();
  }

  attach() {
    this.isActive = true;
    const v = this.video;

    v.addEventListener('mousemove', this.onMouseMove);
    v.addEventListener('mousedown', this.onMouseDown);
    v.addEventListener('mouseup', this.onMouseUp);
    v.addEventListener('wheel', this.onWheel, { passive: false });
    v.addEventListener('contextmenu', this.onContextMenu);

    window.addEventListener('keydown', this.onKeyDown);
    window.addEventListener('keyup', this.onKeyUp);
    document.addEventListener('pointerlockchange', this.onPointerLockChange);
  }

  detach() {
    this.isActive = false;
    const v = this.video;

    v.removeEventListener('mousemove', this.onMouseMove);
    v.removeEventListener('mousedown', this.onMouseDown);
    v.removeEventListener('mouseup', this.onMouseUp);
    v.removeEventListener('wheel', this.onWheel);
    v.removeEventListener('contextmenu', this.onContextMenu);

    window.removeEventListener('keydown', this.onKeyDown);
    window.removeEventListener('keyup', this.onKeyUp);
    document.removeEventListener('pointerlockchange', this.onPointerLockChange);

    if (this.isPointerLocked) {
      document.exitPointerLock();
    }
  }

  requestPointerLock() {
    if (this.video.requestPointerLock) {
      this.video.requestPointerLock();
    }
  }

  releasePointerLock() {
    if (document.exitPointerLock) {
      document.exitPointerLock();
    }
  }

  onPointerLockChange() {
    this.isPointerLocked = (document.pointerLockElement === this.video);
    const event = new CustomEvent('pointerlock-changed', {
      detail: { locked: this.isPointerLocked }
    });
    window.dispatchEvent(event);
  }

  onMouseMove(e) {
    if (!this.isActive) return;

    if (this.isPointerLocked) {
      // Relative motion mode (FPS / Hyprland workspace mode)
      this.send({
        t: 'm_rel',
        dx: e.movementX,
        dy: e.movementY
      });
    } else {
      // Normalized absolute coordinates [0.0 - 1.0]
      const rect = this.video.getBoundingClientRect();
      const x = (e.clientX - rect.left) / rect.width;
      const y = (e.clientY - rect.top) / rect.height;

      if (x >= 0 && x <= 1 && y >= 0 && y <= 1) {
        this.send({
          t: 'm_abs',
          x: parseFloat(x.toFixed(5)),
          y: parseFloat(y.toFixed(5))
        });
      }
    }
  }

  onMouseDown(e) {
    if (!this.isActive) return;
    this.send({
      t: 'm_btn',
      btn: e.button, // 0: Left, 1: Middle, 2: Right
      down: true
    });
  }

  onMouseUp(e) {
    if (!this.isActive) return;
    this.send({
      t: 'm_btn',
      btn: e.button,
      down: false
    });
  }

  onWheel(e) {
    if (!this.isActive) return;
    e.preventDefault();
    this.send({
      t: 'm_wheel',
      dx: Math.round(e.deltaX),
      dy: Math.round(e.deltaY)
    });
  }

  onKeyDown(e) {
    if (!this.isActive) return;

    // Emergency escape combo: Ctrl + Shift + Alt + Escape to release pointer lock
    if (e.ctrlKey && e.shiftKey && e.altKey && e.code === 'Escape') {
      this.releasePointerLock();
      return;
    }

    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      // Prevent browser from capturing default system hotkeys if possible
      if (['Tab', 'AltLeft', 'AltRight', 'MetaLeft', 'MetaRight', 'F1', 'F11'].includes(e.code)) {
        e.preventDefault();
      }

      this.send({
        t: 'key',
        code: linuxCode,
        down: true
      });
    }
  }

  onKeyUp(e) {
    if (!this.isActive) return;
    const linuxCode = WEB_TO_LINUX_KEYMAP[e.code];
    if (linuxCode !== undefined) {
      this.send({
        t: 'key',
        code: linuxCode,
        down: false
      });
    }
  }
}
