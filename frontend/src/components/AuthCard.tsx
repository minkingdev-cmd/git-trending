import { useState, type FormEvent } from "react";
import { postAuth } from "../api";

type Mode = "login" | "register";

export default function AuthCard({ onLoggedIn }: { onLoggedIn: (username: string) => void }) {
  const [mode, setMode] = useState<Mode>("login");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [inviteCode, setInviteCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const path = mode === "login" ? "/api/auth/login" : "/api/auth/register";
    const body =
      mode === "login"
        ? { username, password }
        : { username, password, invite_code: inviteCode };
    try {
      const res = await postAuth(path, body);
      if (res.ok) {
        onLoggedIn(username);
      } else {
        const data = await res.json().catch(() => ({}));
        setError((data as { error?: string }).error ?? `HTTP ${res.status}`);
      }
    } catch {
      setError("network error");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-neutral-950 text-neutral-100">
      <form onSubmit={submit} className="w-80 space-y-3 rounded-lg border border-neutral-800 p-6">
        <h1 className="text-lg font-semibold">GH Trending</h1>
        <input
          className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
          placeholder="username"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          required
        />
        <input
          className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
          placeholder="password (min 8 chars)"
          type="password"
          minLength={8}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        {mode === "register" && (
          <input
            className="w-full rounded border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm"
            placeholder="invite code"
            value={inviteCode}
            onChange={(e) => setInviteCode(e.target.value)}
            required
          />
        )}
        {error && <p className="text-sm text-red-400">{error}</p>}
        <button
          className="w-full rounded bg-emerald-600 py-2 text-sm font-medium hover:bg-emerald-500 disabled:opacity-50"
          disabled={busy}
        >
          {mode === "login" ? "Sign in" : "Create account"}
        </button>
        <button
          type="button"
          className="w-full text-sm text-neutral-400 hover:text-neutral-200"
          onClick={() => setMode(mode === "login" ? "register" : "login")}
        >
          {mode === "login" ? "No account? Register with invite code" : "Have an account? Sign in"}
        </button>
      </form>
    </div>
  );
}
