import { useCallback, useEffect, useState } from "react";
import { api, postAuth, UnauthorizedError } from "../api";
import type { InviteItem, UserItem } from "../types";

interface Props {
  onBack: () => void;
  onUnauthorized?: () => void;
}

export default function AdminPanel({ onBack, onUnauthorized }: Props) {
  const [invites, setInvites] = useState<InviteItem[]>([]);
  const [users, setUsers] = useState<UserItem[]>([]);
  const [uses, setUses] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [inv, usr] = await Promise.all([
        api<InviteItem[]>("/api/admin/invites"),
        api<UserItem[]>("/api/admin/users"),
      ]);
      setInvites(inv);
      setUsers(usr);
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      setError(e instanceof Error ? e.message : "load failed");
    }
  }, [onUnauthorized]);

  useEffect(() => {
    void load();
  }, [load]);

  async function createInvite() {
    setBusy(true);
    setCreated(null);
    setError(null);
    try {
      const res = await postAuth("/api/admin/invites", { max_uses: uses });
      if (!res.ok) {
        const data = await res.json().catch(() => ({}));
        throw new Error((data as { error?: string }).error ?? `HTTP ${res.status}`);
      }
      const data = (await res.json()) as { code: string };
      setCreated(data.code);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : "create failed");
    } finally {
      setBusy(false);
    }
  }

  async function revoke(code: string) {
    setBusy(true);
    setError(null);
    try {
      const res = await postAuth(`/api/admin/invites/${encodeURIComponent(code)}/revoke`);
      if (!res.ok) {
        throw new Error(`HTTP ${res.status}`);
      }
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : "revoke failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen bg-neutral-950 text-neutral-100">
      <div className="mx-auto max-w-3xl space-y-6 p-6">
        <header className="flex items-center justify-between">
          <h1 className="text-xl font-semibold">管理后台</h1>
          <button
            type="button"
            onClick={onBack}
            className="text-sm text-neutral-400 hover:text-white"
          >
            返回榜单
          </button>
        </header>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <section className="space-y-3 rounded-lg border border-neutral-800 p-4">
          <h2 className="text-sm font-medium text-neutral-300">邀请码</h2>
          <div className="flex flex-wrap items-center gap-2">
            <label className="text-sm text-neutral-400">
              可用次数
              <input
                type="number"
                min={1}
                value={uses}
                onChange={(e) => setUses(Number(e.target.value) || 1)}
                className="ml-2 w-16 rounded border border-neutral-700 bg-neutral-900 px-2 py-1"
              />
            </label>
            <button
              type="button"
              disabled={busy}
              onClick={() => void createInvite()}
              className="rounded bg-emerald-600 px-3 py-1 text-sm hover:bg-emerald-500 disabled:opacity-50"
            >
              生成邀请码
            </button>
          </div>
          {created && (
            <p className="text-sm text-emerald-400">
              已创建：<code className="rounded bg-neutral-900 px-1">{created}</code>
            </p>
          )}
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-neutral-800 text-left text-neutral-500">
                <th className="py-2">Code</th>
                <th className="py-2">使用</th>
                <th className="py-2">状态</th>
                <th className="py-2" />
              </tr>
            </thead>
            <tbody>
              {invites.map((i) => (
                <tr key={i.code} className="border-b border-neutral-900">
                  <td className="py-2 font-mono text-xs">{i.code}</td>
                  <td className="py-2">
                    {i.used_count}/{i.max_uses}
                  </td>
                  <td className="py-2">{i.revoked ? "revoked" : "active"}</td>
                  <td className="py-2 text-right">
                    {!i.revoked && (
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void revoke(i.code)}
                        className="text-xs text-red-400 hover:text-red-300"
                      >
                        作废
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {invites.length === 0 && (
                <tr>
                  <td colSpan={4} className="py-4 text-center text-neutral-500">
                    暂无邀请码
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </section>

        <section className="space-y-3 rounded-lg border border-neutral-800 p-4">
          <h2 className="text-sm font-medium text-neutral-300">用户</h2>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-neutral-800 text-left text-neutral-500">
                <th className="py-2">ID</th>
                <th className="py-2">用户名</th>
                <th className="py-2">角色</th>
                <th className="py-2">创建时间</th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id} className="border-b border-neutral-900">
                  <td className="py-2 text-neutral-500">{u.id}</td>
                  <td className="py-2">{u.username}</td>
                  <td className="py-2">{u.is_admin ? "admin" : "user"}</td>
                  <td className="py-2 text-neutral-500 text-xs">
                    {new Date(u.created_at).toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </div>
    </div>
  );
}
