"use client";

import { Component, type ErrorInfo, type ReactNode } from "react";

import { Button } from "@/components/ui/button";

type Props = {
  children: ReactNode;
};

type State = {
  error: Error | null;
};

export class ConsoleErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Console render error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex min-h-screen flex-col items-center justify-center gap-4 p-8 text-center">
          <h1 className="text-xl font-semibold">콘솔을 불러오지 못했습니다</h1>
          <p className="max-w-md text-sm text-muted-foreground">
            {this.state.error.message || "예기치 않은 오류가 발생했습니다."}
          </p>
          <Button type="button" onClick={() => window.location.reload()}>
            새로고침
          </Button>
        </div>
      );
    }

    return this.props.children;
  }
}
