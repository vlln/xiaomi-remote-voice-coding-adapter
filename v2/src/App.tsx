import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type BackTarget = "none" | "backspace" | "escape";
type KeyAction = "voice" | "home" | "tv" | "back";
type Page = "overview" | "keys" | "audio" | "advanced";

type KeySource = { usage: number; name?: string };
type KeyMapSettings = {
  voice_to_right_option: boolean;
  home_to_escape: boolean;
  tv_to_backspace: boolean;
  back_target: BackTarget;
  voice_key: KeySource;
  home_key: KeySource;
  tv_key: KeySource;
  back_key: KeySource;
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
  key_mapping_full: boolean;
  key_capture_supported: boolean;
};

type CapturedKey = { page: number; usage: number; name: string };

const number = new Intl.NumberFormat("zh-CN");

const DEFAULT_KEYS: KeyMapSettings = {
  voice_to_right_option: true,
  home_to_escape: true,
  tv_to_backspace: true,
  back_target: "backspace",
  voice_key: { usage: 0x3e, name: "F5" },
  home_key: { usage: 0x4a, name: "Home" },
  tv_key: { usage: 0x35, name: "`" },
  back_key: { usage: 0xf1, name: "Back (AC Back)" },
};

const ACTIONS: KeyAction[] = ["voice", "home", "tv", "back"];

const ACTION_META: Record<
  KeyAction,
  { label: string; desc: string; target: string; icon: string }
> = {
  voice: {
    label: "语音键",
    desc: "按住说话，发送右 Option",
    target: "右 Option",
    icon: "mic",
  },
  home: {
    label: "Home 键",
    desc: "遥控器主页键",
    target: "Escape",
    icon: "home",
  },
  tv: {
    label: "TV 键",
    desc: "遥控器电视键",
    target: "Backspace",
    icon: "tv",
  },
  back: {
    label: "返回键",
    desc: "遥控器返回键",
    target: "可配置",
    icon: "back",
  },
};

function Icon({ name, size = 18 }: { name: string; size?: number }) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
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
    case "home":
      return (
        <svg {...common}>
          <path d="M3 10.5L12 3l9 7.5" />
          <path d="M5 9.5V21h14V9.5" />
        </svg>
      );
    case "tv":
      return (
        <svg {...common}>
          <rect x="3" y="5" width="18" height="12" rx="2" />
          <path d="M8 21h8M12 17v4" />
        </svg>
      );
    case "back":
      return (
        <svg {...common}>
          <path d="M19 12H5M11 18l-6-6 6-6" />
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
    case "shield":
      return (
        <svg {...common}>
          <path d="M12 3l7 3v6c0 4.5-3 7.5-7 9-4-1.5-7-4.5-7-9V6l7-3z" />
          <path d="M9 12l2 2 4-4" />
        </svg>
      );
    case "sliders":
      return (
        <svg {...common}>
          <path d="M4 7h10M18 7h2M4 17h2M10 17h10" />
          <circle cx="16" cy="7" r="2" />
          <circle cx="8" cy="17" r="2" />
        </svg>
      );
    case "wave":
      return (
        <svg {...common}>
          <path d="M2 12h2l2-6 3 12 3-16 3 16 2-6h5" />
        </svg>
      );
    case "record":
      return (
        <svg {...common}>
          <circle cx="12" cy="12" r="8" />
          <circle cx="12" cy="12" r="3" fill="currentColor" stroke="none" />
        </svg>
      );
    case "play":
      return (
        <svg {...common}>
          <path d="M8 5l11 7-11 7V5z" />
        </svg>
      );
    default:
      return null;
  }
}

function Switch({
  checked,
  onChange,
  disabled,
  label,
}: {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  label?: string;
}) {
  return (
    <label className={`switch ${disabled ? "disabled" : ""}`} title={label}>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="switch-track">
        <span className="switch-knob" />
      </span>
    </label>
  );
}

