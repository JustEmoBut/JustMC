import type { ModKind } from "./api";

/** Modrinth's search indices, in the order the pickers offer them. */
export const SORTS = [
  ["relevance", "Relevance"],
  ["downloads", "Downloads"],
  ["follows", "Followers"],
  ["newest", "Newest"],
  ["updated", "Updated"],
] as const;

/** "game-mechanics" -> "Game mechanics". The value sent to Modrinth is the
    slug; only the label changes. */
export function label(slug: string) {
  const words = slug.replace(/-/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

export function count(n: number) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return `${n}`;
}

/** "mod" / "resource pack" / "shader", singular or plural, for messages. */
export function noun(kind: ModKind, n: number) {
  const one = kind === "mods" ? "mod" : kind === "resourcepacks" ? "resource pack" : "shader";
  return n === 1 ? one : `${one}s`;
}
