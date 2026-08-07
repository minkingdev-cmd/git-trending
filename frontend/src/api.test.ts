import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  api,
  ApiError,
  UnauthorizedError,
  compact,
  listTrackedRepos,
  lookupRepo,
  parseRepoRef,
  trackRepo,
  untrackRepo,
} from "./api";

describe("compact", () => {
  it("formats numbers compactly", () => {
    expect(compact(190000)).toBe("190K");
    expect(compact(950)).toBe("950");
  });
});

describe("parseRepoRef", () => {
  it("parses owner/name and github URLs", () => {
    expect(parseRepoRef("owner/name")).toBe("owner/name");
    expect(parseRepoRef("  acme/widget  ")).toBe("acme/widget");
    expect(parseRepoRef("https://github.com/a/b")).toBe("a/b");
    expect(parseRepoRef("https://github.com/a/b.git")).toBe("a/b");
    expect(parseRepoRef("http://www.github.com/Foo/Bar")).toBe("Foo/Bar");
    expect(parseRepoRef("github.com/x/y")).toBe("x/y");
  });

  it("rejects non-github hosts and bad shapes", () => {
    expect(parseRepoRef("https://evil.com/a/b")).toBeNull();
    expect(parseRepoRef("")).toBeNull();
    expect(parseRepoRef("onlyowner")).toBeNull();
    expect(parseRepoRef("a/b/c")).toBeNull();
  });
});

describe("api 401 handling", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("retries once after successful refresh", async () => {
    const fetchMock = vi
      .fn()
      // first business request 401
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      // refresh succeeds
      .mockResolvedValueOnce(new Response(null, { status: 200 }))
      // replay succeeds
      .mockResolvedValueOnce(new Response(JSON.stringify({ ok: 1 }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const data = await api<{ ok: number }>("/api/leaderboard/trending");
    expect(data.ok).toBe(1);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock.mock.calls[1][0]).toBe("/api/auth/refresh");
  });

  it("throws UnauthorizedError when refresh fails", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 401 }))
      .mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/leaderboard/trending")).rejects.toBeInstanceOf(UnauthorizedError);
  });

  it("does not retry 401 on auth endpoints", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(new Response(null, { status: 401 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/auth/me")).rejects.toBeInstanceOf(UnauthorizedError);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("surfaces ApiError with body message", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(JSON.stringify({ error: "repo not found or private" }), {
        status: 404,
      }),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(api("/api/repos/lookup")).rejects.toMatchObject({
      name: "ApiError",
      status: 404,
      message: "repo not found or private",
    });
  });
});

describe("track helpers", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("lookupRepo posts body", async () => {
    const payload = {
      full_name: "a/b",
      html_url: "https://github.com/a/b",
      description: null,
      license: "MIT",
      languages: [],
      topics: [],
      stars: 1,
      forks: 0,
      watchers: 0,
      on_leaderboard: false,
      already_tracked: false,
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(JSON.stringify(payload), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    const data = await lookupRepo({ full_name: "a/b" });
    expect(data.full_name).toBe("a/b");
    expect(fetchMock.mock.calls[0][0]).toBe("/api/repos/lookup");
    expect(fetchMock.mock.calls[0][1]?.method).toBe("POST");
  });

  it("trackRepo returns item", async () => {
    const item = {
      full_name: "a/b",
      html_url: "https://github.com/a/b",
      description: null,
      license: "MIT",
      languages: [],
      topics: [],
      stars: 1,
      forks: 0,
      watchers: null,
      status: "pending",
      added_at: "2026-08-07T00:00:00Z",
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ item }), { status: 201 }),
      );
    vi.stubGlobal("fetch", fetchMock);

    const data = await trackRepo({ full_name: "a/b" });
    expect(data.full_name).toBe("a/b");
    expect(fetchMock.mock.calls[0][0]).toBe("/api/repos/track");
  });

  it("untrackRepo tolerates 204", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);

    await expect(untrackRepo("a/b")).resolves.toBeUndefined();
    expect(String(fetchMock.mock.calls[0][0])).toContain("full_name=a%2Fb");
    expect(fetchMock.mock.calls[0][1]?.method).toBe("DELETE");
  });

  it("listTrackedRepos builds query string", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ items: [] }), { status: 200 }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await listTrackedRepos({
      q: "cli",
      topics: ["ai", "llm"],
      languages: ["Rust"],
      topicMode: "or",
    });
    const url = String(fetchMock.mock.calls[0][0]);
    expect(url.startsWith("/api/repos/tracked?")).toBe(true);
    expect(url).toContain("q=cli");
    expect(url).toContain("topics=ai%2Cllm");
    expect(url).toContain("languages=Rust");
    expect(url).toContain("topic_mode=or");
  });

  it("throws ApiError on track limit", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(
        JSON.stringify({ error: "tracked repo limit reached", limit: 50 }),
        { status: 409 },
      ),
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(trackRepo({ full_name: "a/b" })).rejects.toBeInstanceOf(ApiError);
  });
});
