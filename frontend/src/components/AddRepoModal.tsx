import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import {
  ApiError,
  compact,
  lookupRepo,
  parseRepoRef,
  repoRefBodyFromInput,
  trackRepo,
  UnauthorizedError,
} from "../api";
import type { LookupResponse, TrackedRepoItem } from "../types";

interface Props {
  open: boolean;
  onClose: () => void;
  onTracked: (item: TrackedRepoItem) => void;
  onUnauthorized?: () => void;
}

export default function AddRepoModal({
  open,
  onClose,
  onTracked,
  onUnauthorized,
}: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<LookupResponse | null>(null);
  const [lookingUp, setLookingUp] = useState(false);
  const [tracking, setTracking] = useState(false);

  useEffect(() => {
    if (!open) return;
    setInput("");
    setError(null);
    setPreview(null);
    setLookingUp(false);
    setTracking(false);
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

  async function runLookup() {
    setError(null);
    setPreview(null);
    const body = repoRefBodyFromInput(input);
    if (!body || !parseRepoRef(input)) {
      setError("无法解析：请输入 owner/name 或 https://github.com/owner/name");
      return;
    }
    setLookingUp(true);
    try {
      const data = await lookupRepo(body);
      setPreview(data);
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        if (e.status === 404) {
          setError("仓库不存在或为私有库");
        } else {
          setError(e.message || `查找失败（HTTP ${e.status}）`);
        }
        return;
      }
      setError(e instanceof Error ? e.message : "查找失败");
    } finally {
      setLookingUp(false);
    }
  }

  async function submitTrack() {
    if (!preview || preview.already_tracked) return;
    const body = repoRefBodyFromInput(preview.full_name) ?? {
      full_name: preview.full_name,
    };
    setTracking(true);
    setError(null);
    try {
      const item = await trackRepo(body);
      onTracked(item);
      onClose();
    } catch (e) {
      if (e instanceof UnauthorizedError) {
        onUnauthorized?.();
        return;
      }
      if (e instanceof ApiError) {
        if (e.status === 409) {
          setError("已达跟踪上限（最多 50 个仓库）");
        } else if (e.status === 404) {
          setError("仓库不存在或为私有库");
        } else {
          setError(e.message || `加入跟踪失败（HTTP ${e.status}）`);
        }
        return;
      }
      setError(e instanceof Error ? e.message : "加入跟踪失败");
    } finally {
      setTracking(false);
    }
  }

  function onInputKeyDown(e: ReactKeyboardEvent<HTMLInputElement>) {
    if (e.key !== "Enter") return;
    e.preventDefault();
    if (preview && !preview.already_tracked && !tracking) {
      void submitTrack();
    } else if (!lookingUp) {
      void runLookup();
    }
  }

  let statusClass = "pv-status";
  let statusText = "";
  if (preview) {
    if (preview.already_tracked) {
      statusClass += " tracking";
      statusText = "你已在跟踪此仓库";
    } else if (preview.on_leaderboard) {
      statusClass += " on-board";
      statusText = "已在今日榜内 — 仍可加入「我的跟踪」方便固定关注";
    } else {
      statusClass += " pending";
      statusText = "未在爬虫当日榜内 — 加入后进入跟踪队列与每日快照";
    }
  }

  const canTrack = Boolean(preview && !preview.already_tracked && !tracking);

  return (
    <div
      className="modal-backdrop open"
      role="dialog"
      aria-modal="true"
      aria-labelledby="addModalTitle"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal wide">
        <button type="button" className="close" onClick={onClose}>
          关闭
        </button>
        <h2 id="addModalTitle">添加仓库</h2>
        <p className="sub">
          跟踪爬虫未覆盖或未进当日榜的 GitHub 仓库。校验后写入跟踪列表，并纳入后续每日快照。
        </p>

        <div className="field">
          <label htmlFor="addRepoInput">仓库地址或 full_name</label>
          <input
            id="addRepoInput"
            ref={inputRef}
            className="field-input"
            type="text"
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              // Clear stale preview when input changes after a successful lookup.
              if (preview) setPreview(null);
            }}
            onKeyDown={onInputKeyDown}
            placeholder="例如 owner/name 或 https://github.com/owner/name"
            autoComplete="off"
            spellCheck={false}
            disabled={lookingUp || tracking}
          />
          <p className="hint">仅支持公开仓库。私有库、不存在或重定向无效时会失败。</p>
          <div className="field-error" role="alert">
            {error ?? ""}
          </div>
        </div>

        {preview && (
          <div className="preview-card show">
            <div className="pv-name">{preview.full_name}</div>
            <div className="pv-desc">{preview.description || "无描述"}</div>
            <div className="pv-meta">
              <span>{compact(preview.stars)} ★</span>
              <span>{compact(preview.forks)} ⑂</span>
              <span>{compact(preview.watchers ?? 0)} 👁</span>
              <span>
                {(preview.languages ?? [])
                  .map((l) => l.name)
                  .slice(0, 3)
                  .join(" · ") || "—"}
              </span>
            </div>
            <div className={statusClass}>{statusText}</div>
          </div>
        )}

        <div className="modal-actions">
          <button type="button" className="btn btn-ghost" onClick={onClose}>
            取消
          </button>
          <button
            type="button"
            className="btn"
            onClick={() => void runLookup()}
            disabled={lookingUp || tracking || !input.trim()}
          >
            {lookingUp ? "解析中…" : "解析预览"}
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => void submitTrack()}
            disabled={!canTrack}
          >
            {tracking
              ? "加入中…"
              : preview?.already_tracked
                ? "已在跟踪"
                : "加入跟踪"}
          </button>
        </div>
      </div>
    </div>
  );
}
