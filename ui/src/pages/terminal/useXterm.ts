import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { ImageAddon } from "@xterm/addon-image";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { UnicodeGraphemesAddon } from "@xterm/addon-unicode-graphemes";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal as XTerm } from "@xterm/xterm";
import { toast } from "sonner";

import {
  MAX_TERMINAL_FONT_SIZE,
  MIN_TERMINAL_FONT_SIZE,
} from "@/pages/terminal/TerminalAccessoryBar";
import { visibleTerminalText } from "@/pages/terminal/sessionUtils";
import { loadTerminalFontFamily } from "@/terminal/terminalFonts";

const DEFAULT_TERMINAL_FONT_SIZE = 14;

// Stable behavioral surface shared with session transport and page controls.
export type TerminalPort = {
  clear: () => void;
  copy: () => Promise<void>;
  dimensions: () => { rows: number; cols: number } | null;
  forgetTitle: (terminalId: string) => void;
  fit: () => void;
  focus: () => void;
  blurInput: () => void;
  onInput: (handler: (data: string) => void) => { dispose: () => void };
  onResize: (handler: () => void) => { dispose: () => void };
  paste: (write: (data: string) => void) => Promise<void>;
  reset: () => void;
  scrollPages: (pages: number) => void;
  sendResize: (send: (message: string) => void, force?: boolean) => void;
  changeFontSize: (delta: number) => void;
  setActiveTerminalId: (terminalId: string | undefined) => void;
  write: (data: Uint8Array) => void;
  writeLine: (line: string) => void;
  ready: boolean;
};

type Options = {
  pageRef: RefObject<HTMLElement | null>;
  hostRef: RefObject<HTMLDivElement | null>;
};

