import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  api,
  ApiError,
  UnauthorizedError,
  compact,
  deleteGithubToken,
  discoverSearch,
  listTrackedRepos,
  lookupRepo,
  parseRepoRef,
  putGithubToken,
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
      licenses: ["MIT"],
      topicMode: "or",
    });
    const url = String(fetchMock.mock.calls[0][0]);
    expect(url.startsWith("/api/repos/tracked?")).toBe(true);
    expect(url).toContain("q=cli");
    expect(url).toContain("topics=ai%2Cllm");
    expect(url).toContain("languages=Rust");
    expect(url).toContain("licenses=MIT");
    expect(url).toContain("topic_mode=or");
    // Default excludeArchived not sent (server default true).
    expect(url).not.toContain("exclude_archived");
  });

  it("listTrackedRepos sends health filters", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ items: [] }), { status: 200 }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await listTrackedRepos({
      excludeArchived: false,
      activeWithin: 90,
    });
    const url = String(fetchMock.mock.calls[0][0]);
    expect(url).toContain("exclude_archived=0");
    expect(url).toContain("active_within=90");
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

describe("discoverSearch", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("builds query string from params", async () => {
    const payload = {
      items: [],
      page: 1,
      per_page: 30,
      total_count: 0,
      incomplete_results: false,
      auth_mode: "shared",
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify(payload), { status: 200 }),
      );
    vi.stubGlobal("fetch", fetchMock);

    await discoverSearch({
      q: "http",
      language: "Rust",
      license: "mit",
      minStars: 100,
      excludeArchived: false,
      activeWithin: 90,
      sort: "updated",
      page: 2,
    });
    const url = String(fetchMock.mock.calls[0][0]);
    expect(url.startsWith("/api/discover/search?")).toBe(true);
    expect(url).toContain("q=http");
    expect(url).toContain("language=Rust");
    expect(url).toContain("license=mit");
    expect(url).toContain("min_stars=100");
    expect(url).toContain("exclude_archived=0");
    expect(url).toContain("active_within=90");
    expect(url).toContain("sort=updated");
    expect(url).toContain("page=2");
  });

  it("omits default sort/page and default exclude_archived", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            items: [],
            page: 1,
            per_page: 30,
            total_count: 0,
            incomplete_results: false,
            auth_mode: "user",
          }),
          { status: 200 },
        ),
      );
    vi.stubGlobal("fetch", fetchMock);

    await discoverSearch({ q: "cli", sort: "stars", page: 1 });
    const url = String(fetchMock.mock.calls[0][0]);
    expect(url).toContain("q=cli");
    expect(url).not.toContain("sort=");
    expect(url).not.toContain("page=");
    expect(url).not.toContain("exclude_archived");
  });

  it("surfaces 429 with body fields on ApiError", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          error: "rate_limited",
          scope: "global",
          auth_mode: "shared",
          retry_after_secs: 12,
        }),
        { status: 429 },
      ),
    );
    vi.stubGlobal("fetch", fetchMock);

    try {
      await discoverSearch({ language: "Go" });
      expect.fail("should throw");
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      const err = e as ApiError;
      expect(err.status).toBe(429);
      expect(err.message).toBe("rate_limited");
      expect(err.body).toMatchObject({
        error: "rate_limited",
        scope: "global",
        retry_after_secs: 12,
      });
    }
  });
});

describe("github token helpers", () => {
  beforeEach(() => vi.restoreAllMocks());
  afterEach(() => vi.restoreAllMocks());

  it("putGithubToken PUTs body and returns status", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(JSON.stringify({ has_github_token: true }), { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);

    const res = await putGithubToken("ghp_test");
    expect(res.has_github_token).toBe(true);
    expect(fetchMock.mock.calls[0][0]).toBe("/api/me/github-token");
    expect(fetchMock.mock.calls[0][1]?.method).toBe("PUT");
    const body = JSON.parse(String(fetchMock.mock.calls[0][1]?.body));
    expect(body.token).toBe("ghp_test");
  });

  it("deleteGithubToken DELETEs", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(JSON.stringify({ has_github_token: false }), { status: 200 }),
    );
    vi.stubGlobal("fetch", fetchMock);

    const res = await deleteGithubToken();
    expect(res.has_github_token).toBe(false);
    expect(fetchMock.mock.calls[0][0]).toBe("/api/me/github-token");
    expect(fetchMock.mock.calls[0][1]?.method).toBe("DELETE");
  });
});
