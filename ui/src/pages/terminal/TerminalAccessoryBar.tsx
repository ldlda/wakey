import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ClipboardCopy,
  ClipboardPaste,
  Keyboard,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { MouseEvent, PointerEvent } from "react";

import { Button } from "@/components/ui/button";
import type { TerminalModifiers } from "@/pages/terminal/sessionUtils";

export const MIN_TERMINAL_FONT_SIZE = 10;
export const MAX_TERMINAL_FONT_SIZE = 22;

type Props = {
  inputReady: boolean;
  modifiers: TerminalModifiers;
  fontSize: number;
  onRememberInputFocus: () => void;
  onToggleModifier: (modifier: keyof TerminalModifiers) => void;
  onWriteKey: (data: string) => void;
  onScrollPages: (pages: number) => void;
  onCopy: () => void;
  onPaste: () => void;
  onToggleKeyboard: () => void;
  onChangeFontSize: (delta: number) => void;
};

const CONTROL_KEYS: ReadonlyArray<
  readonly [label: string, sequence: string, title: string]
> = [
  ["^C", "\x03", "Interrupt"],
  ["^U", "\x15", "Clear input line"],
  ["^A", "\x01", "Start of line"],
  ["^E", "\x05", "End of line"],
];

const ARROW_KEYS: ReadonlyArray<
  readonly [label: string, sequence: string, icon: LucideIcon]
> = [
  ["Left arrow", "\x1b[D", ArrowLeft],
  ["Down arrow", "\x1b[B", ArrowDown],
  ["Up arrow", "\x1b[A", ArrowUp],
  ["Right arrow", "\x1b[C", ArrowRight],
];

function preserveInputFocus(
  event: PointerEvent<HTMLDivElement> | MouseEvent<HTMLDivElement>,
  onRememberInputFocus: () => void,
) {
  if (event.target instanceof Element && event.target.closest("button")) {
    onRememberInputFocus();
    event.preventDefault();
  }
}

export function TerminalAccessoryBar({
  inputReady,
  modifiers,
  fontSize,
  onRememberInputFocus,
  onToggleModifier,
  onWriteKey,
  onScrollPages,
  onCopy,
  onPaste,
  onToggleKeyboard,
  onChangeFontSize,
}: Props) {
  return (
    <div
      className="terminal-accessories"
      role="toolbar"
      aria-label="Terminal controls"
      onPointerDownCapture={(event) => {
        // Preserve whether xterm was focused before the button library moves
        // focus. The keyboard button consumes this to show or hide mobile IME.
        preserveInputFocus(event, onRememberInputFocus);
      }}
      onMouseDownCapture={(event) => {
        // Firefox can take the mouse-event path without emitting pointerdown.
        preserveInputFocus(event, onRememberInputFocus);
      }}
    >
      <div className="terminal-accessory-group">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key"
          data-active={modifiers.ctrl || undefined}
          aria-pressed={modifiers.ctrl}
          disabled={!inputReady}
          onClick={() => onToggleModifier("ctrl")}
        >
          Ctrl
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key"
          data-active={modifiers.meta || undefined}
          aria-pressed={modifiers.meta}
          disabled={!inputReady}
          onClick={() => onToggleModifier("meta")}
        >
          Meta
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key"
          disabled={!inputReady}
          onClick={() => onWriteKey("\x1b")}
        >
          Esc
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key"
          disabled={!inputReady}
          onClick={() => onWriteKey("\t")}
        >
          Tab
        </Button>
      </div>

      <div className="terminal-accessory-group">
        {CONTROL_KEYS.map(([label, sequence, title]) => (
          <Button
            key={label}
            type="button"
            size="sm"
            variant="ghost"
            className="terminal-key terminal-key--control"
            aria-label={title}
            title={title}
            disabled={!inputReady}
            onClick={() => onWriteKey(sequence)}
          >
            {label}
          </Button>
        ))}
      </div>

      <div className="terminal-accessory-group">
        {ARROW_KEYS.map(([label, sequence, Icon]) => (
          <Button
            key={label}
            type="button"
            size="icon"
            variant="ghost"
            className="terminal-key terminal-key--icon"
            aria-label={label}
            disabled={!inputReady}
            onClick={() => onWriteKey(sequence)}
          >
            <Icon className="size-4" aria-hidden />
          </Button>
        ))}
      </div>

      <div className="terminal-accessory-group">
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key terminal-key--wide"
          onClick={() => onScrollPages(-1)}
        >
          PgUp
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="terminal-key terminal-key--wide"
          onClick={() => onScrollPages(1)}
        >
          PgDn
        </Button>
      </div>

      <div className="terminal-accessory-group">
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="terminal-key terminal-key--icon"
          aria-label="Copy selection or visible screen"
          title="Copy selection or visible screen"
          onClick={onCopy}
        >
          <ClipboardCopy className="size-4" aria-hidden />
        </Button>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="terminal-key terminal-key--icon"
          aria-label="Paste"
          title="Paste"
          disabled={!inputReady}
          onClick={onPaste}
        >
          <ClipboardPaste className="size-4" aria-hidden />
        </Button>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="terminal-key terminal-key--icon"
          aria-label="Toggle terminal keyboard"
          title="Toggle terminal keyboard"
          disabled={!inputReady}
          onClick={onToggleKeyboard}
        >
          <Keyboard className="size-4" aria-hidden />
        </Button>
      </div>

      <div className="terminal-accessory-group terminal-font-controls">
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="terminal-key terminal-key--icon"
          disabled={fontSize <= MIN_TERMINAL_FONT_SIZE}
          aria-label="Decrease terminal text size"
          onClick={() => onChangeFontSize(-1)}
        >
          <ZoomOut className="size-4" aria-hidden />
        </Button>
        <output aria-label="Terminal text size">{fontSize}</output>
        <Button
          type="button"
          size="icon"
          variant="ghost"
          className="terminal-key terminal-key--icon"
          disabled={fontSize >= MAX_TERMINAL_FONT_SIZE}
          aria-label="Increase terminal text size"
          onClick={() => onChangeFontSize(1)}
        >
          <ZoomIn className="size-4" aria-hidden />
        </Button>
      </div>
    </div>
  );
}
