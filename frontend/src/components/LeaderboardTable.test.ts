import { describe, expect, it } from "vitest";
import {
  primaryHeader,
  primaryValue,
  secondaryLabel,
} from "./LeaderboardTable";
import type { LeaderboardItem } from "../types";

const item: LeaderboardItem = {
  rank: 1,
  full_name: "owner/repo",
  html_url: "https://github.com/owner/repo",
  description: "desc",
  language: "TypeScript",
  license: "MIT",
  topics: ["typescript", "web"],
  languages: [
    { name: "TypeScript", pct: 80 },
    { name: "CSS", pct: 20 },
  ],
  stars: 455_000,
  forks: 50_000,
  watchers: 12_000,
  stars_today: 320,
  tracked_by_me: false,
};

describe("primaryHeader", () => {
  it("labels trending as 今日 ★", () => {
    expect(primaryHeader("trending", "stars")).toBe("今日 ★");
  });
  it("follows top metric", () => {
    expect(primaryHeader("top", "stars")).toBe("★");
    expect(primaryHeader("top", "forks")).toBe("Fork");
    expect(primaryHeader("top", "watchers")).toBe("Watch");
  });
});

describe("primaryValue", () => {
  it("uses stars_today on trending", () => {
    expect(primaryValue(item, "trending", "stars")).toBe(320);
  });
  it("uses selected metric on top", () => {
    expect(primaryValue(item, "top", "stars")).toBe(455_000);
    expect(primaryValue(item, "top", "forks")).toBe(50_000);
    expect(primaryValue(item, "top", "watchers")).toBe(12_000);
  });
});

describe("secondaryLabel", () => {
  it("merges stars+forks on trending", () => {
    expect(secondaryLabel(item, "trending", "stars")).toMatch(/★/);
    expect(secondaryLabel(item, "trending", "stars")).toMatch(/⑂/);
  });
  it("omits primary metric on top stars (no dual ★)", () => {
    const s = secondaryLabel(item, "top", "stars");
    expect(s).toMatch(/⑂/);
    expect(s).toMatch(/👁/);
    expect(s).not.toMatch(/★/);
  });
  it("shows stars+watchers when primary is forks", () => {
    const s = secondaryLabel(item, "top", "forks");
    expect(s).toMatch(/★/);
    expect(s).toMatch(/👁/);
    expect(s).not.toMatch(/⑂/);
  });
});
