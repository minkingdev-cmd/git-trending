/** GitHub-ish language colors for multi-lang bars (fallback: Other). */
const LANG_COLORS: Record<string, string> = {
  TypeScript: "#3178c6",
  JavaScript: "#f1e05a",
  Python: "#3572A5",
  Rust: "#dea584",
  Go: "#00ADD8",
  Java: "#b07219",
  Kotlin: "#A97BFF",
  Swift: "#F05138",
  Ruby: "#701516",
  PHP: "#4F5D95",
  "C#": "#178600",
  C: "#555555",
  "C++": "#f34b7d",
  Shell: "#89e051",
  HTML: "#e34c26",
  CSS: "#563d7c",
  SCSS: "#c6538c",
  Vue: "#41b883",
  Svelte: "#ff3e00",
  Dart: "#00B4AB",
  Lua: "#000080",
  R: "#198CE7",
  Scala: "#c22d40",
  Elixir: "#6e4a7e",
  Haskell: "#5e5086",
  Markdown: "#083fa1",
  MDX: "#fcb32c",
  Jupyter: "#DA5B0B",
  Dockerfile: "#384d54",
  Makefile: "#427819",
  PowerShell: "#012456",
  Astro: "#ff5a00",
  Assembly: "#6E4C13",
  Cuda: "#3A4E3A",
  Other: "#8b949e",
};

export function langColor(name: string): string {
  return LANG_COLORS[name] ?? LANG_COLORS.Other;
}

/** Format language share percentage for display. */
export function formatPct(pct: number): string {
  if (pct >= 10) return `${Math.round(pct)}%`;
  if (pct >= 1) return `${pct.toFixed(1).replace(/\.0$/, "")}%`;
  return "<1%";
}
