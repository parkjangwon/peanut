"use client";

import type React from "react";
import { useEffect, useRef, useState } from "react";

import { cn } from "@/lib/utils";

export function FunctionField({
  label,
  help,
  children,
}: {
  label: string;
  help: string;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <span className="text-sm font-medium">{label}</span>
      {children}
      <span className="block text-xs leading-5 text-muted-foreground">{help}</span>
    </div>
  );
}

export function CodeWorkbenchPane({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-w-0 border-t p-4 xl:border-l xl:border-t-0 first:xl:border-l-0">
      <div className="mb-3 min-h-14">
        <div className="text-sm font-medium">{title}</div>
        <div className="mt-1 text-xs leading-5 text-muted-foreground">{description}</div>
      </div>
      {children}
    </div>
  );
}

export function CodeEditor({
  value,
  onChange,
  minHeight = 480,
  readOnly = false,
}: {
  value: string;
  onChange?: (value: string) => void;
  minHeight?: number;
  readOnly?: boolean;
}) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const gutterRef = useRef<HTMLDivElement | null>(null);
  const historyRef = useRef([{ value, selectionStart: 0, selectionEnd: 0 }]);
  const historyIndexRef = useRef(0);
  const [currentLine, setCurrentLine] = useState(1);
  const [scrollTop, setScrollTop] = useState(0);
  const lineHeight = 20;
  const lines = Math.max(1, value.split("\n").length);

  useEffect(() => {
    const currentSnapshot = historyRef.current[historyIndexRef.current];
    if (currentSnapshot && currentSnapshot.value !== value) {
      historyRef.current = [{ value, selectionStart: 0, selectionEnd: 0 }];
      historyIndexRef.current = 0;
    }
  }, [value]);

  const syncCursorLine = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    setCurrentLine(value.slice(0, textarea.selectionStart).split("\n").length);
  };

  const pushHistory = (nextValue: string, nextStart: number, nextEnd = nextStart) => {
    const current = historyRef.current[historyIndexRef.current];
    if (current?.value === nextValue && current.selectionStart === nextStart && current.selectionEnd === nextEnd) {
      return;
    }
    const nextHistory = historyRef.current.slice(0, historyIndexRef.current + 1);
    nextHistory.push({ value: nextValue, selectionStart: nextStart, selectionEnd: nextEnd });
    if (nextHistory.length > 100) {
      nextHistory.shift();
    }
    historyRef.current = nextHistory;
    historyIndexRef.current = nextHistory.length - 1;
  };

  const updateValueAndSelection = (nextValue: string, nextStart: number, nextEnd = nextStart) => {
    if (!onChange || readOnly) return;
    pushHistory(nextValue, nextStart, nextEnd);
    onChange(nextValue);
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.selectionStart = nextStart;
      textarea.selectionEnd = nextEnd;
      textarea.focus();
      setCurrentLine(nextValue.slice(0, nextStart).split("\n").length);
    });
  };

  const applyHistorySnapshot = (snapshot: { value: string; selectionStart: number; selectionEnd: number }) => {
    if (!onChange || readOnly) return;
    onChange(snapshot.value);
    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      if (!textarea) return;
      textarea.selectionStart = snapshot.selectionStart;
      textarea.selectionEnd = snapshot.selectionEnd;
      textarea.focus();
      setCurrentLine(snapshot.value.slice(0, snapshot.selectionStart).split("\n").length);
    });
  };

  const undo = () => {
    if (historyIndexRef.current <= 0) return;
    historyIndexRef.current -= 1;
    applyHistorySnapshot(historyRef.current[historyIndexRef.current]);
  };

  const redo = () => {
    if (historyIndexRef.current >= historyRef.current.length - 1) return;
    historyIndexRef.current += 1;
    applyHistorySnapshot(historyRef.current[historyIndexRef.current]);
  };

  const handleTab = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (readOnly) return;
    const isModifierPressed = event.metaKey || event.ctrlKey;
    const key = event.key.toLowerCase();
    if (isModifierPressed && key === "z") {
      event.preventDefault();
      if (event.shiftKey) {
        redo();
      } else {
        undo();
      }
      return;
    }
    if (isModifierPressed && key === "y") {
      event.preventDefault();
      redo();
      return;
    }
    if (event.key !== "Tab") return;
    event.preventDefault();

    const textarea = event.currentTarget;
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const lineStart = value.lastIndexOf("\n", start - 1) + 1;

    if (event.shiftKey) {
      const selectedBlock = value.slice(lineStart, end);
      let removedBeforeStart = 0;
      let removedTotal = 0;
      const unindented = selectedBlock.replace(/^( {1,2}|\t)/gm, (match, indent: string, offset: number) => {
        const removed = indent.length;
        removedTotal += removed;
        if (offset < start - lineStart) {
          removedBeforeStart += removed;
        }
        return "";
      });
      const nextValue = value.slice(0, lineStart) + unindented + value.slice(end);
      updateValueAndSelection(
        nextValue,
        Math.max(lineStart, start - removedBeforeStart),
        Math.max(lineStart, end - removedTotal),
      );
      return;
    }

    if (start !== end && value.slice(start, end).includes("\n")) {
      const selectedBlock = value.slice(lineStart, end);
      const indented = selectedBlock.replace(/^/gm, "  ");
      const addedLines = selectedBlock.split("\n").length;
      const nextValue = value.slice(0, lineStart) + indented + value.slice(end);
      updateValueAndSelection(nextValue, start + 2, end + addedLines * 2);
      return;
    }

    const nextValue = value.slice(0, start) + "  " + value.slice(end);
    updateValueAndSelection(nextValue, start + 2);
  };

  return (
    <div
      className="relative grid grid-cols-[3.5rem_minmax(0,1fr)] overflow-hidden rounded-lg border bg-background shadow-xs"
      style={{ minHeight }}
    >
      <div
        ref={gutterRef}
        className="select-none overflow-hidden border-r bg-muted/40 py-3 text-right font-mono text-xs leading-5 text-muted-foreground"
        aria-hidden="true"
      >
        {Array.from({ length: lines }, (_, index) => (
          <div
            key={index + 1}
            className={cn(
              "px-3 tabular-nums",
              currentLine === index + 1 && "font-semibold text-primary",
            )}
          >
            {index + 1}
          </div>
        ))}
      </div>
      <div className="relative min-w-0">
        <div
          className="pointer-events-none absolute left-0 right-0 bg-primary/10"
          style={{
            height: lineHeight,
            top: (currentLine - 1) * lineHeight + 12 - scrollTop,
          }}
        />
        <textarea
          ref={textareaRef}
          value={value}
          readOnly={readOnly}
          spellCheck={false}
          onChange={(event) => {
            if (!onChange || readOnly) return;
            const nextValue = event.target.value;
            const nextStart = event.target.selectionStart;
            const nextEnd = event.target.selectionEnd;
            pushHistory(nextValue, nextStart, nextEnd);
            onChange(nextValue);
            setCurrentLine(nextValue.slice(0, nextStart).split("\n").length);
          }}
          onKeyDown={handleTab}
          onClick={syncCursorLine}
          onKeyUp={syncCursorLine}
          onScroll={(event) => {
            const nextScrollTop = event.currentTarget.scrollTop;
            setScrollTop(nextScrollTop);
            if (gutterRef.current) {
              gutterRef.current.scrollTop = nextScrollTop;
            }
          }}
          className="relative z-10 w-full resize-y bg-transparent px-4 py-3 font-mono text-xs leading-5 text-foreground outline-none selection:bg-primary/20"
          style={{ minHeight }}
        />
      </div>
    </div>
  );
}
export function JsonBlock({ value, minHeight = 288 }: { value: unknown; minHeight?: number }) {
  return <CodeEditor value={JSON.stringify(value, null, 2)} minHeight={minHeight} readOnly />;
}
