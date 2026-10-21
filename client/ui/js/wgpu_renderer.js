/**
 * WgpuRenderer: High-Performance WebGPU / wgpu Hardware Accelerated Canvas Engine.
 * Features zero-copy GPU texture uploads, WGSL bilinear shaders, and seamless WebGL2 fallback.
 */
export class WgpuRenderer {
  constructor(canvas) {
    this.canvas = canvas;
    this.backend = 'none'; // 'WebGPU' | 'WebGL2' | 'Canvas2D'
    this.device = null;
    this.context = null;
    this.pipeline = null;
    this.sampler = null;
    this.bindGroupLayout = null;
    this.currentTexture = null;
    this.currentBindGroup = null;
    this.lastWidth = 0;
    this.lastHeight = 0;

    // WebGL fallback state
    this.gl = null;
    this.glProgram = null;
    this.glTexture = null;

    // 2D fallback state
    this.ctx2d = null;
  }

  async init() {
    // 1. Try modern WebGPU (wgpu standard in WebKit / Safari / Chromium)
    if (navigator.gpu) {
      try {
        const adapter = await navigator.gpu.requestAdapter({
          powerPreference: 'high-performance',
        });
        if (adapter) {
          this.device = await adapter.requestDevice();
          this.context = this.canvas.getContext('webgpu');
          const canvasFormat = navigator.gpu.getPreferredCanvasFormat();
          this.context.configure({
            device: this.device,
            format: canvasFormat,
            alphaMode: 'opaque',
          });

          // WGSL High-Performance Bilinear Texture Shader
          const shaderCode = `
            struct VertexOutput {
              @builtin(position) position: vec4f,
              @location(0) uv: vec2f,
            };

            @vertex
            fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
              var pos = array<vec2f, 6>(
                vec2f(-1.0, -1.0), vec2f( 1.0, -1.0), vec2f(-1.0,  1.0),
                vec2f(-1.0,  1.0), vec2f( 1.0, -1.0), vec2f( 1.0,  1.0)
              );
              var uvs = array<vec2f, 6>(
                vec2f(0.0, 1.0), vec2f(1.0, 1.0), vec2f(0.0, 0.0),
                vec2f(0.0, 0.0), vec2f(1.0, 1.0), vec2f(1.0, 0.0)
              );
              var out: VertexOutput;
              out.position = vec4f(pos[index], 0.0, 1.0);
              out.uv = uvs[index];
              return out;
            }

            @group(0) @binding(0) var screenSampler: sampler;
            @group(0) @binding(1) var screenTexture: texture_2d<f32>;

            @fragment
            fn fs_main(in: VertexOutput) -> @location(0) vec4f {
              return textureSample(screenTexture, screenSampler, in.uv);
            }
          `;

          const shaderModule = this.device.createShaderModule({ code: shaderCode });

          this.sampler = this.device.createSampler({
            magFilter: 'linear',
            minFilter: 'linear',
            addressModeU: 'clamp-to-edge',
            addressModeV: 'clamp-to-edge',
          });

          this.bindGroupLayout = this.device.createBindGroupLayout({
            entries: [
              {
                binding: 0,
                visibility: GPUShaderStage.FRAGMENT,
                sampler: { type: 'filtering' },
              },
              {
                binding: 1,
                visibility: GPUShaderStage.FRAGMENT,
                texture: { sampleType: 'float' },
              },
            ],
          });

          const pipelineLayout = this.device.createPipelineLayout({
            bindGroupLayouts: [this.bindGroupLayout],
          });

          this.pipeline = this.device.createRenderPipeline({
            layout: pipelineLayout,
            vertex: {
              module: shaderModule,
              entryPoint: 'vs_main',
            },
            fragment: {
              module: shaderModule,
              entryPoint: 'fs_main',
              targets: [{ format: canvasFormat }],
            },
            primitive: {
              topology: 'triangle-list',
            },
          });

          this.backend = 'WebGPU';
          console.log('[WgpuRenderer] WebGPU hardware acceleration engine active!');
          return true;
        }
      } catch (err) {
        console.warn('[WgpuRenderer] WebGPU init failed, trying WebGL2:', err);
      }
    }

    // 2. Hardware Accelerated WebGL2 Fallback
    try {
      const gl =
        this.canvas.getContext('webgl2', {
          alpha: false,
          depth: false,
          stencil: false,
          antialias: false,
          desynchronized: true,
        }) ||
        this.canvas.getContext('webgl', {
          alpha: false,
          depth: false,
          stencil: false,
          antialias: false,
        });

      if (gl) {
        this.gl = gl;
        const vsSource = `
          attribute vec2 a_pos;
          attribute vec2 a_uv;
          varying vec2 v_uv;
          void main() {
            gl_Position = vec4(a_pos, 0.0, 1.0);
            v_uv = a_uv;
          }
        `;
        const fsSource = `
          precision mediump float;
          uniform sampler2D u_image;
          varying vec2 v_uv;
          void main() {
            gl_FragColor = texture2D(u_image, v_uv);
          }
        `;

        const createShader = (type, src) => {
          const s = gl.createShader(type);
          gl.shaderSource(s, src);
          gl.compileShader(s);
          return s;
        };

        const prog = gl.createProgram();
        gl.attachShader(prog, createShader(gl.VERTEX_SHADER, vsSource));
        gl.attachShader(prog, createShader(gl.FRAGMENT_SHADER, fsSource));
        gl.linkProgram(prog);
        gl.useProgram(prog);
        this.glProgram = prog;

        // Quad buffer
        const buffer = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
        gl.bufferData(
          gl.ARRAY_BUFFER,
          new Float32Array([
            -1, -1, 0, 1,
             1, -1, 1, 1,
            -1,  1, 0, 0,
             1,  1, 1, 0,
          ]),
          gl.STATIC_DRAW
        );

        const aPos = gl.getAttribLocation(prog, 'a_pos');
        const aUv = gl.getAttribLocation(prog, 'a_uv');
        gl.enableVertexAttribArray(aPos);
        gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 16, 0);
        gl.enableVertexAttribArray(aUv);
        gl.vertexAttribPointer(aUv, 2, gl.FLOAT, false, 16, 8);

        this.glTexture = gl.createTexture();
        gl.bindTexture(gl.TEXTURE_2D, this.glTexture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);

        this.backend = 'WebGL2';
        console.log('[WgpuRenderer] WebGL2 hardware acceleration engine active!');
        return true;
      }
    } catch (e) {
      console.warn('[WgpuRenderer] WebGL init failed:', e);
    }

