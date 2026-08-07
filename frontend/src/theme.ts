export type Theme = "light" | "dark";
export type Density = "compact" | "comfortable";

export const THEME_KEY = "ght-theme";
export const DENSITY_KEY = "ght-density";
export const SHOW_DESC_KEY = "ght-show-desc";

export function getPreferredTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_KEY);
    if (stored === "light" || stored === "dark") return stored;
  } catch {
    /* ignore */
  }
  if (typeof window !== "undefined" && window.matchMedia) {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  }
  return "dark";
}

export function applyTheme(theme: Theme): void {
  const t = theme === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = t;
  try {
    localStorage.setItem(THEME_KEY, t);
  } catch {
    /* ignore */
  }
}

export function readDensity(): Density {
  try {
    const v = localStorage.getItem(DENSITY_KEY);
    if (v === "compact" || v === "comfortable") return v;
  } catch {
    /* ignore */
  }
  return "comfortable";
}

export function writeDensity(density: Density): void {
  try {
    localStorage.setItem(DENSITY_KEY, density);
  } catch {
    /* ignore */
  }
}

export function readShowDesc(): boolean {
  try {
    const v = localStorage.getItem(SHOW_DESC_KEY);
    if (v === "0" || v === "false") return false;
    if (v === "1" || v === "true") return true;
  } catch {
    /* ignore */
  }
  return true;
}

export function writeShowDesc(show: boolean): void {
  try {
    localStorage.setItem(SHOW_DESC_KEY, show ? "1" : "0");
  } catch {
    /* ignore */
  }
}

export function applyDensityClasses(density: Density, showDesc: boolean): void {
  document.body.classList.toggle("density-compact", density === "compact");
  document.body.classList.toggle("density-comfortable", density === "comfortable");
  document.body.classList.toggle("hide-desc", !showDesc);
}
