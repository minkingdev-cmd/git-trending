import { useCallback, useEffect, useState } from "react";
import AuthCard from "./components/AuthCard";
import Leaderboard from "./components/Leaderboard";
import AdminPanel from "./components/AdminPanel";
import { api, startRefreshTimer } from "./api";
import type { MeResponse } from "./types";

type AuthState =
  | { kind: "loading" }
  | { kind: "anon" }
  | {
      kind: "user";
      username: string;
      isAdmin: boolean;
      hasGithubToken: boolean;
    };

type View = "board" | "admin";

export default function App() {
  const [auth, setAuth] = useState<AuthState>({ kind: "loading" });
  const [view, setView] = useState<View>("board");

  const forceLogout = useCallback(() => {
    setAuth({ kind: "anon" });
    setView("board");
  }, []);

  useEffect(() => {
    api<MeResponse>("/api/auth/me")
      .then((me) =>
        setAuth({
          kind: "user",
          username: me.username,
          isAdmin: !!me.is_admin,
          hasGithubToken: !!me.has_github_token,
        }),
      )
      .catch(() => setAuth({ kind: "anon" }));
  }, []);

  useEffect(() => {
    if (auth.kind !== "user") return;
    return startRefreshTimer(forceLogout);
  }, [auth.kind, forceLogout]);

  if (auth.kind === "loading") {
    return (
      <div
        className="min-h-screen flex items-center justify-center"
        style={{ color: "var(--muted)", background: "var(--bg)" }}
      >
        Loading…
      </div>
    );
  }
  if (auth.kind === "anon") {
    return (
      <AuthCard
        onLoggedIn={async (username) => {
          try {
            const me = await api<MeResponse>("/api/auth/me");
            setAuth({
              kind: "user",
              username: me.username || username,
              isAdmin: !!me.is_admin,
              hasGithubToken: !!me.has_github_token,
            });
          } catch {
            setAuth({
              kind: "user",
              username,
              isAdmin: false,
              hasGithubToken: false,
            });
          }
          setView("board");
        }}
      />
    );
  }

  if (view === "admin" && auth.isAdmin) {
    return (
      <AdminPanel
        onBack={() => setView("board")}
        onUnauthorized={forceLogout}
      />
    );
  }

  return (
    <Leaderboard
      username={auth.username}
      isAdmin={auth.isAdmin}
      hasGithubToken={auth.hasGithubToken}
      onHasGithubTokenChange={(has) =>
        setAuth((prev) =>
          prev.kind === "user" ? { ...prev, hasGithubToken: has } : prev,
        )
      }
      onOpenAdmin={() => setView("admin")}
      onUnauthorized={forceLogout}
      onLogout={async () => {
        try {
          await fetch("/api/auth/logout", { method: "POST", credentials: "include" });
        } finally {
          forceLogout();
        }
      }}
    />
  );
}