    // 3. Fallback to 2D Canvas
    this.ctx2d = this.canvas.getContext('2d', { alpha: false });
    this.backend = 'Canvas2D';
    console.log('[WgpuRenderer] Using 2D Canvas fallback.');
    return true;
  }

  render(imageSource, width, height) {
    if (!imageSource) return;

    if (this.backend === 'WebGPU' && this.device && this.context) {
      try {
        if (!this.currentTexture || this.lastWidth !== width || this.lastHeight !== height) {
          if (this.currentTexture) this.currentTexture.destroy();
          this.currentTexture = this.device.createTexture({
            size: [width, height, 1],
            format: 'rgba8unorm',
            usage:
              GPUTextureUsage.TEXTURE_BINDING |
              GPUTextureUsage.COPY_DST |
              GPUTextureUsage.RENDER_ATTACHMENT,
          });

          this.currentBindGroup = this.device.createBindGroup({
            layout: this.bindGroupLayout,
            entries: [
              { binding: 0, resource: this.sampler },
              { binding: 1, resource: this.currentTexture.createView() },
            ],
          });
          this.lastWidth = width;
          this.lastHeight = height;
        }

        // Zero-copy GPU upload from ImageBitmap / Video
        this.device.queue.copyExternalImageToTexture(
          { source: imageSource },
          { texture: this.currentTexture },
          [width, height]
        );

        const commandEncoder = this.device.createCommandEncoder();
        const textureView = this.context.getCurrentTexture().createView();
        const renderPass = commandEncoder.beginRenderPass({
          colorAttachments: [
            {
              view: textureView,
              loadOp: 'clear',
              storeOp: 'store',
            },
          ],
        });

        renderPass.setPipeline(this.pipeline);
        renderPass.setBindGroup(0, this.currentBindGroup);
        renderPass.draw(6);
        renderPass.end();

        this.device.queue.submit([commandEncoder.finish()]);
        return;
      } catch (err) {
        console.warn('[WgpuRenderer] WebGPU render pass failed:', err);
      }
    }

    if (this.backend === 'WebGL2' && this.gl) {
      const gl = this.gl;
      gl.viewport(0, 0, this.canvas.width, this.canvas.height);
      gl.bindTexture(gl.TEXTURE_2D, this.glTexture);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, imageSource);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      return;
    }

    if (this.ctx2d) {
      this.ctx2d.drawImage(imageSource, 0, 0, width, height);
    }
  }

  getBackendName() {
    return this.backend;
  }
}
