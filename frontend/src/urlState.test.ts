import { describe, expect, it } from "vitest";
import {
  buildSearch,
  DEFAULT_URL_STATE,
  hasActiveFilters,
  hasDiscoverCondition,
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
    expect(s.dq).toBe("");
    expect(s.dlanguage).toBe("");
    expect(s.dlicense).toBe("");
    expect(s.dminStars).toBeNull();
    expect(s.dexcludeArchived).toBe(true);
    expect(s.dactiveWithin).toBeNull();
    expect(s.dsort).toBe("stars");
    expect(s.dpage).toBe(1);
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

  it("parses board=discover", () => {
    const s = parseSearch("?board=discover");
    expect(s.board).toBe("discover");
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

  it("parses discover d* params", () => {
    const s = parseSearch(
      "?board=discover&dq=http&dlanguage=Rust&dlicense=mit&dmin_stars=100&dexclude_archived=0&dactive_within=90&dsort=updated&dpage=2",
    );
    expect(s.board).toBe("discover");
    expect(s.dq).toBe("http");
    expect(s.dlanguage).toBe("Rust");
    expect(s.dlicense).toBe("mit");
    expect(s.dminStars).toBe(100);
    expect(s.dexcludeArchived).toBe(false);
    expect(s.dactiveWithin).toBe(90);
    expect(s.dsort).toBe("updated");
    expect(s.dpage).toBe(2);
  });

  it("clamps dpage to 1..=10", () => {
    expect(parseSearch("?dpage=0").dpage).toBe(1);
    expect(parseSearch("?dpage=11").dpage).toBe(10);
    expect(parseSearch("?dpage=abc").dpage).toBe(1);
  });

  it("defaults dsort to stars for unknown values", () => {
    expect(parseSearch("?dsort=forks").dsort).toBe("stars");
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
      dq: "",
      dlanguage: "",
      dlicense: "",
      dminStars: null,
      dexcludeArchived: true,
      dactiveWithin: null,
      dsort: "stars",
      dpage: 1,
    };
    expect(buildSearch(state)).toBe("?board=trending");
  });

  it("writes q, topics, topic_mode, languages, licenses", () => {
    const s = buildSearch({
      ...DEFAULT_URL_STATE,
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
      dq: "",
      dlanguage: "",
      dlicense: "",
      dminStars: null,
      dexcludeArchived: true,
      dactiveWithin: null,
      dsort: "stars",
      dpage: 1,
    };
    expect(parseSearch(buildSearch(original))).toEqual(original);
  });

  it("round-trips board=tracked with health filters", () => {
    const original: UrlState = {
      ...DEFAULT_URL_STATE,
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

  it("writes discover d* params only when board=discover", () => {
    const discover: UrlState = {
      ...DEFAULT_URL_STATE,
      board: "discover",
      dq: "http",
      dlanguage: "Rust",
      dlicense: "mit",
      dminStars: 50,
      dexcludeArchived: false,
      dactiveWithin: 90,
      dsort: "updated",
      dpage: 3,
    };
    const s = buildSearch(discover);
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("board")).toBe("discover");
    expect(params.get("dq")).toBe("http");
    expect(params.get("dlanguage")).toBe("Rust");
    expect(params.get("dlicense")).toBe("mit");
    expect(params.get("dmin_stars")).toBe("50");
    expect(params.get("dexclude_archived")).toBe("0");
    expect(params.get("dactive_within")).toBe("90");
    expect(params.get("dsort")).toBe("updated");
    expect(params.get("dpage")).toBe("3");
  });

  it("drops d* when leaving discover", () => {
    const leaving: UrlState = {
      ...DEFAULT_URL_STATE,
      board: "trending",
      dq: "http",
      dlanguage: "Rust",
      dminStars: 10,
      dpage: 2,
      dsort: "updated",
    };
    const s = buildSearch(leaving);
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("board")).toBe("trending");
    expect(params.get("dq")).toBeNull();
    expect(params.get("dlanguage")).toBeNull();
    expect(params.get("dmin_stars")).toBeNull();
    expect(params.get("dpage")).toBeNull();
    expect(params.get("dsort")).toBeNull();
  });

  it("omits default discover fields", () => {
    const s = buildSearch({
      ...DEFAULT_URL_STATE,
      board: "discover",
      dq: "cli",
    });
    const params = new URLSearchParams(s.slice(1));
    expect(params.get("dq")).toBe("cli");
    expect(params.get("dexclude_archived")).toBeNull();
    expect(params.get("dsort")).toBeNull();
    expect(params.get("dpage")).toBeNull();
  });

  it("round-trips discover state", () => {
    const original: UrlState = {
      ...DEFAULT_URL_STATE,
      board: "discover",
      dq: "agent",
      dlanguage: "Go",
      dlicense: "apache-2.0",
      dminStars: 0,
      dexcludeArchived: true,
      dactiveWithin: 30,
      dsort: "stars",
      dpage: 1,
    };
    // dpage=1 and dsort=stars and dexcludeArchived=true are omitted on build,
    // but parse defaults restore them.
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

describe("hasDiscoverCondition", () => {
  it("requires at least one of q/language/license/min_stars/active_within", () => {
    expect(
      hasDiscoverCondition({
        dq: "",
        dlanguage: "",
        dlicense: "",
        dminStars: null,
        dactiveWithin: null,
      }),
    ).toBe(false);
    expect(
      hasDiscoverCondition({
        dq: "  ",
        dlanguage: "",
        dlicense: "",
        dminStars: null,
        dactiveWithin: null,
      }),
    ).toBe(false);
    expect(
      hasDiscoverCondition({
        dq: "cli",
        dlanguage: "",
        dlicense: "",
        dminStars: null,
        dactiveWithin: null,
      }),
    ).toBe(true);
    expect(
      hasDiscoverCondition({
        dq: "",
        dlanguage: "Rust",
        dlicense: "",
        dminStars: null,
        dactiveWithin: null,
      }),
    ).toBe(true);
    expect(
      hasDiscoverCondition({
        dq: "",
        dlanguage: "",
        dlicense: "mit",
        dminStars: null,
        dactiveWithin: null,
      }),
    ).toBe(true);
    expect(
      hasDiscoverCondition({
        dq: "",
        dlanguage: "",
        dlicense: "",
        dminStars: 0,
        dactiveWithin: null,
      }),
    ).toBe(true);
    expect(
      hasDiscoverCondition({
        dq: "",
        dlanguage: "",
        dlicense: "",
        dminStars: null,
        dactiveWithin: 90,
      }),
    ).toBe(true);
  });
});
