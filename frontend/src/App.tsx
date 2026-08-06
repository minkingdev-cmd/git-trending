import { useEffect, useState } from "react";
import AuthCard from "./components/AuthCard";
import Leaderboard from "./components/Leaderboard";
import { api, startRefreshTimer } from "./api";
import type { MeResponse } from "./types";

type AuthState =
  | { kind: "loading" }
  | { kind: "anon" }
  | { kind: "user"; username: string };

export default function App() {
  const [auth, setAuth] = useState<AuthState>({ kind: "loading" });

  useEffect(() => {
    api<MeResponse>("/api/auth/me")
      .then((me) => setAuth({ kind: "user", username: me.username }))
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
    return <AuthCard onLoggedIn={(username) => setAuth({ kind: "user", username })} />;
  }
  return (
    <Leaderboard
      username={auth.username}
      onLogout={async () => {
        await fetch("/api/auth/logout", { method: "POST", credentials: "include" });
        setAuth({ kind: "anon" });
      }}
    />
  );
}
