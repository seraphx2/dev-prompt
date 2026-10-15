// Frames -> animated GIF, for the README's demo (scene 0 in capture.mjs).
//
// Frames are screenshots of the page taken as a scripted session plays. Each
// is drawn centred on a fixed canvas the size of the overlay with a shell open,
// so the panel visibly grows when one opens, as it does on screen. Identical
// consecutive frames merge into one longer frame, and the whole GIF shares one
// palette, which keeps the file small and the colours steady between frames.

import { writeFile } from "node:fs/promises";
import gifenc from "gifenc"; // CommonJS: no named exports under Node's ESM
import { PNG } from "pngjs";

const { GIFEncoder, applyPalette, quantize } = gifenc;

export class GifRecorder {
  /**
   * @param {import("playwright").Page} page
   * @param {{ width: number, height: number }} canvas
   * @param {[number, number, number]} background  behind the panel, and in
   *   the margin around a smaller screen
   */
  constructor(page, canvas, background) {
    this.page = page;
    this.width = canvas.width;
    this.height = canvas.height;
    this.background = background;
    /** @type {{ rgba: Uint8Array, delay: number }[]} */
    this.frames = [];
  }

  /** Paint the page background, so the translucent panel and its rounded
   *  corners sit on the same colour as the canvas margin. */
  async paintBackground() {
    const [r, g, b] = this.background;
    await this.page.addStyleTag({
      content: `html, body { background: rgb(${r} ${g} ${b}) !important; }`,
    });
  }

  /** Capture the page as it is now and show it for `delay` ms. */
  async snap(delay) {
    const png = PNG.sync.read(await this.page.screenshot({ type: "png", caret: "hide" }));
    const rgba = this.compose(png);
    const last = this.frames.at(-1);
    if (last && equal(last.rgba, rgba)) last.delay += delay;
    else this.frames.push({ rgba, delay });
  }

  /** Keep capturing for `ms` while something animates (shell output
   *  streaming in), a frame every `every` ms. */
  async record(ms, every = 100) {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      const started = Date.now();
      await this.snap(every);
      await this.page.waitForTimeout(Math.max(0, every - (Date.now() - started)));
    }
  }

  /** `png` centred on the canvas. */
  compose(png) {
    const out = new Uint8Array(this.width * this.height * 4);
    const [r, g, b] = this.background;
    for (let i = 0; i < out.length; i += 4) {
      out[i] = r;
      out[i + 1] = g;
      out[i + 2] = b;
      out[i + 3] = 255;
    }
    const w = Math.min(png.width, this.width);
    const h = Math.min(png.height, this.height);
    const x0 = Math.floor((this.width - w) / 2);
    const y0 = Math.floor((this.height - h) / 2);
    for (let y = 0; y < h; y++) {
      const src = y * png.width * 4;
      out.set(png.data.subarray(src, src + w * 4), ((y0 + y) * this.width + x0) * 4);
    }
    return out;
  }

  async save(path) {
    if (!this.frames.length) throw new Error("no frames captured");
    // One palette for the whole GIF, from a sample of every frame's pixels.
    const step = 4 * 7; // every 7th pixel
    const sample = [];
    for (const f of this.frames) {
      for (let i = 0; i < f.rgba.length; i += step) sample.push(f.rgba[i], f.rgba[i + 1], f.rgba[i + 2], 255);
    }
    const palette = quantize(new Uint8Array(sample), 256);
    const gif = GIFEncoder();
    this.frames.forEach((f, i) => {
      gif.writeFrame(applyPalette(f.rgba, palette), this.width, this.height, {
        palette: i === 0 ? palette : undefined,
        delay: f.delay,
        repeat: 0, // loop forever
      });
    });
    gif.finish();
    await writeFile(path, gif.bytes());
    const seconds = this.frames.reduce((n, f) => n + f.delay, 0) / 1000;
    return { frames: this.frames.length, seconds };
  }
}

function equal(a, b) {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
