import { useEffect, useRef, useState } from "react";
import {
  ApiError,
  deleteGithubToken,
  putGithubToken,
  UnauthorizedError,
} from "../api";

interface Props {
  open: boolean;
  hasGithubToken: boolean;
  onClose: () => void;
  onChanged: (hasToken: boolean) => void;
  onUnauthorized?: () => void;
}

/**
 * Save / clear personal GitHub PAT for discover Search quota isolation.
 * Never displays the stored token (server does not return it).
 */
export default function GithubTokenModal({
  open,
  hasGithubToken,
  onClose,
  onChanged,
  onUnauthorized,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [token, setToken] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);

  useEffect(() => {
    if (!open) return;
    setToken("");
    setError(null);
    setSaving(false);
    setClearing(false);
    const t = window.setTimeout(() => inputRef.current?.focus(), 40);
    return () => window.clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  async function onSave() {
    const trimmed = token.trim();
    if (!trimmed) {
      setError("请粘贴 GitHub Personal Access Token");
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const res = await putGithubToken(trimmed);
      onChanged(!!res.has_github_token);
      setToken("");
      onClose();
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        if (e.message === "invalid github token") {
          setError("Token 无效或已失效，请检查后重试");
        } else if (e.message === "token is required") {
          setError("Token 不能为空");
        } else {
          setError(e.message || `保存失败（HTTP ${e.status}）`);
        }
        return;
      }
      setError(e instanceof Error ? e.message : "保存失败");
    } finally {
      setSaving(false);
    }
  }

  async function onClear() {
    setClearing(true);
    setError(null);
    try {
      const res = await deleteGithubToken();
      onChanged(!!res.has_github_token);
      setToken("");
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        setError(e.message || `清除失败（HTTP ${e.status}）`);
        return;
      }
      setError(e instanceof Error ? e.message : "清除失败");
    } finally {
      setClearing(false);
    }
  }

  const busy = saving || clearing;

  return (
    <div
      className="modal-backdrop open"
      role="dialog"
      aria-modal="true"
      aria-labelledby="ghTokenModalTitle"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal wide">
        <button type="button" className="close" onClick={onClose}>
          关闭
        </button>
        <h2 id="ghTokenModalTitle">GitHub Token</h2>
        <p className="sub">
          配置个人 PAT 后，发现搜索走你的配额（
          <code>auth_mode=user</code>
          ），不占用站点共享桶；未配置时回退服务端{" "}
          <code>GITHUB_TOKEN</code>。本站<strong>从不</strong>回显已保存的
          Token，也不用它跑全站采集。
        </p>

        <div className="field">
          <label htmlFor="ghTokenInput">Personal Access Token</label>
          <input
            id="ghTokenInput"
            ref={inputRef}
            className="field-input"
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy && token.trim()) {
                e.preventDefault();
                void onSave();
              }
            }}
            placeholder={
              hasGithubToken
                ? "已配置 — 粘贴新 Token 可覆盖"
                : "ghp_… 或 fine-grained token"
            }
            autoComplete="off"
            spellCheck={false}
            disabled={busy}
          />
          <p className="hint">
            公开库只读即可（classic <code>public_repo</code> 或 fine-grained
            Contents/Metadata 只读）。不访问私有库。
          </p>
          <div className="field-error" role="alert">
            {error ?? ""}
          </div>
        </div>

        <p
          className="text-sm"
          style={{ color: "var(--text-3)", margin: "0 0 12px" }}
        >
          状态：
          {hasGithubToken ? (
            <span style={{ color: "var(--link)", fontWeight: 600 }}>
              {" "}
              已配置个人 Token
            </span>
          ) : (
            <span> 未配置（发现将使用共享配额，若已配置服务端 Token）</span>
          )}
        </p>

        <div className="modal-actions">
          <button
            type="button"
            className="btn btn-ghost"
            onClick={onClose}
            disabled={busy}
          >
            取消
          </button>
          {hasGithubToken && (
            <button
              type="button"
              className="btn"
              onClick={() => void onClear()}
              disabled={busy}
            >
              {clearing ? "清除中…" : "清除 Token"}
            </button>
          )}
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void onSave()}
            disabled={busy || !token.trim()}
          >
            {saving ? "保存中…" : hasGithubToken ? "更新 Token" : "保存 Token"}
          </button>
        </div>
      </div>
    </div>
  );
}
