import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type BackTarget = "none" | "backspace" | "escape";

type KeyMapSettings = {
  voice_to_right_option: boolean;
  home_to_escape: boolean;
  tv_to_backspace: boolean;
  back_target: BackTarget;
};

type Settings = {
  audio_device: string;
  key_map: KeyMapSettings;
  auto_start_engine: boolean;
  launch_at_login: boolean;
  verbose_logs: boolean;
  record_last_session: boolean;
};

type SessionSnapshot = {
  state: string;
  mic_open: boolean;
  audio_frames: number;
  pcm_samples: number;
  gate_gain: number;
  caps: unknown;
};

type EngineStatus = {
  running: boolean;
  state: string;
  message?: string | null;
  device?: string | null;
  session: SessionSnapshot;
  audio_device: string;
  key_backend: string;
  key_supported: boolean;
};

type LogEntry = { timestamp: string; level: string; message: string };

type Snapshot = {
  settings: Settings;
  status: EngineStatus;
  devices: { name: string; is_default: boolean }[];
  logs: LogEntry[];
  default_device: string;
  platform: string;
  last_recording: string | null;
  accessibility_trusted: boolean;
};

const number = new Intl.NumberFormat("zh-CN");

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.8,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
  };
  switch (name) {
    case "mic":
      return (
        <svg {...common}>
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0M12 18v3M8 21h8" />
        </svg>
      );
    case "bluetooth":
      return (
        <svg {...common}>
          <path d="M7 7l10 10-5 4V3l5 4L7 17" />
        </svg>
      );
    case "keyboard":
      return (
        <svg {...common}>
          <rect x="2.5" y="6" width="19" height="12" rx="2.5" />
          <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6" />
        </svg>
      );
    case "speaker":
      return (
        <svg {...common}>
          <path d="M4 10v4h3l4 3V7L7 10H4z" />
          <path d="M15 9a4 4 0 0 1 0 6M17.5 6.5a8 8 0 0 1 0 11" />
        </svg>
      );
    case "power":
      return (
        <svg {...common}>
          <path d="M12 3v9" />
          <path d="M6.4 6.4a8 8 0 1 0 11.2 0" />
        </svg>
      );
    case "refresh":
      return (
        <svg {...common}>
          <path d="M20 11a8 8 0 0 0-14.9-4M4 5v5h5" />
          <path d="M4 13a8 8 0 0 0 14.9 4M20 19v-5h-5" />
        </svg>
      );
    case "check":
      return (
        <svg {...common}>
          <path d="M20 6L9 17l-5-5" />
        </svg>
      );
    case "x":
      return (
        <svg {...common}>
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      );
    case "wave":
      return (
        <svg {...common}>
          <path d="M2 12h2l2-6 3 12 3-16 3 16 2-6h5" />
        </svg>
      );
    case "settings":
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.9.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.9 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.9.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.9-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.9V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
        </svg>
      );
    default:
      return null;
  }
}

function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  hint?: string;
}) {
  return (
    <label className="toggle-row">
      <span>
        <span className="toggle-label">{label}</span>
        {hint && <span className="toggle-hint">{hint}</span>}
      </span>
      <span className={`switch ${checked ? "on" : ""}`}>
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
        />
        <span className="knob" />
      </span>
    </label>
  );
}

