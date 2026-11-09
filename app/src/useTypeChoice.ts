import { useState } from "react";
import { api, errorText, type Category } from "./api";
import { TYPE_ORDER, describeTypes } from "./indexTypes";
import { useIndexedTypes } from "./useIndexedTypes";

type Change = "add" | "remove" | "clear";

export interface TypeQuestion {
  title: string;
  confirmLabel: string;
  danger: boolean;
  paragraphs: string[];
  open: boolean;
}

const UNTOUCHED = "Your files themselves aren’t touched.";

function questionFor(type: Category, change: Change): Omit<TypeQuestion, "open"> {
  const words = describeTypes([type]);
  switch (change) {
    case "add":
      return {
        title: `Index ${words} too?`,
        confirmLabel: `Index ${words}`,
        danger: false,
        paragraphs: [`Mesa will find the ${words} in everything that’s indexed and process them in the background.`],
      };
    case "remove":
      return {
        title: `Remove ${words} from the index?`,
        confirmLabel: "Remove",
        danger: true,
        paragraphs: [`The ${words} in the index will be removed, and they won’t be indexed until you turn them back on.`, UNTOUCHED],
      };
    case "clear":
      return {
        title: "Remove everything from the index?",
        confirmLabel: "Remove everything",
        danger: true,
        paragraphs: [`Turning off ${words} will remove everything from the index.`, UNTOUCHED],
      };
  }
}

const toggled = (types: Category[], type: Category) =>
  TYPE_ORDER.filter((each) => (each === type ? !types.includes(type) : types.includes(each)));

export function useTypeChoice({
  active,
  nothingIndexed,
  onChanged,
  onError,
}: {
  active: boolean;
  nothingIndexed: boolean;
  onChanged: () => Promise<void>;
  onError: (message: string) => void;
}) {
  const indexed = useIndexedTypes(active);
  const [asking, setAsking] = useState<{ type: Category; change: Change; open: boolean } | null>(null);

  const change = async (types: Category[]) => {
    try {
      await indexed.set(types);
      await onChanged();
    } catch (e) {
      onError(`Couldn't change the kinds of file that are indexed. ${errorText(e)}`);
    }
  };

  const toggle = async (type: Category) => {
    const types = indexed.types;
    if (types === null) return;
    const next = toggled(types, type);
    if (nothingIndexed) return void change(next);
    if (types.includes(type)) {
      if (next.length === 0) return setAsking({ type, change: "clear", open: true });
      if ((await api.countIndexedOfType(type).catch(() => -1)) === 0) return void change(next);
      return setAsking({ type, change: "remove", open: true });
    }
    setAsking({ type, change: "add", open: true });
  };

  const answer = (yes: boolean) => {
    if (yes && asking && indexed.types) void change(toggled(indexed.types, asking.type));
    setAsking((now) => now && { ...now, open: false });
  };

  const question: TypeQuestion | null = asking && { ...questionFor(asking.type, asking.change), open: asking.open };
  return { types: indexed.types, toggle, question, answer };
}
