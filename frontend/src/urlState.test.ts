import { describe, expect, it } from "vitest";
import {
  buildSearch,
  hasActiveFilters,
  parseCsvList,
  parseSearch,
  toggleInList,
  type UrlState,
} from "./urlState";

describe("parseCsvList", () => {
  it("splits, trims, drops empty", () => {
    expect(parseCsvList("ai, llm , ,rust")).toEqual(["ai", "llm", "rust"]);
  });
  it("lowercases when requested and dedupes", () => {
    expect(parseCsvList("AI,ai, LLM", true)).toEqual(["ai", "llm"]);
  });
  it("returns empty for null/blank", () => {
    expect(parseCsvList(null)).toEqual([]);
    expect(parseCsvList("  ")).toEqual([]);
  });
});

describe("parseSearch", () => {
  it("parses topics and topic_mode", () => {
    const s = parseSearch("?topics=ai,llm&topic_mode=or");
    expect(s.topics).toEqual(["ai", "llm"]);
    expect(s.topicMode).toBe("or");
  });

  it("defaults board/metric/topicMode", () => {
    const s = parseSearch("");
    expect(s.board).toBe("trending");
    expect(s.metric).toBe("stars");
    expect(s.topicMode).toBe("and");
    expect(s.q).toBe("");
    expect(s.topics).toEqual([]);
    expect(s.languages).toEqual([]);
  });

  it("parses board top + metric + date + q + languages", () => {
    const s = parseSearch(
      "?board=top&metric=forks&date=2026-08-07&q=react&languages=TypeScript,Go",
    );
    expect(s.board).toBe("top");
    expect(s.metric).toBe("forks");
    expect(s.date).toBe("2026-08-07");
    expect(s.q).toBe("react");
    expect(s.languages).toEqual(["TypeScript", "Go"]);
  });

  it("parses board=tracked", () => {
    const s = parseSearch("?board=tracked&q=cli");
    expect(s.board).toBe("tracked");
    expect(s.q).toBe("cli");
  });

  it("maps legacy lang to languages when languages absent", () => {
    const s = parseSearch("?lang=Rust");
    expect(s.languages).toEqual(["Rust"]);
  });

  it("prefers languages over legacy lang", () => {
    const s = parseSearch("?lang=Rust&languages=Go");
    expect(s.languages).toEqual(["Go"]);
  });

  it("accepts search without leading ?", () => {
    expect(parseSearch("topics=ai,llm&topic_mode=or").topics).toEqual(["ai", "llm"]);
  });
});

describe("buildSearch", () => {
  it("omits defaults and empty filters", () => {
    const state: UrlState = {
      board: "trending",
      metric: "stars",
      date: "",
      q: "",
      topics: [],
      topicMode: "and",
      languages: [],
    };
    expect(buildSearch(state)).toBe("?board=trending");
  });

  it("writes q, topics, topic_mode, languages", () => {
    const s = buildSearch({
      board: "top",
      metric: "watchers",
      date: "2026-08-07",
      q: "  agent  ",
      topics: ["ai", "llm"],
      topicMode: "or",
      languages: ["Python", "TypeScript"],
    });
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("board")).toBe("top");
    expect(params.get("metric")).toBe("watchers");
    expect(params.get("date")).toBe("2026-08-07");
    expect(params.get("q")).toBe("agent");
    expect(params.get("topics")).toBe("ai,llm");
    expect(params.get("topic_mode")).toBe("or");
    expect(params.get("languages")).toBe("Python,TypeScript");
  });

  it("round-trips with parseSearch", () => {
    const original: UrlState = {
      board: "top",
      metric: "forks",
      date: "2026-08-01",
      q: "cli",
      topics: ["rust", "tui"],
      topicMode: "or",
      languages: ["Rust"],
    };
    expect(parseSearch(buildSearch(original))).toEqual(original);
  });

  it("round-trips board=tracked", () => {
    const original: UrlState = {
      board: "tracked",
      metric: "stars",
      date: "",
      q: "agent",
      topics: ["ai"],
      topicMode: "and",
      languages: ["Go"],
    };
    expect(parseSearch(buildSearch(original))).toEqual(original);
  });
});

describe("toggleInList", () => {
  it("adds when missing and removes when present", () => {
    expect(toggleInList(["a"], "b")).toEqual(["a", "b"]);
    expect(toggleInList(["a", "b"], "a")).toEqual(["b"]);
  });
});

describe("hasActiveFilters", () => {
  it("detects q / topics / languages", () => {
    expect(hasActiveFilters({ q: "", topics: [], languages: [] })).toBe(false);
    expect(hasActiveFilters({ q: "x", topics: [], languages: [] })).toBe(true);
    expect(hasActiveFilters({ q: "", topics: ["ai"], languages: [] })).toBe(true);
    expect(hasActiveFilters({ q: "", topics: [], languages: ["Go"] })).toBe(true);
  });
});