function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLElement>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners: (() => void)[] = [];

    (async () => {
      const initial = await invoke<Snapshot>("get_snapshot");
      if (disposed) return;
      setSnap(initial);
      setDraft(initial.settings);
      unlisteners.push(
        await listen<EngineStatus>("status", (event) => {
          setSnap((prev) => (prev ? { ...prev, status: event.payload } : prev));
        }),
      );
    })().catch((error) => setToast(String(error)));

    const timer = window.setInterval(async () => {
      try {
        const next = await invoke<Snapshot>("get_snapshot");
        if (disposed) return;
        setSnap((prev) =>
          prev
            ? {
                ...prev,
                status: next.status,
                devices: next.devices,
                logs: next.logs,
              }
            : next,
        );
      } catch {
        /* the app may be shutting down */
      }
    }, 2500);

    return () => {
      disposed = true;
      window.clearInterval(timer);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    gridRef.current?.scrollTo({ top: 0 });
  }, []);

  // After the user grants Accessibility in System Settings, retry the tap
  // automatically instead of making them press "应用映射" again.
  useEffect(() => {
    if (snap?.accessibility_trusted) {
      invoke("apply_key_mapping").catch(() => {});
    }
  }, [snap?.accessibility_trusted]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [snap?.logs.length]);

  const dirty = useMemo(() => {
    if (!snap || !draft) return false;
    return JSON.stringify(snap.settings) !== JSON.stringify(draft);
  }, [snap, draft]);

  if (!snap || !draft) {
    return (
      <div className="loading">
        <div className="loading-mark">
          <Icon name="wave" size={28} />
        </div>
        <p>正在启动 Xiaomi Remote…</p>
      </div>
    );
  }

  const { status } = snap;
  const running = status.running;
  const recording = status.session.mic_open;
  const stateTone = recording
    ? "recording"
    : status.state === "connected"
      ? "ok"
      : status.state === "error"
        ? "bad"
        : running
          ? "busy"
          : "idle";

  const stateLabel = recording
    ? "录音中"
    : status.state === "connected"
      ? "已连接"
      : status.state === "scanning"
        ? "扫描中"
        : status.state === "connecting"
          ? "连接中"
          : status.state === "starting"
            ? "启动中"
            : status.state === "error"
              ? "错误"
              : running
                ? "运行中"
                : "已停止";

  const deviceReady = snap.devices.some(
    (d) => d.name.toLowerCase() === draft.audio_device.toLowerCase(),
  );
  const hintPresent = snap.devices.some((d) =>
    d.name.toLowerCase().includes(snap.default_device.toLowerCase()),
  );

  function patch(patchValue: Partial<Settings>) {
    setDraft((prev) => (prev ? { ...prev, ...patchValue } : prev));
  }

  function patchKeys(patchValue: Partial<KeyMapSettings>) {
    setDraft((prev) =>
      prev ? { ...prev, key_map: { ...prev.key_map, ...patchValue } } : prev,
    );
  }

  async function refresh() {
    const next = await invoke<Snapshot>("get_snapshot");
    setSnap(next);
    setDraft(next.settings);
  }

  async function toggleEngine() {
    setBusy(true);
    try {
      await invoke(running ? "stop" : "start");
      await refresh();
    } catch (error) {
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function save() {
    if (!draft) return;
    setBusy(true);
    try {
      const next = await invoke<Snapshot>("update_settings", {
        settings: draft,
      });
      setSnap(next);
      setDraft(next.settings);
      setToast("设置已保存");
    } catch (error) {
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function toggleLogin(value: boolean) {
    setBusy(true);
    try {
      const next = await invoke<Snapshot>("set_launch_at_login", {
        enabled: value,
      });
      setSnap(next);
      setDraft(next.settings);
    } catch (error) {
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function applyKeys() {
    setBusy(true);
    try {
      await invoke("apply_key_mapping");
      setToast("按键映射已应用");
      await refresh();
    } catch (error) {
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function openAccessibility() {
    try {
      await invoke("open_accessibility_settings");
    } catch (error) {
      setToast(String(error));
    }
  }

  async function openRecording() {
    try {
      await invoke("open_last_recording");
    } catch (error) {
      setToast(String(error));
    }
  }

  async function clearKeys() {
    setBusy(true);
    try {
      await invoke("clear_key_mapping");
      setToast("按键映射已清除");
      await refresh();
    } catch (error) {
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  const checklist = [
    {
      label: `已安装虚拟音频设备（${snap.default_device}）`,
      ok: hintPresent,
      detail: hintPresent
        ? "已检测到"
        : "macOS 用 BlackHole 2ch，Windows 用 VB-CABLE，Linux 用 PipeWire null sink",
    },
    {
      label: "已选择输出设备",
      ok: deviceReady,
      detail: deviceReady ? draft.audio_device : `当前选择：${draft.audio_device}`,
    },
    {
      label: "遥控器已连接",
      ok: Boolean(status.device) && status.state === "connected",
      detail: status.device ?? "等待 BLE 连接",
    },
    {
      label: "辅助功能权限",
      ok: snap.accessibility_trusted,
      detail: snap.accessibility_trusted
        ? "已授权，可发送干净的右 Option"
        : "未授权时只能回退到 F5+Option",
    },
    {
      label: "按键映射可用",
      ok: status.key_supported,
      detail: status.key_supported ? status.key_backend : "当前平台暂不支持",
    },
  ];

  return (
    <div className="app">
      <div className="glow glow-a" />
      <div className="glow glow-b" />

      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">
            <Icon name="wave" size={22} />
          </div>
          <div>
            <h1>Xiaomi Remote</h1>
            <p>蓝牙语音遥控器 · V2</p>
          </div>
        </div>

        <div className="topbar-actions">
          <span className={`state-pill ${stateTone}`}>
            <span className="dot" />
            {stateLabel}
          </span>
          <button
            className={`primary ${running ? "stop" : ""}`}
            onClick={toggleEngine}
            disabled={busy}
          >
            <Icon name="power" size={17} />
            {running ? "停止桥接" : "启动桥接"}
          </button>
        </div>
      </header>

      {status.message && (
        <div className={`banner ${status.state === "error" ? "bad" : ""}`}>
          {status.message}
        </div>
      )}

      <main className="grid" ref={gridRef}>
        <section className="column">
          <article className="card device-card">
            <div className="card-head">
              <div className="card-title">
                <Icon name="bluetooth" />
                <h2>遥控器</h2>
              </div>
              <span className={`mini-pill ${stateTone}`}>{stateLabel}</span>
            </div>

            <div className="remote-hero">
              <div className={`remote-orb ${recording ? "live" : ""}`}>
                <Icon name={recording ? "mic" : "bluetooth"} size={30} />
                {recording && <span className="pulse-ring" />}
              </div>
              <div>
                <div className="remote-name">{status.device ?? "等待遥控器"}</div>
                <div className="remote-sub">
                  {recording
                    ? "正在接收麦克风音频"
                    : running
                      ? "BLE 语音桥接运行中"
                      : "桥接已停止"}
                </div>
              </div>
            </div>

            <div className="metrics">
              <div className="metric">
                <span>音频帧</span>
                <strong>{number.format(status.session.audio_frames)}</strong>
              </div>
              <div className="metric">
                <span>采样数</span>
                <strong>{number.format(status.session.pcm_samples)}</strong>
              </div>
              <div className="metric">
                <span>噪声门</span>
                <strong>{status.session.gate_gain.toFixed(2)}</strong>
              </div>
            </div>
          </article>

          <article className="card">
            <div className="card-head">
              <div className="card-title">
                <Icon name="speaker" />
                <h2>音频输出</h2>
              </div>
              <button className="ghost icon-only" onClick={refresh} title="刷新设备">
                <Icon name="refresh" size={16} />
              </button>
            </div>

            <label className="field">
              <span>虚拟麦克风 / 输出设备</span>
              <select
                value={draft.audio_device}
                onChange={(e) => patch({ audio_device: e.target.value })}
              >
                {!deviceReady && (
                  <option value={draft.audio_device}>
                    {draft.audio_device}（未找到）
                  </option>
                )}
                {snap.devices.map((device) => (
                  <option key={device.name} value={device.name}>
                    {device.name}
                    {device.is_default ? "（系统默认）" : ""}
                  </option>
                ))}
              </select>
            </label>

            <p className="hint">
              需要 48 kHz / 2ch 的虚拟音频设备。macOS 推荐 BlackHole 2ch，
              Windows 推荐 VB-CABLE。
            </p>

            <div className="record-row">
              <Toggle
                label="保存每次录音"
                hint="写入 last_session.wav，便于回放验证"
                checked={draft.record_last_session}
                onChange={(v) => patch({ record_last_session: v })}
              />
              <button
                className="ghost"
                onClick={openRecording}
                disabled={!snap.last_recording}
                title={snap.last_recording ?? "还没有录音"}
              >
                播放上次录音
              </button>
            </div>
          </article>

          <article className="card">
            <div className="card-head">
              <div className="card-title">
                <Icon name="settings" />
                <h2>准备检查</h2>
              </div>
              <span className="mini-pill idle">
                {checklist.filter((item) => item.ok).length}/{checklist.length}
              </span>
            </div>

            {!snap.accessibility_trusted && (
              <button className="ghost" onClick={openAccessibility}>
                打开辅助功能设置
              </button>
            )}

            <ul className="checklist">
              {checklist.map((item) => (
                <li key={item.label}>
                  <span className={`check ${item.ok ? "ok" : "bad"}`}>
                    <Icon name={item.ok ? "check" : "x"} size={13} />
                  </span>
                  <span>
                    <span className="check-label">{item.label}</span>
                    <span className="check-detail">{item.detail}</span>
                  </span>
                </li>
              ))}
            </ul>

            <div className="login-row">
              <Toggle
                label="登录时自动启动"
                hint="通过系统登录项启动 App"
                checked={draft.launch_at_login}
                onChange={toggleLogin}
              />
            </div>
          </article>
        </section>

        <section className="column">
          <article className="card">
            <div className="card-head">
              <div className="card-title">
                <Icon name="keyboard" />
                <h2>按键映射</h2>
              </div>
              <span className={`mini-pill ${status.key_supported ? "ok" : "idle"}`}>
                {status.key_backend}
              </span>
            </div>

            <div className="toggle-list">
              <Toggle
                label="语音键 → 右 Option"
                hint="Karabiner 里叫 right_alt"
                checked={draft.key_map.voice_to_right_option}
                onChange={(v) => patchKeys({ voice_to_right_option: v })}
              />
              <Toggle
                label="Home → Escape"
                checked={draft.key_map.home_to_escape}
                onChange={(v) => patchKeys({ home_to_escape: v })}
              />
              <Toggle
                label="TV → Backspace"
                checked={draft.key_map.tv_to_backspace}
                onChange={(v) => patchKeys({ tv_to_backspace: v })}
              />
            </div>

            <label className="field compact">
              <span>返回键（0xF1）</span>
              <select
                value={draft.key_map.back_target}
                onChange={(e) =>
                  patchKeys({ back_target: e.target.value as BackTarget })
                }
              >
                <option value="none">不映射</option>
                <option value="backspace">Backspace</option>
                <option value="escape">Escape</option>
              </select>
            </label>

            <div className="button-row">
              <button className="ghost" onClick={applyKeys} disabled={busy}>
                应用映射
              </button>
              <button className="ghost danger" onClick={clearKeys} disabled={busy}>
                清除
              </button>
            </div>
          </article>

          <article className="card logs-card">
            <div className="card-head">
              <div className="card-title">
                <Icon name="wave" />
                <h2>运行日志</h2>
              </div>
              <span className="mini-pill idle">{snap.logs.length} 条</span>
            </div>
            <div className="logs" ref={logRef}>
              {snap.logs.length === 0 && (
                <div className="log-empty">暂无日志，点击“启动桥接”开始。</div>
              )}
              {snap.logs.map((entry, index) => (
                <div className={`log-line ${entry.level}`} key={`${entry.timestamp}-${index}`}>
                  <span className="log-time">{entry.timestamp}</span>
                  <span className="log-level">{entry.level}</span>
                  <span className="log-message">{entry.message}</span>
                </div>
              ))}
            </div>
          </article>
        </section>
      </main>

      <footer className="footer">
        <div className="footer-left">
          <span>V2.0.0</span>
          <span className="sep">·</span>
          <span>{snap.platform}</span>
          <span className="sep">·</span>
          <span>音频链路 16 kHz → 48 kHz</span>
        </div>
        <div className="footer-right">
          {dirty && <span className="dirty">有未保存的更改</span>}
          <button className="ghost" onClick={save} disabled={!dirty || busy}>
            保存设置
          </button>
        </div>
      </footer>

      {toast && (
        <div className="toast" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}

export default App;
