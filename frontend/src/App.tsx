import { useEffect, useState } from "react";
import AuthCard from "./components/AuthCard";
import Leaderboard from "./components/Leaderboard";
import AdminPanel from "./components/AdminPanel";
import { api, startRefreshTimer } from "./api";
import type { MeResponse } from "./types";

type AuthState =
  | { kind: "loading" }
  | { kind: "anon" }
  | { kind: "user"; username: string; isAdmin: boolean };

type View = "board" | "admin";

export default function App() {
  const [auth, setAuth] = useState<AuthState>({ kind: "loading" });
  const [view, setView] = useState<View>("board");

  useEffect(() => {
    api<MeResponse>("/api/auth/me")
      .then((me) =>
        setAuth({
          kind: "user",
          username: me.username,
          isAdmin: !!me.is_admin,
        }),
      )
      .catch(() => setAuth({ kind: "anon" }));
  }, []);

  useEffect(() => {
    if (auth.kind !== "user") return;
    return startRefreshTimer(() => setAuth({ kind: "anon" }));
  }, [auth.kind]);

  if (auth.kind === "loading") {
    return <div className="p-8 text-neutral-500">Loading…</div>;
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
            });
          } catch {
            setAuth({ kind: "user", username, isAdmin: false });
          }
          setView("board");
        }}
      />
    );
  }

  if (view === "admin" && auth.isAdmin) {
    return <AdminPanel onBack={() => setView("board")} />;
  }

  return (
    <Leaderboard
      username={auth.username}
      isAdmin={auth.isAdmin}
      onOpenAdmin={() => setView("admin")}
      onLogout={async () => {
        await fetch("/api/auth/logout", { method: "POST", credentials: "include" });
        setAuth({ kind: "anon" });
        setView("board");
      }}
    />
  );
}