export function useXterm({ pageRef, hostRef }: Options) {
  const terminalRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const activeTerminalIdRef = useRef<string | undefined>(undefined);
  const lastSizeRef = useRef({ rows: 0, cols: 0 });
  const resizeListenersRef = useRef(new Set<() => void>());
  const [fontFamily, setFontFamily] = useState<string>();
  const [ready, setReady] = useState(false);
  const [fontSize, setFontSize] = useState(DEFAULT_TERMINAL_FONT_SIZE);
  const [sessionTitles, setSessionTitles] = useState<Record<string, string>>(
    {},
  );

  const setActiveTerminalId = useCallback((terminalId: string | undefined) => {
    activeTerminalIdRef.current = terminalId;
  }, []);

  useEffect(() => {
    let cancelled = false;
    void loadTerminalFontFamily().then((loadedFontFamily) => {
      if (!cancelled) setFontFamily(loadedFontFamily);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const viewport = window.visualViewport;
    const updateViewportHeight = () => {
      const height = Math.round(viewport?.height ?? window.innerHeight);
      pageRef.current?.style.setProperty(
        "--terminal-visual-viewport-height",
        `${height}px`,
      );
    };
    updateViewportHeight();
    viewport?.addEventListener("resize", updateViewportHeight);
    viewport?.addEventListener("scroll", updateViewportHeight);
    window.addEventListener("resize", updateViewportHeight);
    return () => {
      viewport?.removeEventListener("resize", updateViewportHeight);
      viewport?.removeEventListener("scroll", updateViewportHeight);
      window.removeEventListener("resize", updateViewportHeight);
    };
  }, [pageRef]);

  const sendResize = useCallback(
    (send: (message: string) => void, force = false) => {
      const terminal = terminalRef.current;
      if (!terminal) return;
      if (
        !force &&
        lastSizeRef.current.rows === terminal.rows &&
        lastSizeRef.current.cols === terminal.cols
      ) {
        return;
      }
      lastSizeRef.current = { rows: terminal.rows, cols: terminal.cols };
      send(
        JSON.stringify({
          type: "resize",
          rows: terminal.rows,
          cols: terminal.cols,
        }),
      );
    },
    [],
  );

  const fit = useCallback(() => fitRef.current?.fit(), []);
  const focus = useCallback(() => terminalRef.current?.focus(), []);
  const blurInput = useCallback(
    () => terminalRef.current?.textarea?.blur(),
    [],
  );
  const reset = useCallback(() => terminalRef.current?.reset(), []);
  const clear = useCallback(() => terminalRef.current?.clear(), []);
  const write = useCallback(
    (data: Uint8Array) => terminalRef.current?.write(data),
    [],
  );
  const writeLine = useCallback(
    (line: string) => terminalRef.current?.writeln(line),
    [],
  );
  const dimensions = useCallback(() => {
    const terminal = terminalRef.current;
    return terminal ? { rows: terminal.rows, cols: terminal.cols } : null;
  }, []);
  const forgetTitle = useCallback((terminalId: string) => {
    setSessionTitles((current) => {
      if (!(terminalId in current)) return current;
      const next = { ...current };
      delete next[terminalId];
      return next;
    });
  }, []);
  const onInput = useCallback(
    (handler: (data: string) => void) =>
      terminalRef.current?.onData(handler) ?? { dispose: () => {} },
    [],
  );
  const onResize = useCallback((handler: () => void) => {
    resizeListenersRef.current.add(handler);
    return { dispose: () => resizeListenersRef.current.delete(handler) };
  }, []);
  const scrollPages = useCallback(
    (pages: number) => terminalRef.current?.scrollPages(pages),
    [],
  );
  const copy = useCallback(async () => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    const text = terminal.hasSelection()
      ? terminal.getSelection()
      : visibleTerminalText(terminal);
    if (!text) {
      toast.info("Nothing to copy");
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      toast.success(
        terminal.hasSelection() ? "Selection copied" : "Visible screen copied",
      );
    } catch (error) {
      toast.error("Clipboard access was denied", {
        description: String(error),
      });
    }
  }, []);
  const paste = useCallback(async (writeData: (data: string) => void) => {
    try {
      const text = await navigator.clipboard.readText();
      if (text) writeData(text);
      terminalRef.current?.focus();
    } catch (error) {
      toast.error("Clipboard access was denied", {
        description: String(error),
      });
    }
  }, []);
  const changeFontSize = useCallback((delta: number) => {
    const terminal = terminalRef.current;
    if (!terminal) return;
    const current = terminal.options.fontSize ?? DEFAULT_TERMINAL_FONT_SIZE;
    const next = Math.min(
      MAX_TERMINAL_FONT_SIZE,
      Math.max(MIN_TERMINAL_FONT_SIZE, current + delta),
    );
    if (next === current) return;
    terminal.options.fontSize = next;
    setFontSize(next);
    window.requestAnimationFrame(() => {
      fitRef.current?.fit();
      resizeListenersRef.current.forEach((handler) => handler());
    });
  }, []);

  // Wait for the font before creating xterm; this effect owns the instance and its observers.
  useEffect(() => {
    if (!hostRef.current || !fontFamily) return;
    const terminal = new XTerm({
      cursorBlink: true,
      convertEol: false,
      fontFamily,
      fontSize: DEFAULT_TERMINAL_FONT_SIZE,
      lineHeight: 1.1,
      scrollbar: {
        width: window.matchMedia("(pointer: coarse)").matches ? 22 : 16,
      },
      scrollback: 10_000,
      theme: {
        background: "#0b1117",
        foreground: "#e5e7eb",
        cursor: "#6ee7a8",
        scrollbarSliderBackground: "#52617099",
        scrollbarSliderHoverBackground: "#6b7c8dcc",
        scrollbarSliderActiveBackground: "#8294a6",
      },
      allowProposedApi: true,
    });
    terminal.loadAddon(new Unicode11Addon());
    terminal.unicode.activeVersion = "11";
    terminal.loadAddon(new UnicodeGraphemesAddon());
    terminal.loadAddon(new WebLinksAddon());
    terminal.loadAddon(new ImageAddon({}));
    const fitAddon = new FitAddon();
    terminal.loadAddon(new ClipboardAddon());
    terminal.loadAddon(fitAddon);
    terminal.open(hostRef.current);
    terminal.attachCustomKeyEventHandler((event) => {
      const copiesTerminalSelection =
        event.type === "keydown" &&
        event.ctrlKey &&
        event.shiftKey &&
        event.code === "KeyC";
      if (!copiesTerminalSelection) return true;
      event.preventDefault();
      event.stopPropagation();
      if (terminal.hasSelection()) {
        if (!navigator.clipboard) {
          toast.error("Clipboard access requires HTTPS or localhost");
        } else {
          void navigator.clipboard
            .writeText(terminal.getSelection())
            .catch((error) =>
              toast.error("Clipboard access was denied", {
                description: String(error),
              }),
            );
        }
      }
      return false;
    });
    const titleChange = terminal.onTitleChange((title) => {
      const terminalId = activeTerminalIdRef.current;
      if (!terminalId) return;
      const normalized = title.trim();
      setSessionTitles((current) => {
        if (current[terminalId] === normalized) return current;
        if (!normalized) {
          const next = { ...current };
          delete next[terminalId];
          return next;
        }
        return { ...current, [terminalId]: normalized };
      });
    });
    fitAddon.fit();
    // Consumers subscribe only after this transition; the port methods stay stable.
    terminalRef.current = terminal;
    fitRef.current = fitAddon;
    setReady(true);

    let resizeFrame = 0;
    let lastHostWidth = 0;
    let lastHostHeight = 0;
    const resizeObserver = new ResizeObserver(([entry]) => {
      const width = Math.round(entry.contentRect.width);
      const height = Math.round(entry.contentRect.height);
      if (width === lastHostWidth && height === lastHostHeight) return;
      lastHostWidth = width;
      lastHostHeight = height;
      window.cancelAnimationFrame(resizeFrame);
      resizeFrame = window.requestAnimationFrame(() => {
        fitAddon.fit();
        resizeListenersRef.current.forEach((handler) => handler());
      });
    });
    resizeObserver.observe(hostRef.current);

    return () => {
      titleChange.dispose();
      resizeObserver.disconnect();
      resizeListenersRef.current.clear();
      window.cancelAnimationFrame(resizeFrame);
      terminal.dispose();
      terminalRef.current = null;
      fitRef.current = null;
      setReady(false);
    };
  }, [fontFamily, hostRef]);

  const terminal = useMemo<TerminalPort>(
    () => ({
      clear,
      copy,
      dimensions,
      forgetTitle,
      fit,
      focus,
      blurInput,
      onInput,
      onResize,
      paste,
      reset,
      scrollPages,
      sendResize,
      changeFontSize,
      setActiveTerminalId,
      write,
      writeLine,
      ready,
    }),
    [
      clear,
      copy,
      dimensions,
      forgetTitle,
      fit,
      focus,
      blurInput,
      onInput,
      onResize,
      paste,
      reset,
      scrollPages,
      sendResize,
      changeFontSize,
      setActiveTerminalId,
      write,
      writeLine,
      ready,
    ],
  );

  return { terminal, fontSize, sessionTitles };
}
