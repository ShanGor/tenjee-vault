import { createElement, useState } from "react";
import { PromptDialog } from "./dialogs";

export function usePrompt() {
  const [state, setState] = useState<{
    title: string;
    label: string;
    initial?: string;
    resolve?: (value: string | null) => void;
  } | null>(null);

  const prompt = (title: string, label: string, initial = "") =>
    new Promise<string | null>((resolve) =>
      setState({ title, label, initial, resolve }),
    );

  const element = state ? createElement(PromptDialog, {
    title: state.title,
    label: state.label,
    initial: state.initial,
    onResolve: (value: string | null) => {
      state.resolve?.(value);
      setState(null);
    },
  }) : null;

  return { prompt, element };
}
