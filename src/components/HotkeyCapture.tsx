import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

function formatKey(k: string): string {
  if (k === " ") return "Space";
  if (k === "ArrowUp") return "Up";
  if (k === "ArrowDown") return "Down";
  if (k === "ArrowLeft") return "Left";
  if (k === "ArrowRight") return "Right";
  if (k === "Escape") return "Esc";
  if (k.length === 1) return k.toUpperCase();
  return k;
}

function buildCombo(e: KeyboardEvent): { combo: string | null; partial: string } {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Win");

  if (MODIFIER_KEYS.has(e.key)) {
    return { combo: null, partial: parts.length ? parts.join("+") + "+…" : "…" };
  }

  parts.push(formatKey(e.key));
  const combo = parts.join("+");
  return { combo, partial: combo };
}

export function HotkeyCapture({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const [capturing, setCapturing] = useState(false);
  const [preview, setPreview] = useState("");
  const pendingCombo = useRef<string | null>(null);
  const pressedKeys = useRef<Set<string>>(new Set());
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!capturing) return;

    pressedKeys.current.clear();
    pendingCombo.current = null;
    setPreview("");

    const handleDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape" && pressedKeys.current.size === 0) {
        setCapturing(false);
        buttonRef.current?.blur();
        return;
      }

      pressedKeys.current.add(e.code);
      const { combo, partial } = buildCombo(e);
      if (combo) pendingCombo.current = combo;
      setPreview(partial);
    };

    const handleUp = (e: KeyboardEvent) => {
      e.preventDefault();
      pressedKeys.current.delete(e.code);

      if (pressedKeys.current.size === 0) {
        if (pendingCombo.current) {
          onChange(pendingCombo.current);
        }
        setCapturing(false);
        buttonRef.current?.blur();
      }
    };

    window.addEventListener("keydown", handleDown, { capture: true });
    window.addEventListener("keyup", handleUp, { capture: true });
    return () => {
      window.removeEventListener("keydown", handleDown, { capture: true });
      window.removeEventListener("keyup", handleUp, { capture: true });
    };
  }, [capturing, onChange]);

  return (
    <button
      ref={buttonRef}
      type="button"
      onClick={() => setCapturing(true)}
      onBlur={() => setCapturing(false)}
      aria-label="Set global hotkey"
      className={cn(
        "flex h-10 w-full items-center justify-between rounded-md border input-fill px-3 py-2 font-mono text-sm text-foreground transition-colors ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
        capturing
          ? "border-accent shadow-[0_0_0_3px_rgba(244,181,138,.15)]"
          : "border-input hover:border-border-strong",
      )}
    >
      <span className={cn("truncate", capturing && !preview && "text-muted-foreground")}>
        {capturing
          ? preview || "Hold a key combination…"
          : value || "Click to set"}
      </span>
      <span
        className={cn(
          "text-[9px] uppercase tracking-[0.14em] ml-3 shrink-0",
          capturing ? "text-accent" : "text-muted-foreground/70",
        )}
      >
        {capturing ? "capturing" : "click to change"}
      </span>
    </button>
  );
}
