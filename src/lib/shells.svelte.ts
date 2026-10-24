/**
 * The built-in shell's sessions, one per folder. Each owns its xterm.js
 * instance and DOM element, kept alive across screen changes and overlay
 * hides so scrollback, a running dev server and the prompt are all still
 * there when you come back. The process side lives in `src-tauri/src/pty.rs`.
 */
import { Terminal, type ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import "@xterm/xterm/css/xterm.css";
import { SvelteMap } from "svelte/reactivity";
import {
  copyText,
  openUrl,
  readClipboard,
  shellClose,
  shellOpen,
  shellResize,
  shellWrite,
} from "./ipc";
import { classifyShellKey } from "./shellKeys";
import type { Repo } from "./types";

/** Nerd Font builds first, so icon-heavy prompts (oh-my-posh, Starship)
 *  render when one is installed; then the stock monospace fonts. */
export const DEFAULT_SHELL_FONT =
  '"CaskaydiaCove Nerd Font Mono", "CaskaydiaMono NF", "JetBrainsMono Nerd Font Mono", ' +
  '"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", "Liberation Mono", monospace';
export const DEFAULT_SHELL_FONT_SIZE = 13;

let font = { family: DEFAULT_SHELL_FONT, size: DEFAULT_SHELL_FONT_SIZE };

/** Apply the Settings font to every session, open or not. Blank = default. */
export function setShellFont(family: string | null | undefined, size: number | null | undefined) {
  font = {
    family: family?.trim() ? `${family.trim()}, ${DEFAULT_SHELL_FONT}` : DEFAULT_SHELL_FONT,
    size: size && size >= 6 && size <= 40 ? size : DEFAULT_SHELL_FONT_SIZE,
  };
  for (const s of shells.values()) s.applyFont();
}

// The panel's palette: Tailwind v3 tones, so shell output sits with the rest
// of the UI. Transparent background — the panel's own fill shows through.
const theme: ITheme = {
  background: "#00000000",
  foreground: "#e4e4e7",
  cursor: "#7dd3fc",
  cursorAccent: "#141418",
  selectionBackground: "rgba(125, 211, 252, 0.28)",
  // Normal colours are mid-tones (500s): programs use them as backgrounds
  // too — PowerShell lists folders as white on blue — and a pastel there
  // leaves the text unreadable. The pastels (300s) are the bright variants.
  black: "#27272a",
  red: "#ef4444",
  green: "#10b981",
  yellow: "#f59e0b",
  blue: "#3b82f6",
  magenta: "#a855f7",
  cyan: "#06b6d4",
  white: "#d4d4d8",
  brightBlack: "#71717a",
  brightRed: "#fca5a5",
  brightGreen: "#6ee7b7",
  brightYellow: "#fcd34d",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#67e8f9",
  brightWhite: "#fafafa",
};

export type ShellState = "starting" | "live" | "exited";

const encoder = new TextEncoder();

export class ShellSession {
  /** The folder the shell runs in, and this session's key. */
  readonly cwd: string;
  /** The repo it was opened from — for the running-shells list and the header. */
  readonly repo: Repo;
  /** Shell picked for it; "" = `config.shell` / the platform default. */
  shell = $state("");
  /** What the backend actually started (`pwsh`, `bash`, …). */
  label = $state("");
  state = $state<ShellState>("starting");
  exitCode = $state<number | null>(null);
  /** Why it couldn't start, when it couldn't. */
  error = $state("");

  readonly el: HTMLDivElement;
  private readonly term: Terminal;
  private readonly fitter = new FitAddon();
  private opened = false;
  private id: number | null = null;
  /** Bumped on every (re)start, so a replaced process's late output and exit
   *  can't land in its successor. */
  private gen = 0;
  /** Input waiting to go out; sent one write at a time so keystrokes can't
   *  overtake each other on the way. */
  private pending: Uint8Array[] = [];
  private sending = false;

  constructor(cwd: string, repo: Repo, shell: string) {
    this.cwd = cwd;
    this.repo = repo;
    this.shell = shell;
    this.el = document.createElement("div");
    this.el.className = "h-full w-full";
    this.term = new Terminal({
      allowTransparency: true,
      theme,
      fontFamily: font.family,
      fontSize: font.size,
      scrollback: 5000,
      cursorBlink: true,
      // Programs pick their own text-on-background pairs, and some clash in
      // any palette — `ls` shows WSL's /mnt folders blue on green. Below this
      // contrast, xterm.js shifts the text lighter or darker until it reads.
      minimumContrastRatio: 4.5,
      // The Unicode 11 width table (below) is a "proposed" API.
      allowProposedApi: true,
      // Windows: ConPTY already reflows wrapped lines its own way; telling
      // xterm.js keeps the two from fighting over resized output.
      windowsPty: navigator.userAgent.includes("Windows") ? { backend: "conpty" } : undefined,
    });
    this.term.loadAddon(this.fitter);
    // Emoji and other wide characters at their modern widths — prompts like
    // Starship's put emoji right before the cursor, and the old table leaves it
    // a column off.
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = "11";
    // Ctrl+click opens a link (a dev server's localhost URL, an error's docs
    // page), as in Windows Terminal — a plain click stays free for selecting.
    this.term.loadAddon(
      new WebLinksAddon((e, uri) => {
        if (e.ctrlKey) void openUrl(uri).catch(() => {});
      }),
    );
    this.term.attachCustomKeyEventHandler((e) => this.onKey(e));
    this.term.onData((d) => this.input(encoder.encode(d)));
    this.term.onBinary((d) => this.input(Uint8Array.from(d, (c) => c.charCodeAt(0))));
    this.term.onResize(({ cols, rows }) => {
      if (this.id !== null) void shellResize(this.id, cols, rows).catch(() => {});
    });
  }

  /** Mount into `host` (the shell screen) and start the process on first show.
   *  The element has to be on screen first — sizing measures the font. */
  show(host: HTMLElement) {
    if (this.el.parentElement !== host) host.replaceChildren(this.el);
    if (!this.opened) {
      this.term.open(this.el);
      this.opened = true;
    }
    this.fit();
    if (this.state === "starting" && this.id === null && !this.error) void this.start();
    this.term.focus();
  }

  focus() {
    this.term.focus();
  }

  fit() {
    if (!this.opened || !this.el.isConnected || this.el.offsetParent === null) return;
    try {
      this.fitter.fit();
    } catch {
      // Not measurable yet (zero-size host mid-resize); the next fit catches up.
    }
  }

  applyFont() {
    this.term.options.fontFamily = font.family;
    this.term.options.fontSize = font.size;
    this.fit();
  }

  hasSelection() {
    return this.term.hasSelection();
  }

  copySelection() {
    const text = this.term.getSelection();
    if (text) void copyText(text);
    this.term.clearSelection();
  }

  async paste() {
    const text = await readClipboard();
    // xterm.js applies bracketed paste when the shell asked for it, so a
    // multi-line paste doesn't run line by line.
    if (text) this.term.paste(text);
    this.term.focus();
  }

  clear() {
    this.term.clear();
    this.term.focus();
  }

  /** Start (or start over) in `shell`, or in the same shell when omitted. */
  async restart(shell?: string) {
    if (shell !== undefined) this.shell = shell;
    this.gen++; // the old process's last words stay out of the fresh screen
    const old = this.id;
    this.id = null;
    this.pending = [];
    if (old !== null) void shellClose(old).catch(() => {});
    this.term.reset();
    await this.start();
  }

  /** Resolves a `dispose` waiting for the process to be gone. */
  private gone: (() => void) | null = null;

  /** End the process and drop the terminal for good. Resolves once the
   *  process has exited (or after a few seconds regardless) — until then it
   *  still has its folder open, which keeps that from being moved. */
  dispose(): Promise<void> {
    this.gen++;
    const done =
      this.id === null
        ? Promise.resolve()
        : new Promise<void>((resolve) => {
            this.gone = resolve;
            setTimeout(resolve, 5000);
          });
    if (this.id !== null) void shellClose(this.id).catch(() => {});
    this.id = null;
    this.term.dispose();
    this.el.remove();
    return done;
  }

  private async start() {
    const gen = ++this.gen;
    this.state = "starting";
    this.exitCode = null;
    this.error = "";
    const { cols, rows } = this.term;
    try {
      const { id, label } = await shellOpen(
        this.cwd,
        this.shell,
        cols,
        rows,
        (bytes) => {
          if (gen === this.gen) this.term.write(bytes);
        },
        (code) => {
          this.gone?.();
          this.gone = null;
          if (gen === this.gen) this.exited(code);
        },
      );
      if (gen !== this.gen) {
        void shellClose(id).catch(() => {}); // replaced while it was starting
        return;
      }
      this.label = label;
      // A shell that exited at once has already been reported; keep that.
      if (this.state === "starting") {
        this.id = id;
        this.state = "live";
        void this.flush();
      }
    } catch (e) {
      if (gen !== this.gen) return;
      this.error = `${e}`;
      this.state = "exited";
      this.term.write(`\x1b[31m${this.error}\x1b[0m\r\n`);
      this.hint();
    }
  }

  private exited(code: number | null) {
    this.id = null;
    this.pending = [];
    this.exitCode = code;
    this.state = "exited";
    // A clean `exit` closes the session, like a terminal tab; anything else
    // stays up so the output that explains it can still be read.
    if (code === 0) {
      closeShell(this.cwd);
      return;
    }
    this.term.write(
      `\r\n\x1b[2m[exited${code === null ? "" : ` with code ${code}`}]\x1b[0m\r\n`,
    );
    this.hint();
  }

  private hint() {
    this.term.write("\x1b[2mEnter starts it again · Shift+Esc goes back\x1b[0m\r\n");
  }

  private input(bytes: Uint8Array) {
    if (this.state === "exited") {
      if (bytes.length === 1 && bytes[0] === 0x0d) void this.restart();
      return;
    }
    this.pending.push(bytes);
    void this.flush();
  }

  private async flush() {
    if (this.sending) return;
    this.sending = true;
    try {
      while (this.id !== null && this.pending.length) {
        const chunks = this.pending.splice(0);
        const all = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
        let at = 0;
        for (const c of chunks) {
          all.set(c, at);
          at += c.length;
        }
        await shellWrite(this.id, all);
      }
    } catch {
      // The session ended between keystrokes; its exit is on its way.
    } finally {
      this.sending = false;
    }
  }

  /** xterm.js asks before handling each key; `false` keeps it from the shell. */
  private onKey(e: KeyboardEvent): boolean {
    const verdict = classifyShellKey(e, this.term.hasSelection());
    if (verdict === "shell") return true;
    if (e.type === "keydown") {
      if (verdict === "copy") {
        e.preventDefault();
        this.copySelection();
      } else if (verdict === "paste") {
        // Ours, not the webview's native paste — that one isn't reliable for
        // Ctrl+Shift+V everywhere, and two would paste twice.
        e.preventDefault();
        void this.paste();
      }
      // "back" and "zoom" bubble on to the window, where App steps back or zooms.
    }
    return false;
  }
}

/** Every session, keyed by folder. */
export const shells = new SvelteMap<string, ShellSession>();

/** The session for `cwd` (in `repo`), created (not yet started) when there
 *  isn't one. */
export function shellFor(cwd: string, repo: Repo, shell = ""): ShellSession {
  let s = shells.get(cwd);
  if (!s) {
    s = new ShellSession(cwd, repo, shell);
    shells.set(cwd, s);
  }
  return s;
}

/** Resolves once its process has exited (see `ShellSession.dispose`). */
export function closeShell(cwd: string): Promise<void> {
  const s = shells.get(cwd);
  if (!s) return Promise.resolve();
  shells.delete(cwd);
  return s.dispose();
}

/** `cwd` is `root` or a folder inside it. */
export function isUnder(cwd: string, root: string): boolean {
  const r = root.replace(/[\\/]+$/, "");
  return cwd === r || cwd.startsWith(r + "/") || cwd.startsWith(r + "\\");
}

/** Where `s` runs relative to its repo, or null at the repo root. */
export function shellCrumb(s: ShellSession): string | null {
  const root = s.repo.path;
  if (s.cwd === root) return null;
  return isUnder(s.cwd, root) ? s.cwd.slice(root.length).replace(/^[\\/]+/, "") : s.cwd;
}

/** Every shell in `path` or a folder inside it (a repo's root and sub-projects). */
export function shellsUnder(path: string): ShellSession[] {
  return [...shells.values()].filter((s) => isUnder(s.cwd, path));
}

/** Whether a running shell sits in `path` or a folder inside it. */
export function hasShellUnder(path: string): boolean {
  return shellsUnder(path).some((s) => s.state !== "exited");
}

/** Resolves once they've all exited. */
export function closeShellsUnder(path: string): Promise<void> {
  return Promise.all(shellsUnder(path).map((s) => closeShell(s.cwd))).then(() => {});
}

export function closeAllShells() {
  for (const cwd of [...shells.keys()]) closeShell(cwd);
}
