// Regenerate the README / AppStream screenshots and the demo GIF in docs/img/.
//
//   npm run screenshots                 # all of them, stills at 2x
//   npm run screenshots -- --scale 1    # stills at the overlay's native pixel size
//   npm run screenshots -- --only 4,5   # just some (0 is the GIF)
//
// The real frontend runs in headless Chromium against a fake backend
// (shots.ts + demo-data.ts), so every image shows the same repos, apps and
// shell output, whatever machine it's taken on. Each scene drives the app
// with the same keys a person would press, then captures <main> at exactly the
// overlay's size: 760×480, or 1000×640 while a shell is open. No desktop shows
// through — the panel sits on a transparent background.
//
// Fonts come from the machine running it (the app uses the system UI font and
// a monospace stack), so take them on Linux to match the AppStream shots —
// from Windows, `npm run screenshots:wsl` does that through WSL (wsl.sh).

import { execFileSync } from "node:child_process";
import { mkdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import { createServer } from "vite";
import { GifRecorder } from "./gif.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const outDir = join(root, "docs", "img");

const SMALL = { width: 760, height: 480 };
const LARGE = { width: 1000, height: 640 };
// Linux, so the app picks its Linux look and wording ("Trash", the panel's
// near-opaque fill) whatever OS takes the shots.
const UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36";

const argv = process.argv.slice(2);
const opt = (name) => {
  const i = argv.indexOf(`--${name}`);
  return i === -1 ? undefined : argv[i + 1];
};
const scale = Number(opt("scale") ?? 2);
const only = opt("only")?.split(",").map(Number);

// --- helpers -------------------------------------------------------------------

/** Search for a repo and open its action menu. */
async function openRepo(page, name) {
  await page.keyboard.type(name);
  await page.getByRole("button", { name: new RegExp(`^${name}`) }).first().waitFor();
  await page.keyboard.press("Enter");
  await page.getByText("Open shell").first().waitFor();
}

/** From the repo list: open `name`'s shell and wait for its output. */
async function openShell(page, name) {
  await openRepo(page, name);
  await page.keyboard.press("Control+R");
  await page.locator(".xterm-rows").getByText("❯").first().waitFor();
}

/** From a shell back to an empty repo list, the shell left running. */
async function backToList(page) {
  await page.keyboard.press("Shift+Escape"); // shell -> action menu
  await page.getByText("Open shell").first().waitFor();
  await page.keyboard.press("Escape"); // action menu -> list
  await page.keyboard.press("Delete"); // clear the search
  await page.getByText("alacritty").first().waitFor();
}

// --- scenes --------------------------------------------------------------------

const scenes = [
  {
    n: 1,
    file: "1_dev-prompt_repos.png",
    // One shell left running: the row's terminal icon and the footer count.
    async run(page) {
      await openShell(page, "hugo");
      await backToList(page);
    },
  },
  {
    n: 2,
    file: "2_dev-prompt_actions.png",
    async run(page) {
      await openRepo(page, "prettier");
      await page.getByText("yarn test").first().waitFor();
    },
  },
  {
    n: 3,
    file: "3_dev-prompt_apps.png",
    async run(page) {
      await page.keyboard.type(">");
      await page.getByText("Firefox").first().waitFor();
    },
  },
  {
    n: 4,
    file: "4_dev-prompt_shell.png",
    async run(page) {
      await openShell(page, "hugo");
      await page.locator(".xterm-rows").getByText("Press").waitFor();
    },
  },
  {
    n: 5,
    file: "5_dev-prompt_shells.png",
    async run(page) {
      for (const name of ["hugo", "prettier", "ripgrep"]) {
        await openShell(page, name);
        await backToList(page);
      }
      await page.keyboard.type("$");
      await page.getByText("Close all").first().waitFor();
    },
  },
  {
    n: 6,
    file: "6_dev-prompt_new-repo.png",
    async run(page) {
      await page.keyboard.press("Control+Shift+N");
      await page.getByText("New repository").first().waitFor();
      await page.keyboard.type("weather-cli");
    },
  },
  {
    n: 7,
    file: "7_dev-prompt_remove.png",
    async run(page) {
      await openRepo(page, "lazygit");
      await page.keyboard.type("trash");
      await page.keyboard.press("Enter");
      await page.getByText("uncommitted").first().waitFor();
    },
  },
  {
    n: 8,
    file: "8_dev-prompt_attention.png",
    // The `!` scope: only repos with work left only here or half-done.
    async run(page) {
      await page.keyboard.type("!");
      await page.getByText("rebasing").first().waitFor();
    },
  },
  {
    n: 10,
    file: "10_dev-prompt_scripts.png",
    // The same menu scrolled down to what the project itself offers — the
    // way you'd find the scripts by moving down to them.
    async run(page) {
      await openRepo(page, "prettier");
      const first = page.getByText("yarn test").first();
      await first.waitFor();
      await first.scrollIntoViewIfNeeded();
      await first.hover();
    },
  },
  {
    n: 9,
    file: "9_dev-prompt_branches.png",
    async run(page) {
      await openRepo(page, "lazygit");
      await page.keyboard.press("Control+B");
      await page.getByText("perf/commit-graph").first().waitFor();
    },
  },
];

// --- the demo GIF --------------------------------------------------------------

const GIF_FILE = "0_dev-prompt_demo.gif";
const MP4_FILE = "0_dev-prompt_demo.mp4";

/** The demo as an MP4 too, for places that take video but not GIFs (some
 *  subreddits, social posts). Needs ffmpeg; skipped with a note when it isn't
 *  installed — the GIF itself never needs it. Two passes, since a video
 *  player won't loop it the way a GIF does. */
function gifToMp4(gif, mp4) {
  try {
    execFileSync("ffmpeg", ["-version"], { stdio: "ignore" });
  } catch {
    console.log(`${MP4_FILE}  skipped: ffmpeg isn't installed`);
    return;
  }
  execFileSync("ffmpeg", [
    "-loglevel", "error", "-y", "-stream_loop", "1", "-i", gif,
    "-movflags", "+faststart", "-pix_fmt", "yuv420p", "-c:v", "libx264",
    "-crf", "18", "-r", "30", mp4,
  ]);
  console.log(`${MP4_FILE}  ${LARGE.width}×${LARGE.height}, two passes`);
}
const GIF_BACKGROUND = [14, 14, 17];

/** Type `text` a key at a time, a frame after each, like someone typing. */
async function typeSlowly(page, gif, text) {
  for (const ch of text) {
    await page.keyboard.type(ch);
    await page.waitForTimeout(40);
    await gif.snap(130);
  }
}

/** Hotkey's up: search, the action menu, a shell (the overlay grows and the
 *  output streams in), back out leaving it running, the `$` list, and
 *  straight back into the shell. About 16 seconds, looping. */
async function demo(page, gif) {
  await gif.snap(1200); // the repo list, as the hotkey brings it up
  await typeSlowly(page, gif, "hu");
  await page.getByRole("button", { name: /^hugo/ }).first().waitFor();
  await gif.snap(700);

  await page.keyboard.press("Enter");
  await page.getByText("yarn test").or(page.getByText("go build")).first().waitFor();
  await page.waitForTimeout(150);
  await gif.snap(1500); // the action menu

  await page.keyboard.press("Control+R");
  await page.locator(".xterm-rows").getByText("❯").first().waitFor();
  await gif.record(2000); // output streaming in
  await page.locator(".xterm-rows").getByText("Press").waitFor();
  await gif.snap(2200);

  await page.keyboard.press("Shift+Escape"); // back to the menu; the shell runs on
  await page.getByText("Open shell").first().waitFor();
  await page.waitForTimeout(150);
  await gif.snap(800);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Delete");
  await page.getByText("alacritty").first().waitFor();
  await page.waitForTimeout(150);
  await gif.snap(1500); // the list, hugo marked as running a shell

  await typeSlowly(page, gif, "$");
  await page.getByText("Close all").first().waitFor();
  await page.waitForTimeout(150);
  await gif.snap(1600);

  await page.keyboard.press("Enter"); // straight back into it
  await page.locator(".xterm-rows").getByText("Press").waitFor();
  await page.waitForTimeout(250);
  await gif.snap(2200);
}

// --- run -----------------------------------------------------------------------

const server = await createServer({
  root,
  logLevel: "warn",
  // Its own port, so a running `tauri dev` (1420) can stay up.
  server: { port: 1430, strictPort: false, hmr: false },
});
await server.listen();
const url = new URL("scripts/screenshots/shots.html", server.resolvedUrls.local[0]).href;

const browser = await chromium.launch();
await mkdir(outDir, { recursive: true });
let failed = 0;

/** A fresh app on the repo list. */
async function openApp(label, { pixelRatio, streamSlowly = false }) {
  const context = await browser.newContext({
    viewport: SMALL,
    deviceScaleFactor: pixelRatio,
    userAgent: UA,
    colorScheme: "dark",
  });
  const page = await context.newPage();
  page.on("pageerror", (e) => console.error(`  [${label}] page error: ${e.message}`));
  // The app asks for a bigger window while a shell is open; this is it.
  await page.exposeFunction("shotsResize", (large) =>
    page.setViewportSize(large ? LARGE : SMALL),
  );
  if (streamSlowly) await page.addInitScript(() => (window.shotsStreamSlowly = true));
  await page.goto(url);
  await page.getByText("alacritty").first().waitFor();
  await page.evaluate(() => document.fonts.ready);
  return { context, page };
}

try {
  // The GIF is drawn at 1x whatever --scale says: 2x would quadruple its size
  // for detail a moving README banner doesn't need.
  if (!only || only.includes(0)) {
    const { context, page } = await openApp(GIF_FILE, { pixelRatio: 1, streamSlowly: true });
    try {
      const gif = new GifRecorder(page, LARGE, GIF_BACKGROUND);
      await gif.paintBackground();
      await demo(page, gif);
      const { frames, seconds } = await gif.save(join(outDir, GIF_FILE));
      console.log(`${GIF_FILE}  ${LARGE.width}×${LARGE.height}, ${frames} frames, ${seconds.toFixed(1)} s`);
      gifToMp4(join(outDir, GIF_FILE), join(outDir, MP4_FILE));
    } catch (e) {
      failed++;
      console.error(`${GIF_FILE}  FAILED: ${e.message.split("\n")[0]}`);
    } finally {
      await context.close();
    }
  }

  for (const scene of scenes) {
    if (only && !only.includes(scene.n)) continue;
    const { context, page } = await openApp(scene.file, { pixelRatio: scale });
    try {
      await scene.run(page);
      // Let fonts, the resize and any transitions settle.
      await page.evaluate(() => document.fonts.ready);
      await page.waitForTimeout(400);
      const path = join(outDir, scene.file);
      await page.locator("main").screenshot({
        path,
        omitBackground: true,
        animations: "disabled",
        caret: "hide",
      });
      const size = page.viewportSize();
      console.log(`${scene.file}  ${size.width * scale}×${size.height * scale}`);
    } catch (e) {
      failed++;
      console.error(`${scene.file}  FAILED: ${e.message.split("\n")[0]}`);
    } finally {
      await context.close();
    }
  }
} finally {
  await browser.close();
  await server.close();
}
process.exit(failed ? 1 : 0);
