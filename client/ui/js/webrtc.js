export class WebRTCClient {
  constructor(options = {}) {
    this.options = {
      fps: 60,
      bitrateKbps: 25000,
      ...options
    };

    this.ws = null;
    this.pc = null;
    this.dataChannel = null;
    this.statsInterval = null;
    this.pingInterval = null;

    this.onTrack = null;
    this.onStats = null;
    this.onStateChange = null;
    this.onClipboard = null;

    this.lastBytesReceived = 0;
    this.lastStatsTime = 0;
  }

  async connect(hostIp, port = 8080) {
    this._setState('connecting');
    const wsUrl = `ws://${hostIp}:${port}/ws`;

    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(wsUrl);
      } catch (err) {
        this._setState('failed');
        return reject(err);
      }

      this.ws.onopen = async () => {
        try {
          await this._initPeerConnection();
          // Send hello handshake
          this._wsSend({
            type: 'hello',
            client_name: 'HyprRemote Client',
            version: '1.0.0',
            requested_fps: this.options.fps,
            requested_bitrate_kbps: this.options.bitrateKbps
          });
          resolve();
        } catch (e) {
          this._setState('failed');
          reject(e);
        }
      };

      this.ws.onmessage = async (event) => {
        try {
          const msg = JSON.parse(event.data);
          await this._handleSignalingMessage(msg);
        } catch (e) {
          console.error('[Signaling] Failed to parse message:', e);
        }
      };

      this.ws.onerror = (e) => {
        console.error('[Signaling] WebSocket error:', e);
        this._setState('failed');
      };

      this.ws.onclose = () => {
        this._cleanup();
        this._setState('disconnected');
      };
    });
  }

  disconnect() {
    this._cleanup();
    if (this.ws) {
      this.ws.close();
      this.ws = null;
    }
    this._setState('disconnected');
  }

  sendControl(msg) {
    if (this.dataChannel && this.dataChannel.readyState === 'open') {
      this.dataChannel.send(JSON.stringify(msg));
    }
  }

  async _initPeerConnection() {
    // Ultra-low latency LAN WebRTC configuration
    const config = {
      iceServers: [], // LAN direct mode doesn't need external STUN/TURN
      iceTransportPolicy: 'all',
      bundlePolicy: 'max-bundle',
      rtcpMuxPolicy: 'require'
    };

    this.pc = new RTCPeerConnection(config);

    // Setup DataChannel for input events
    this.dataChannel = this.pc.createDataChannel('control', {
      ordered: false,
      maxRetransmits: 0 // Prioritize low-latency over guaranteed delivery for mouse
    });

    this.dataChannel.onopen = () => {
      console.log('[DataChannel] Control channel opened');
      this._startPing();
    };

    this.dataChannel.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.t === 'pong') {
          const rtt = Date.now() - msg.client_ts;
          if (this.onStats) {
            this.onStats({ rtt });
          }
        } else if (msg.t === 'clip' && this.onClipboard) {
          this.onClipboard(msg.text);
        }
      } catch (e) {
        console.error('[DataChannel] Parse error:', e);
      }
    };

    // Receive media tracks from Hyprland
    this.pc.ontrack = (event) => {
      if (this.onTrack && event.streams && event.streams[0]) {
        this.onTrack(event.streams[0]);
      }
    };

    this.pc.onicecandidate = (event) => {
      if (event.candidate) {
        this._wsSend({
          type: 'ice_candidate',
          candidate: event.candidate
        });
      }
    };

    this.pc.onconnectionstatechange = () => {
      const state = this.pc.connectionState;
      if (state === 'connected') {
        this._setState('connected');
        this._startStatsMonitoring();
      } else if (state === 'disconnected' || state === 'failed') {
        this._setState(state);
      }
    };

    // Add receive-only transceiver for Video & Audio
    this.pc.addTransceiver('video', { direction: 'recvonly' });
    this.pc.addTransceiver('audio', { direction: 'recvonly' });

    // Create Offer
    const offer = await this.pc.createOffer({
      offerToReceiveVideo: true,
      offerToReceiveAudio: true
    });
    await this.pc.setLocalDescription(offer);

    this._wsSend({
      type: 'sdp_offer',
      sdp: offer.sdp
    });
  }

  async _handleSignalingMessage(msg) {
    if (msg.type === 'sdp_answer') {
      await this.pc.setRemoteDescription(new RTCSessionDescription({
        type: 'answer',
        sdp: msg.sdp
      }));
    } else if (msg.type === 'ice_candidate') {
      if (msg.candidate) {
        await this.pc.addIceCandidate(new RTCIceCandidate(msg.candidate));
      }
    }
  }

  _wsSend(payload) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(payload));
    }
  }

  _startPing() {
    if (this.pingInterval) clearInterval(this.pingInterval);
    this.pingInterval = setInterval(() => {
      this.sendControl({
        t: 'ping',
        ts: Date.now()
      });
    }, 1000);
  }

  _startStatsMonitoring() {
    if (this.statsInterval) clearInterval(this.statsInterval);
    this.lastBytesReceived = 0;
    this.lastStatsTime = performance.now();

    this.statsInterval = setInterval(async () => {
      if (!this.pc || this.pc.connectionState !== 'connected') return;

      const stats = await this.pc.getStats();
      let fps = 0;
      let bitrateKbps = 0;
      let packetsLost = 0;

      stats.forEach((report) => {
        if (report.type === 'inbound-rtp' && report.kind === 'video') {
          fps = report.framesPerSecond || 0;
          packetsLost = report.packetsLost || 0;

          const now = performance.now();
          const bytes = report.bytesReceived || 0;

          if (this.lastBytesReceived > 0 && this.lastStatsTime > 0) {
            const timeDiffSec = (now - this.lastStatsTime) / 1000;
            const bytesDiff = bytes - this.lastBytesReceived;
            bitrateKbps = Math.round((bytesDiff * 8) / (timeDiffSec * 1000));
          }

          this.lastBytesReceived = bytes;
          this.lastStatsTime = now;
        }
      });

      if (this.onStats) {
        this.onStats({
          fps: Math.round(fps),
          bitrateKbps: Math.max(0, bitrateKbps),
          packetsLost
        });
      }
    }, 1000);
  }

  _cleanup() {
    if (this.statsInterval) clearInterval(this.statsInterval);
    if (this.pingInterval) clearInterval(this.pingInterval);
    this.statsInterval = null;
    this.pingInterval = null;

    if (this.dataChannel) {
      this.dataChannel.close();
      this.dataChannel = null;
    }
    if (this.pc) {
      this.pc.close();
      this.pc = null;
    }
  }

  _setState(state) {
    if (this.onStateChange) {
      this.onStateChange(state);
    }
  }
}
