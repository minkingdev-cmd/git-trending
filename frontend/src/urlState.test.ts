import { describe, expect, it } from "vitest";
import {
  buildSearch,
  DEFAULT_URL_STATE,
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
    expect(s.excludeArchived).toBe(true);
    expect(s.activeWithin).toBeNull();
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

  it("defaults excludeArchived true and activeWithin null", () => {
    const s = parseSearch("?board=trending");
    expect(s.excludeArchived).toBe(true);
    expect(s.activeWithin).toBeNull();
  });

  it("parses exclude_archived=0 as false", () => {
    expect(parseSearch("?exclude_archived=0").excludeArchived).toBe(false);
    expect(parseSearch("?exclude_archived=false").excludeArchived).toBe(false);
    expect(parseSearch("?exclude_archived=no").excludeArchived).toBe(false);
  });

  it("parses exclude_archived=1/true as true", () => {
    expect(parseSearch("?exclude_archived=1").excludeArchived).toBe(true);
    expect(parseSearch("?exclude_archived=true").excludeArchived).toBe(true);
  });

  it("parses active_within days", () => {
    expect(parseSearch("?active_within=90").activeWithin).toBe(90);
    expect(parseSearch("?active_within=7").activeWithin).toBe(7);
  });

  it("ignores invalid or non-positive active_within", () => {
    expect(parseSearch("?active_within=0").activeWithin).toBeNull();
    expect(parseSearch("?active_within=-5").activeWithin).toBeNull();
    expect(parseSearch("?active_within=abc").activeWithin).toBeNull();
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
      licenses: [],
      excludeArchived: true,
      activeWithin: null,
    };
    expect(buildSearch(state)).toBe("?board=trending");
  });

  it("writes q, topics, topic_mode, languages, licenses", () => {
    const s = buildSearch({
      board: "top",
      metric: "watchers",
      date: "2026-08-07",
      q: "  agent  ",
      topics: ["ai", "llm"],
      topicMode: "or",
      languages: ["Python", "TypeScript"],
      licenses: ["MIT", "Apache-2.0"],
      excludeArchived: true,
      activeWithin: null,
    });
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("board")).toBe("top");
    expect(params.get("metric")).toBe("watchers");
    expect(params.get("date")).toBe("2026-08-07");
    expect(params.get("q")).toBe("agent");
    expect(params.get("topics")).toBe("ai,llm");
    expect(params.get("topic_mode")).toBe("or");
    expect(params.get("languages")).toBe("Python,TypeScript");
    expect(params.get("licenses")).toBe("MIT,Apache-2.0");
    expect(params.get("exclude_archived")).toBeNull();
    expect(params.get("active_within")).toBeNull();
  });

  it("writes exclude_archived=0 when disabled", () => {
    const s = buildSearch({
      ...DEFAULT_URL_STATE,
      excludeArchived: false,
    });
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("exclude_archived")).toBe("0");
  });

  it("writes active_within when set", () => {
    const s = buildSearch({
      ...DEFAULT_URL_STATE,
      activeWithin: 90,
    });
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("active_within")).toBe("90");
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
      licenses: ["MIT"],
      excludeArchived: true,
      activeWithin: null,
    };
    expect(parseSearch(buildSearch(original))).toEqual(original);
  });

  it("round-trips board=tracked with health filters", () => {
    const original: UrlState = {
      board: "tracked",
      metric: "stars",
      date: "",
      q: "agent",
      topics: ["ai"],
      topicMode: "and",
      languages: ["Go"],
      licenses: [],
      excludeArchived: false,
      activeWithin: 90,
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
  it("detects q / topics / languages / licenses", () => {
    expect(
      hasActiveFilters({ q: "", topics: [], languages: [], licenses: [] }),
    ).toBe(false);
    expect(
      hasActiveFilters({ q: "x", topics: [], languages: [], licenses: [] }),
    ).toBe(true);
    expect(
      hasActiveFilters({ q: "", topics: ["ai"], languages: [], licenses: [] }),
    ).toBe(true);
    expect(
      hasActiveFilters({ q: "", topics: [], languages: ["Go"], licenses: [] }),
    ).toBe(true);
    expect(
      hasActiveFilters({ q: "", topics: [], languages: [], licenses: ["MIT"] }),
    ).toBe(true);
  });
});