function sourceLabel(source: KeySource) {
  return (
    source.name?.trim() ||
    `Key 0x${source.usage.toString(16).toUpperCase()}`
  );
}

function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [page, setPage] = useState<Page>("overview");
  const [capturing, setCapturing] = useState<KeyAction | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);

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
                last_recording: next.last_recording,
                accessibility_trusted: next.accessibility_trusted,
                key_mapping_full: next.key_mapping_full,
                key_capture_supported: next.key_capture_supported,
              }
            : next,
        );
      } catch {
        /* shutting down */
      }
    }, 2500);

    return () => {
      disposed = true;
      window.clearInterval(timer);
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (snap?.accessibility_trusted) {
      invoke("apply_key_mapping").catch(() => {});
    }
  }, [snap?.accessibility_trusted]);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [snap?.logs.length, page]);

  useEffect(() => {
    if (!capturing) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        invoke("cancel_key_capture").catch(() => {});
        setCapturing(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [capturing]);

  const dirty = useMemo(() => {
    if (!snap || !draft) return false;
    return JSON.stringify(snap.settings) !== JSON.stringify(draft);
  }, [snap, draft]);

  if (!snap || !draft) {
    return (
      <div className="loading">
        <span className="loading-mark">
          <Icon name="wave" size={24} />
        </span>
        <span>正在启动 Xiaomi Remote…</span>
      </div>
    );
  }

  const { status } = snap;
  const running = status.running;
  const recording = status.session.mic_open;
  const tone = recording
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
    (device) => device.name.toLowerCase() === draft.audio_device.toLowerCase(),
  );
  const hintPresent = snap.devices.some((device) =>
    device.name.toLowerCase().includes(snap.default_device.toLowerCase()),
  );
  const connected = Boolean(status.device) && status.state === "connected";

  const steps = [
    {
      key: "mic",
      title: "虚拟麦克风",
      desc: hintPresent
        ? `${snap.default_device} 已就绪`
        : "安装 BlackHole / VB-CABLE 后选择它作为输出",
      ok: hintPresent,
      action: hintPresent
        ? null
        : {
            label: "安装 BlackHole",
            run: () => openUrl("https://existential.audio/blackhole/"),
          },
    },
    {
      key: "shield",
      title: "键盘控制权限",
      desc: snap.accessibility_trusted
        ? "已授权，可发送干净的右 Option"
        : "未授权时语音键会回退到 F5+Option",
      ok: snap.accessibility_trusted,
      action: snap.accessibility_trusted
        ? null
        : { label: "打开设置", run: openAccessibility },
    },
    {
      key: "bluetooth",
      title: "遥控器连接",
      desc: connected ? (status.device ?? "已连接") : "等待 BLE 连接",
      ok: connected,
      action: connected ? null : { label: "启动桥接", run: toggleEngine },
    },
  ];
  const readyCount = steps.filter((step) => step.ok).length;

  function patch(patchValue: Partial<Settings>) {
    setDraft((prev) => (prev ? { ...prev, ...patchValue } : prev));
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

  async function saveKeys(
    patchValue: Partial<KeyMapSettings>,
    message?: string,
  ) {
    if (!draft) return;
    const previous = draft;
    const next = {
      ...draft,
      key_map: { ...draft.key_map, ...patchValue },
    };
    setDraft(next);
    setBusy(true);
    try {
      const nextSnap = await invoke<Snapshot>("update_settings", {
        settings: next,
      });
      setSnap(nextSnap);
      setDraft(nextSnap.settings);
      if (message) setToast(message);
    } catch (error) {
      setDraft(previous);
      setToast(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function capture(action: KeyAction) {
    if (!draft || busy) return;
    setCapturing(action);
    try {
      const key = await invoke<CapturedKey>("capture_key");
      const conflict = ACTIONS.find(
        (other) =>
          other !== action &&
          draft.key_map[`${other}_key`].usage === key.usage,
      );
      if (conflict) {
        setToast(
          `这个按键已经用于「${ACTION_META[conflict].label}」，请换一个`,
        );
        return;
      }
      await saveKeys(
        { [`${action}_key`]: { usage: key.usage, name: key.name } },
        `已录制 ${key.name}`,
      );
    } catch (error) {
      setToast(String(error));
    } finally {
      setCapturing(null);
    }
  }

  function cancelCapture() {
    invoke("cancel_key_capture").catch(() => {});
    setCapturing(null);
  }

  async function resetKeys() {
    await saveKeys(DEFAULT_KEYS, "已恢复默认按键");
  }

  async function openAccessibility() {
    try {
      await invoke("open_accessibility_settings");
    } catch (error) {
      setToast(String(error));
    }
  }

  async function openUrl(url: string) {
    try {
      await invoke("open_url", { url });
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

  function go(next: Page) {
    setPage(next);
    pageRef.current?.scrollTo({ top: 0 });
  }

  const pageMeta: Record<Page, { title: string; subtitle: string }> = {
    overview: { title: "概览", subtitle: "遥控器、麦克风和权限状态" },
    keys: {
      title: "按键映射",
      subtitle: "点击按键，然后按下遥控器上的对应键",
    },
    audio: { title: "音频", subtitle: "虚拟麦克风与录音" },
    advanced: { title: "高级", subtitle: "权限、启动项与运行日志" },
  };

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <span className="brand-mark">
            <Icon name="wave" size={20} />
          </span>
          <span>
            <strong>Xiaomi Remote</strong>
            <small>V2.1 · 菜单栏应用</small>
          </span>
        </div>

        <nav className="nav">
          <button
            className={`nav-item ${page === "overview" ? "active" : ""}`}
            onClick={() => go("overview")}
          >
            <Icon name="wave" size={16} />
            概览
            {readyCount < 3 && <span className="nav-badge">{3 - readyCount}</span>}
          </button>
          <button
            className={`nav-item ${page === "keys" ? "active" : ""}`}
            onClick={() => go("keys")}
          >
            <Icon name="keyboard" size={16} />
            按键映射
          </button>
          <button
            className={`nav-item ${page === "audio" ? "active" : ""}`}
            onClick={() => go("audio")}
          >
            <Icon name="speaker" size={16} />
            音频
          </button>
          <button
            className={`nav-item ${page === "advanced" ? "active" : ""}`}
            onClick={() => go("advanced")}
          >
            <Icon name="sliders" size={16} />
            高级
          </button>
        </nav>

        <div className="sidebar-foot">
          <div className={`status-line ${tone}`}>
            <span className="status-dot" />
            <span>{stateLabel}</span>
          </div>
          <div className="sidebar-version">{snap.platform} · 2.1.0</div>
        </div>
      </aside>

      <main className="content">
        <header className="page-head">
          <div>
            <h1>{pageMeta[page].title}</h1>
            <p>{pageMeta[page].subtitle}</p>
          </div>
          <div className="page-actions">
            <span className={`status-chip ${tone}`}>
              <span className="status-dot" />
              {stateLabel}
            </span>
            <button
              className={`btn ${running ? "btn-ghost" : "btn-primary"}`}
              onClick={toggleEngine}
              disabled={busy}
            >
              <Icon name="power" size={15} />
              {running ? "停止" : "启动"}
            </button>
          </div>
        </header>

        {status.message && (
          <div className={`notice ${status.state === "error" ? "bad" : ""}`}>
            {status.message}
          </div>
        )}

        <div className="page-scroll" ref={pageRef}>
          {page === "overview" && (
            <div className="page">
              <section className="hero">
                <div className={`hero-orb ${recording ? "live" : ""}`}>
                  <Icon name={recording ? "mic" : "bluetooth"} size={26} />
                </div>
                <div className="hero-info">
                  <span className="eyebrow">遥控器</span>
                  <h2>{status.device ?? "等待连接"}</h2>
                  <p>
                    {recording
                      ? "正在接收麦克风音频"
                      : running
                        ? "BLE 语音桥接运行中"
                        : "桥接已停止，点击右上角启动"}
                  </p>
                </div>
                <div className="hero-metrics">
                  <div>
                    <span>音频帧</span>
                    <strong>{number.format(status.session.audio_frames)}</strong>
                  </div>
                  <div>
                    <span>采样数</span>
                    <strong>{number.format(status.session.pcm_samples)}</strong>
                  </div>
                  <div>
                    <span>噪声门</span>
                    <strong>{status.session.gate_gain.toFixed(2)}</strong>
                  </div>
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>快速设置</h3>
                    <p>完成这三步即可开始使用</p>
                  </div>
                  <span
                    className={`count ${readyCount === steps.length ? "ok" : ""}`}
                  >
                    {readyCount}/{steps.length}
                  </span>
                </div>
                <div className="steps">
                  {steps.map((step) => (
                    <div className={`step ${step.ok ? "ok" : ""}`} key={step.key}>
                      <span className="step-icon">
                        <Icon name={step.ok ? "check" : step.key} size={16} />
                      </span>
                      <div className="step-body">
                        <strong>{step.title}</strong>
                        <span>{step.desc}</span>
                      </div>
                      {step.action && (
                        <button className="btn btn-small" onClick={step.action.run}>
                          {step.action.label}
                        </button>
                      )}
                    </div>
                  ))}
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>按键</h3>
                    <p>
                      {status.key_backend} ·{" "}
                      {snap.key_mapping_full ? "完整映射" : "仅语音键"}
                    </p>
                  </div>
                  <button className="btn btn-small" onClick={() => go("keys")}>
                    配置
                  </button>
                </div>
                <div className="mini-keys">
                  {ACTIONS.map((action) => (
                    <div className="mini-key" key={action}>
                      <span>{ACTION_META[action].label}</span>
                      <code>{sourceLabel(draft.key_map[`${action}_key`])}</code>
                    </div>
                  ))}
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>最近日志</h3>
                    <p>最新 {Math.min(snap.logs.length, 4)} 条</p>
                  </div>
                  <button
                    className="btn btn-small"
                    onClick={() => go("advanced")}
                  >
                    全部日志
                  </button>
                </div>
                <LogList logs={snap.logs.slice(-4)} />
              </section>
            </div>
          )}

          {page === "keys" && (
            <div className="page">
              <section className="section keys-section">
                <div className="section-head">
                  <div>
                    <h3>遥控器按键</h3>
                    <p>点击右侧按键，然后按下遥控器上的对应键；Esc 取消</p>
                  </div>
                  <button
                    className="btn btn-small"
                    onClick={resetKeys}
                    disabled={busy}
                  >
                    恢复默认
                  </button>
                </div>

                <div className="key-list">
                  {ACTIONS.map((action) => {
                    const meta = ACTION_META[action];
                    const source = draft.key_map[`${action}_key`];
                    const enabled =
                      action === "voice"
                        ? draft.key_map.voice_to_right_option
                        : action === "home"
                          ? draft.key_map.home_to_escape
                          : action === "tv"
                            ? draft.key_map.tv_to_backspace
                            : draft.key_map.back_target !== "none";
                    const disabled =
                      !snap.key_mapping_full && action !== "voice";
                    return (
                      <div
                        className={`key-row ${disabled ? "is-disabled" : ""}`}
                        key={action}
                      >
                        <div className="key-main">
                          <span className="key-icon">
                            <Icon name={meta.icon} size={17} />
                          </span>
                          <div>
                            <strong>{meta.label}</strong>
                            <span>{meta.desc}</span>
                          </div>
                        </div>
                        <div className="key-controls">
                          {action === "back" ? (
                            <select
                              className="select compact"
                              value={draft.key_map.back_target}
                              disabled={disabled || busy}
                              onChange={(event) =>
                                saveKeys(
                                  {
                                    back_target: event.target
                                      .value as BackTarget,
                                  },
                                  "返回键目标已更新",
                                )
                              }
                            >
                              <option value="backspace">Backspace</option>
                              <option value="escape">Escape</option>
                              <option value="none">不映射</option>
                            </select>
                          ) : (
                            <span className="target">{meta.target}</span>
                          )}
                          <button
                            className={`shortcut ${
                              capturing === action ? "capturing" : ""
                            }`}
                            disabled={disabled || busy || !snap.key_capture_supported}
                            onClick={() => capture(action)}
                            title="点击重新录制"
                          >
                            <Icon name="keyboard" size={13} />
                            {capturing === action
                              ? "按下按键…"
                              : sourceLabel(source)}
                          </button>
                          <Switch
                            checked={enabled}
                            disabled={disabled || busy}
                            label={`${meta.label}启用`}
                            onChange={(value) => {
                              if (action === "back") {
                                saveKeys({
                                  back_target: value ? "backspace" : "none",
                                });
                              } else {
                                const field =
                                  action === "voice"
                                    ? "voice_to_right_option"
                                    : action === "home"
                                      ? "home_to_escape"
                                      : "tv_to_backspace";
                                saveKeys({ [field]: value });
                              }
                            }}
                          />
                        </div>
                      </div>
                    );
                  })}
                </div>

                {!snap.key_mapping_full && (
                  <p className="inline-note">
                    Windows 默认只支持语音键；Home / TV / 返回需要
                    Interception 驱动和可选构建。
                  </p>
                )}
                <p className="inline-note">
                  {snap.key_capture_supported
                    ? "录制复用 macOS「键盘控制权限」。如果点击后无法录制，请在系统设置 → 隐私与安全性 → Device Control and Data Access 中重新授权 Xiaomi Remote。"
                    : "当前平台暂不支持录制按键；可以继续使用默认绑定。"}
                </p>
              </section>
            </div>
          )}

          {page === "audio" && (
            <div className="page">
              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>虚拟麦克风</h3>
                    <p>语音桥接会把 16 kHz 音频重采样到 48 kHz 输出</p>
                  </div>
                  <button className="btn btn-small" onClick={refresh}>
                    <Icon name="refresh" size={14} />
                    刷新
                  </button>
                </div>

                <label className="field">
                  <span>输出设备</span>
                  <select
                    className="select"
                    value={draft.audio_device}
                    onChange={(event) =>
                      patch({ audio_device: event.target.value })
                    }
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

                <div className="format-row">
                  <span className="format-pill">48 kHz</span>
                  <span className="format-pill">2 ch</span>
                  <span className="format-pill">16 → 48 kHz</span>
                </div>

                <p className="inline-note">
                  macOS 推荐 BlackHole 2ch，Windows 推荐 VB-CABLE，Linux 使用
                  PipeWire null sink。安装说明见 README。
                </p>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>录音</h3>
                    <p>只保留最近一次会话，便于回放验证</p>
                  </div>
                </div>
                <div className="row">
                  <div className="row-main">
                    <Icon name="record" size={17} />
                    <div>
                      <strong>保存每次录音</strong>
                      <span>写入 last_session.wav</span>
                    </div>
                  </div>
                  <Switch
                    checked={draft.record_last_session}
                    label="保存每次录音"
                    onChange={(value) => patch({ record_last_session: value })}
                  />
                </div>
                <div className="row">
                  <div className="row-main">
                    <Icon name="play" size={17} />
                    <div>
                      <strong>上次录音</strong>
                      <span>{snap.last_recording ?? "还没有录音"}</span>
                    </div>
                  </div>
                  <button
                    className="btn btn-small"
                    onClick={openRecording}
                    disabled={!snap.last_recording}
                  >
                    播放
                  </button>
                </div>
              </section>
            </div>
          )}

          {page === "advanced" && (
            <div className="page">
              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>权限</h3>
                    <p>发送干净的按键事件需要系统授权</p>
                  </div>
                  <span
                    className={`status-chip ${
                      snap.accessibility_trusted ? "ok" : "bad"
                    }`}
                  >
                    <span className="status-dot" />
                    {snap.accessibility_trusted ? "已授权" : "未授权"}
                  </span>
                </div>
                <div className="row">
                  <div className="row-main">
                    <Icon name="shield" size={17} />
                    <div>
                      <strong>键盘控制权限</strong>
                      <span>
                        {snap.accessibility_trusted
                          ? "EventTap 可发送右 Option 并吞掉 F5"
                          : "未授权时语音键回退到 F5+Option"}
                      </span>
                    </div>
                  </div>
                  <button className="btn btn-small" onClick={openAccessibility}>
                    打开设置
                  </button>
                </div>
                <div className="row">
                  <div className="row-main">
                    <Icon name="power" size={17} />
                    <div>
                      <strong>登录时自动启动</strong>
                      <span>通过系统登录项启动菜单栏 App</span>
                    </div>
                  </div>
                  <Switch
                    checked={draft.launch_at_login}
                    label="登录时自动启动"
                    onChange={toggleLogin}
                  />
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>运行日志</h3>
                    <p>{snap.logs.length} 条</p>
                  </div>
                </div>
                <LogList logs={snap.logs} scrollRef={logRef} />
              </section>

              <section className="section about">
                <div className="about-grid">
                  <div>
                    <span>版本</span>
                    <strong>2.1.0</strong>
                  </div>
                  <div>
                    <span>平台</span>
                    <strong>{snap.platform}</strong>
                  </div>
                  <div>
                    <span>按键后端</span>
                    <strong>{status.key_backend}</strong>
                  </div>
                  <div>
                    <span>音频链路</span>
                    <strong>16 kHz → 48 kHz</strong>
                  </div>
                </div>
              </section>
            </div>
          )}
        </div>

        <footer className="savebar">
          <span className="savebar-hint">
            {dirty ? "有未保存的更改" : "所有更改已保存"}
          </span>
          <div className="savebar-actions">
            {dirty && (
              <button
                className="btn btn-primary"
                onClick={save}
                disabled={busy}
              >
                保存设置
              </button>
            )}
          </div>
        </footer>
      </main>

      {capturing && (
        <div className="capture-overlay">
          <div className="capture-card">
            <span className="capture-key">
              <Icon name="keyboard" size={30} />
            </span>
            <h2>按下遥控器上的按键</h2>
            <p>
              正在录制「{ACTION_META[capturing].label}」，按下后立即生效
            </p>
            <span className="capture-timer">10 秒后自动取消</span>
            <button className="btn btn-ghost" onClick={cancelCapture}>
              取消
            </button>
          </div>
        </div>
      )}

      {toast && (
        <div className="toast" onClick={() => setToast(null)}>
          {toast}
        </div>
      )}
    </div>
  );
}

function LogList({
  logs,
  scrollRef,
}: {
  logs: LogEntry[];
  scrollRef?: React.RefObject<HTMLDivElement | null>;
}) {
  return (
    <div className={`log ${scrollRef ? "log-tall" : ""}`} ref={scrollRef}>
      {logs.length === 0 && <div className="log-empty">暂无日志</div>}
      {logs.map((entry, index) => (
        <div
          className={`log-line ${entry.level}`}
          key={`${entry.timestamp}-${index}`}
        >
          <span className="log-time">{entry.timestamp}</span>
          <span className="log-level">{entry.level}</span>
          <span className="log-message">{entry.message}</span>
        </div>
      ))}
    </div>
  );
}

export default App;
