import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type BackTarget = "none" | "backspace" | "escape";
type KeyAction = "voice" | "home" | "tv" | "menu" | "back";
type Page = "overview" | "keys" | "audio" | "advanced";
type Guide = "mic" | "permission" | "remote";

type KeySource = { usage: number; name?: string };
type KeyMapSettings = {
  voice_to_right_option: boolean;
  home_to_escape: boolean;
  tv_to_backspace: boolean;
  back_target: BackTarget;
  menu_to_context: boolean;
  voice_target: KeySource;
  menu_target: KeySource;
  home_target: KeySource;
  tv_target: KeySource;
  back_target_key: KeySource;
};

type AudioProcessing = {
  noise_reduction: boolean;
  noise_gate: boolean;
  gain_db: number;
};

type Settings = {
  audio_device: string;
  key_map: KeyMapSettings;
  auto_start_engine: boolean;
  launch_at_login: boolean;
  verbose_logs: boolean;
  record_last_session: boolean;
  audio_processing: AudioProcessing;
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
  version: string;
  last_recording: string | null;
  accessibility_trusted: boolean;
  key_mapping_full: boolean;
  key_capture_supported: boolean;
};

type CapturedKey = { page: number; usage: number; name: string };

type UpdateInfo = {
  current: string;
  latest: string;
  available: boolean;
  url: string;
  asset_url: string | null;
  asset_name: string | null;
  notes: string | null;
  source: string;
};

const number = new Intl.NumberFormat("zh-CN");

const DEFAULT_KEYS: KeyMapSettings = {
  voice_to_right_option: true,
  home_to_escape: true,
  tv_to_backspace: true,
  back_target: "backspace",
  menu_to_context: true,
  voice_target: { usage: 0xe6, name: "Right Option" },
  menu_target: { usage: 0x65, name: "Menu" },
  home_target: { usage: 0x29, name: "Esc" },
  tv_target: { usage: 0x2a, name: "Backspace" },
  back_target_key: { usage: 0x2a, name: "Backspace" },
};

const ACTIONS: KeyAction[] = ["voice", "home", "tv", "menu", "back"];
const BUILTIN_SOURCES: Record<KeyAction, number> = {
  voice: 0x3e,
  home: 0x4a,
  tv: 0x35,
  menu: 0x65,
  back: 0xf1,
};

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
    desc: "固定来源：遥控器 Home",
    target: "Esc",
    icon: "home",
  },
  tv: {
    label: "TV 键",
    desc: "固定来源：遥控器 TV",
    target: "退格",
    icon: "tv",
  },
  menu: {
    label: "菜单键",
    desc: "固定来源：遥控器菜单",
    target: "菜单",
    icon: "menu",
  },
  back: {
    label: "返回键",
    desc: "固定来源：遥控器返回",
    target: "退格",
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
    case "menu":
      return (
        <svg {...common}>
          <path d="M4 7h16M4 12h16M4 17h16" />
        </svg>
      );
    case "github":
      return (
        <svg
          width={size}
          height={size}
          viewBox="0 0 24 24"
          fill="currentColor"
          aria-hidden="true"
        >
          <path d="M12 .8a11.2 11.2 0 0 0-3.54 21.83c.56.1.77-.24.77-.54v-2.1c-3.13.68-3.79-1.33-3.79-1.33-.51-1.3-1.25-1.65-1.25-1.65-1.02-.7.08-.69.08-.69 1.13.08 1.72 1.16 1.72 1.16 1 1.72 2.63 1.22 3.27.93.1-.72.39-1.22.71-1.5-2.5-.28-5.13-1.25-5.13-5.57 0-1.23.44-2.24 1.16-3.03-.12-.28-.5-1.43.11-2.98 0 0 .95-.3 3.1 1.16a10.8 10.8 0 0 1 5.65 0c2.15-1.46 3.1-1.16 3.1-1.16.61 1.55.23 2.7.11 2.98.72.79 1.16 1.8 1.16 3.03 0 4.33-2.64 5.28-5.15 5.56.4.35.76 1.03.76 2.08v3.08c0 .3.2.65.78.54A11.2 11.2 0 0 0 12 .8z" />
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

function targetLabel(source: KeySource) {
  if (source.name === "Escape" || source.name === "Esc") return "Esc";
  if (source.name === "Backspace") return "退格";
  if (source.name === "Return") return "回车";
  if (source.name === "Space") return "空格";
  if (source.name === "Delete") return "删除";
  if (source.name === "Right Option") return "右 Option";
  if (source.name === "Right Alt") return "右 Alt";
  if (source.name === "Menu") return "菜单";
  return sourceLabel(source);
}

function backendLabel(backend: string) {
  if (backend.includes("event tap")) return "系统级映射";
  if (backend.includes("hidutil")) return "兼容模式";
  if (backend.includes("evdev")) return "系统级映射";
  if (backend.includes("Interception")) return "系统级映射";
  if (backend.includes("SendInput")) return "语音键";
  return backend;
}

const HOTSPOTS: {
  action: KeyAction;
  label: string;
  top: string;
  left: string;
}[] = [
  { action: "voice", label: "语音键", top: "11.5%", left: "66.5%" },
  { action: "back", label: "返回键", top: "39.7%", left: "33.7%" },
  { action: "home", label: "Home 键", top: "48.3%", left: "34.2%" },
  { action: "tv", label: "TV 键", top: "56.7%", left: "63.0%" },
  { action: "menu", label: "菜单键", top: "56.8%", left: "33.8%" },
];

function RemoteVisual({
  capturing,
  pressed,
  disabled,
  onPick,
}: {
  capturing: KeyAction | null;
  pressed: KeyAction | null;
  disabled: boolean;
  onPick: (action: KeyAction) => void;
}) {
  return (
    <div className="remote-panel">
      <div className="remote-visual">
        <img src="/remote-2.jpg" alt="小米遥控器 2" draggable={false} />
        {HOTSPOTS.map((spot) => (
          <button
            key={spot.action}
            className={`hotspot ${
              capturing === spot.action ? "active" : ""
            } ${pressed === spot.action ? "pressed" : ""}`}
            style={{ top: spot.top, left: spot.left }}
            disabled={disabled}
            title={`${spot.label}：设置目标键`}
            onClick={() => onPick(spot.action)}
          >
            <span className="hotspot-label">{spot.label}</span>
          </button>
        ))}
      </div>
      <p className="remote-caption">
        点击遥控器按键，设置它的目标键
      </p>
    </div>
  );
}

function App() {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [page, setPage] = useState<Page>("overview");
  const [capturing, setCapturing] = useState<KeyAction | null>(null);
  const [testing, setTesting] = useState(false);
  const [pressedAction, setPressedAction] = useState<KeyAction | null>(null);
  const [testResult, setTestResult] = useState<{
    source: string;
    target: string;
    ok: boolean;
  } | null>(null);
  const [guide, setGuide] = useState<Guide | null>(null);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [updateDismissed, setUpdateDismissed] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const pageRef = useRef<HTMLDivElement>(null);
  const testHandlerRef = useRef<(key: CapturedKey) => void>(() => {});

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
    let unlistenKey: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    listen<CapturedKey>("key-test", (event) => {
      testHandlerRef.current(event.payload);
    }).then((unlisten) => {
      unlistenKey = unlisten;
    });
    listen<string>("key-test-error", (event) => {
      setToast(event.payload);
      setTesting(false);
    }).then((unlisten) => {
      unlistenError = unlisten;
    });
    return () => {
      unlistenKey?.();
      unlistenError?.();
      invoke("stop_key_test").catch(() => {});
    };
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      checkUpdate(false);
    }, 4000);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 2000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    if (page !== "audio") return;
    let disposed = false;
    invoke<Snapshot>("refresh_devices")
      .then((next) => {
        if (disposed) return;
        setSnap((prev) => (prev ? { ...prev, devices: next.devices } : next));
      })
      .catch(() => {});
    return () => {
      disposed = true;
    };
  }, [page]);

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
  const platform = snap.platform;
  const fallbackVoiceTarget = platform === "macos" ? "右 Option" : "右 Alt";
  const voiceTarget = draft
    ? targetLabel(draft.key_map.voice_target)
    : fallbackVoiceTarget;
  const metaFor = (action: KeyAction) => ({
    ...ACTION_META[action],
    desc:
      action === "voice"
        ? `按住说话，发送${voiceTarget}`
        : ACTION_META[action].desc,
    target: action === "voice" ? voiceTarget : ACTION_META[action].target,
  });
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

  const micUrl =
    platform === "windows"
      ? "https://vb-audio.com/Cable/"
      : "https://existential.audio/blackhole/";
  const micActionLabel =
    platform === "windows"
      ? "安装 VB-CABLE"
      : platform === "linux"
        ? "查看配置"
        : "安装 BlackHole";

  const steps = [
    {
      key: "mic",
      title: "虚拟麦克风",
      desc: hintPresent
        ? `${snap.default_device} 已就绪`
        : platform === "linux"
          ? "创建 PipeWire 虚拟输出并选择它"
          : `安装 ${platform === "windows" ? "VB-CABLE" : "BlackHole"} 后选择它作为输出`,
      ok: hintPresent,
      action: hintPresent
        ? null
        : {
            label: micActionLabel,
            run:
              platform === "linux"
                ? () => setGuide("mic")
                : () => openUrl(micUrl),
          },
      guide: "mic" as Guide,
    },
    {
      key: "shield",
      title: platform === "macos" ? "键盘控制权限" : "输入权限",
      desc:
        platform === "macos"
          ? snap.accessibility_trusted
            ? "已授权，可发送干净的按键事件"
            : "未授权时语音键会回退到 F5+Option"
          : platform === "linux"
            ? "需要 /dev/input 和 /dev/uinput 权限"
            : "Windows 无需额外权限",
      ok: platform === "macos" ? snap.accessibility_trusted : true,
      action:
        platform === "macos" && !snap.accessibility_trusted
          ? { label: "打开设置", run: openAccessibility }
          : null,
      guide: "permission" as Guide,
    },
    {
      key: "bluetooth",
      title: "遥控器连接",
      desc: connected ? (status.device ?? "已连接") : "等待 BLE 连接",
      ok: connected,
      action: connected ? null : { label: "启动桥接", run: toggleEngine },
      guide: "remote" as Guide,
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

  function targetFor(action: KeyAction): KeySource | null {
    if (!draft) return null;
    if (action === "voice") return draft.key_map.voice_target;
    if (action === "menu") return draft.key_map.menu_target;
    if (action === "home") return draft.key_map.home_target;
    if (action === "tv") return draft.key_map.tv_target;
    if (action === "back") return draft.key_map.back_target_key;
    return null;
  }

  function enabledFor(action: KeyAction) {
    if (!draft) return false;
    if (action === "voice") return draft.key_map.voice_to_right_option;
    if (action === "menu") return draft.key_map.menu_to_context;
    if (action === "home") return draft.key_map.home_to_escape;
    if (action === "tv") return draft.key_map.tv_to_backspace;
    return draft.key_map.back_target !== "none";
  }

  async function capture(action: KeyAction) {
    if (!draft || busy) return;
    setCapturing(action);
    setTestResult(null);
    try {
      const key = await invoke<CapturedKey>("capture_key");
      const target = { usage: key.usage, name: key.name };
      if (action === "voice") {
        await saveKeys(
          { voice_target: target, voice_to_right_option: true },
          `语音键 → ${key.name}`,
        );
      } else if (action === "menu") {
        await saveKeys(
          { menu_target: target, menu_to_context: true },
          `菜单键 → ${key.name}`,
        );
      } else if (action === "home") {
        await saveKeys(
          { home_target: target, home_to_escape: true },
          `Home 键 → ${key.name}`,
        );
      } else if (action === "tv") {
        await saveKeys(
          { tv_target: target, tv_to_backspace: true },
          `TV 键 → ${key.name}`,
        );
      } else {
        await saveKeys(
          { back_target_key: target, back_target: "backspace" },
          `返回键 → ${key.name}`,
        );
      }
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

  function handleTestKey(key: CapturedKey) {
    if (!draft) return;
    const known = ACTIONS.find((item) => BUILTIN_SOURCES[item] === key.usage);
    if (known) {
      const enabled = enabledFor(known);
      const target = !enabled
        ? "已禁用"
        : targetLabel(targetFor(known)!);
      setPressedAction(known);
      setTestResult({
        source: metaFor(known).label,
        target,
        ok: enabled,
      });
      window.setTimeout(() => setPressedAction(null), 420);
      return;
    }

    setTestResult({ source: key.name, target: "未绑定", ok: false });
  }
  testHandlerRef.current = handleTestKey;

  async function startTest() {
    if (
      !draft ||
      !snap ||
      busy ||
      testing ||
      capturing ||
      !snap.key_capture_supported
    ) {
      return;
    }
    setTestResult(null);
    try {
      await invoke("start_key_test");
      setTesting(true);
    } catch (error) {
      setToast(String(error));
    }
  }

  function stopTest() {
    invoke("stop_key_test").catch(() => {});
    setTesting(false);
    setPressedAction(null);
  }

  async function resetKeys() {
    await saveKeys(DEFAULT_KEYS, "已恢复默认");
  }

  async function saveProcessing(patchValue: Partial<AudioProcessing>) {
    if (!draft) return;
    const next = {
      ...draft,
      audio_processing: { ...draft.audio_processing, ...patchValue },
    };
    setDraft(next);
    setBusy(true);
    try {
      const nextSnap = await invoke<Snapshot>("update_settings", {
        settings: next,
      });
      setSnap(nextSnap);
      setDraft(nextSnap.settings);
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

  async function openUrl(url: string) {
    try {
      await invoke("open_url", { url });
    } catch (error) {
      setToast(String(error));
    }
  }

  async function checkUpdate(manual = false) {
    if (checkingUpdate) return;
    setCheckingUpdate(true);
    setUpdateError(null);
    try {
      const info = await invoke<UpdateInfo>("check_update");
      setUpdateInfo(info);
      if (info.available) setUpdateDismissed(false);
      if (manual) {
        setToast(
          info.available
            ? `发现新版本 v${info.latest}`
            : `已是最新版本 v${info.current}`,
        );
      }
    } catch (error) {
      setUpdateError(String(error));
      if (manual) setToast(String(error));
    } finally {
      setCheckingUpdate(false);
    }
  }

  function downloadUpdate() {
    const url = updateInfo?.asset_url ?? updateInfo?.url;
    if (url) openUrl(url);
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

  const platformName =
    platform === "macos" ? "macOS" : platform === "windows" ? "Windows" : "Linux";

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setToast("已复制");
    } catch {
      setToast("复制失败，请手动选择文本");
    }
  }

  function guideContent() {
    if (guide === "mic") {
      if (platform === "linux") {
        const command =
          'pactl load-module module-null-sink sink_name=xiaomi_remote sink_properties=device.description="Xiaomi Remote"';
        return (
          <>
            <p>虚拟音频设备需要单独安装一次，应用本身不包含。</p>
            <p>在终端执行下面的命令，创建一个名为 Xiaomi Remote 的虚拟输出：</p>
            <pre className="code">{command}</pre>
            <p>然后在「音频」页把输出设备选为 Xiaomi Remote，在目标 App 里选择它的 Monitor。</p>
            <div className="modal-actions">
              <button className="btn btn-ghost" onClick={() => copyText(command)}>
                复制命令
              </button>
            </div>
          </>
        );
      }
      const url =
        platform === "windows"
          ? "https://vb-audio.com/Cable/"
          : "https://existential.audio/blackhole/";
      return (
        <>
          <p>虚拟音频设备需要单独安装一次，应用本身不包含。</p>
          <p>
            {platform === "windows"
              ? "安装 VB-CABLE，然后在音频页把输出设备选为 CABLE 输入。"
              : "安装 BlackHole 2ch，然后在音频页把输出设备选为 BlackHole 2ch。"}
          </p>
          <p>
            {platform === "windows"
              ? "目标 App 的麦克风设为 CABLE 输出。"
              : "目标 App 的麦克风设为 BlackHole 2ch。"}
          </p>
          {platform === "macos" && (
            <pre className="code">brew install blackhole-2ch</pre>
          )}
          <div className="modal-actions">
            {platform === "macos" && (
              <button
                className="btn btn-ghost"
                onClick={() => copyText("brew install blackhole-2ch")}
              >
                复制命令
              </button>
            )}
            <button className="btn btn-primary" onClick={() => openUrl(url)}>
              打开下载页
            </button>
          </div>
        </>
      );
    }

    if (guide === "permission") {
      if (platform === "macos") {
        return (
          <>
            <p>
              打开「系统设置 → 隐私与安全性 → 设备控制与数据访问」，
              解锁后把 Xiaomi Remote 打开。授权后 App 会切换到系统级按键映射。
            </p>
            <p>
              如果刚重装过 App，开关可能还指向旧签名；先关闭再打开一次，
              或者用 tccutil 清理后重新授权。
            </p>
            <div className="modal-actions">
              <button className="btn btn-primary" onClick={() => { openAccessibility(); setGuide(null); }}>
                打开系统设置
              </button>
            </div>
          </>
        );
      }
      if (platform === "linux") {
        const command = `sudo usermod -aG input "$USER"
sudo tee /etc/udev/rules.d/99-xiaomi-remote.rules <<'RULE'
KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"
RULE
sudo udevadm control --reload-rules && sudo udevadm trigger`;
        return (
          <>
            <p>
              Linux 的按键映射需要读取 /dev/input/event*，并写入 /dev/uinput。
              执行下面的命令后重新登录：
            </p>
            <pre className="code">{command}</pre>
            <div className="modal-actions">
              <button className="btn btn-ghost" onClick={() => copyText(command)}>
                复制命令
              </button>
            </div>
          </>
        );
      }
      return (
        <>
          <p>Windows 的语音键通过 SendInput 注入，不需要额外权限。</p>
          <p>
            如果还需要 Home / TV / 返回键的设备级映射，需要安装 Interception
            驱动并使用可选构建。
          </p>
          <div className="modal-actions">
            <button className="btn btn-primary" onClick={() => setGuide(null)}>
              知道了
            </button>
          </div>
        </>
      );
    }

    const pairPath =
      platform === "macos"
        ? "系统设置 → 蓝牙"
        : platform === "windows"
          ? "设置 → 蓝牙和其他设备"
          : "bluetoothctl";
    return (
      <>
        <p>
          确保小米遥控器 2 已经在 {platformName} 的蓝牙设置中配对（{pairPath}）。
          如果遥控器不在列表里，先让遥控器进入配对模式，再重新搜索。
        </p>
        <p>配对完成后回到 App，启动桥接；App 会优先复用系统已连接的遥控器。</p>
        <div className="modal-actions">
          <button
            className="btn btn-primary"
            onClick={() => { setGuide(null); toggleEngine(); }}
          >
            启动桥接
          </button>
        </div>
      </>
    );
  }

  const guideTitle =
    guide === "mic"
      ? "虚拟麦克风设置"
      : guide === "permission"
        ? "权限设置"
        : "连接小米遥控器 2";

  const pageMeta: Record<Page, { title: string; subtitle: string }> = {
    overview: { title: "概览", subtitle: "遥控器、麦克风和权限状态" },
    keys: {
      title: "按键映射",
      subtitle: "遥控器按键固定，目标键可自定义",
    },
    audio: { title: "音频", subtitle: "虚拟麦克风与音频处理" },
    advanced: { title: "高级", subtitle: "权限、启动项与运行日志" },
  };

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar-brand">
          <span className="brand-mark">
            <img src="/app-icon.png" alt="" draggable={false} />
          </span>
          <span>
            <strong>Xiaomi Remote</strong>
            <small>小米遥控器 2 · V{snap.version}</small>
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
          <div className="sidebar-version">{snap.platform} · {snap.version}</div>
        </div>
      </aside>

      <main className="content">
        <header className="page-head">
          <div>
            <h1>{pageMeta[page].title}</h1>
          </div>
          <div className="page-actions">
            {page === "keys" && testing && (
              <span className="test-chip">
                <span className="status-dot recording" />
                测试中
                <button className="btn btn-small" onClick={stopTest}>
                  停止
                </button>
              </span>
            )}
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
              {running ? "停止语音桥接" : "启动语音桥接"}
            </button>
          </div>
        </header>

        {status.message && (
          <div className={`notice ${status.state === "error" ? "bad" : ""}`}>
            {status.message}
          </div>
        )}

        <div className="page-scroll" ref={pageRef}>
          {updateInfo?.available && !updateDismissed && (
            <div className="update-banner">
              <div className="update-banner-copy">
                <strong>发现新版本 v{updateInfo.latest}</strong>
                <span>当前 v{updateInfo.current}</span>
              </div>
              <button className="btn btn-small btn-primary" onClick={downloadUpdate}>
                下载更新
              </button>
              <button
                className="icon-btn"
                title="稍后"
                onClick={() => setUpdateDismissed(true)}
              >
                <Icon name="x" size={14} />
              </button>
            </div>
          )}

          {page === "overview" && (
            <div className="page">
              <section className="hero">
                <div className={`hero-orb ${recording ? "live" : ""}`}>
                  <Icon name={recording ? "mic" : "bluetooth"} size={26} />
                </div>
                <div className="hero-info">
                  <span className="eyebrow">遥控器</span>
                  <h2>{status.device ?? "小米遥控器 2"}</h2>
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
                      <div className="step-actions">
                        {step.action && (
                          <button
                            className="btn btn-small"
                            onClick={step.action.run}
                          >
                            {step.action.label}
                          </button>
                        )}
                        <button
                          className="btn btn-text"
                          onClick={() => setGuide(step.guide)}
                        >
                          指引
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>按键</h3>
                  </div>
                  <button className="btn btn-small" onClick={() => go("keys")}>
                    配置
                  </button>
                </div>
                <div className="mini-keys keys-mini">
                  {ACTIONS.map((action) => (
                    <div className="mini-key" key={action}>
                      <span>{ACTION_META[action].label}</span>
                      <div className="mini-flow">
                        <span className="arrow">→</span>
                        <code>
                          {action === "voice"
                            ? voiceTarget
                            : targetFor(action)
                              ? targetLabel(targetFor(action)!)
                              : "未设置"}
                        </code>
                      </div>
                    </div>
                  ))}
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>音频</h3>
                  </div>
                  <button className="btn btn-small" onClick={() => go("audio")}>
                    配置
                  </button>
                </div>
                <div className="mini-keys audio-mini">
                  <div className="mini-key">
                    <span>输出设备</span>
                    <code>{deviceReady ? "已就绪" : "未找到"}</code>
                  </div>
                  <div className="mini-key">
                    <span>降噪</span>
                    <code>
                      {draft.audio_processing.noise_reduction ? "开" : "关"}
                    </code>
                  </div>
                  <div className="mini-key">
                    <span>噪声门</span>
                    <code>{draft.audio_processing.noise_gate ? "开" : "关"}</code>
                  </div>
                  <div className="mini-key">
                    <span>增益</span>
                    <code>{draft.audio_processing.gain_db} dB</code>
                  </div>
                </div>
              </section>
            </div>
          )}

          {page === "keys" && (
            <div className="page">
              <section className="section keys-section">
                <div className="section-head">
                  <div>
                    <h3>遥控器按键</h3>
                  </div>
                  <div className="section-tools">
                    <button
                      className="btn btn-small"
                      onClick={testing ? stopTest : startTest}
                      disabled={
                        !snap.key_capture_supported ||
                        busy ||
                        Boolean(capturing)
                      }
                    >
                      {testing ? "停止测试" : "测试按键"}
                    </button>
                    <button
                      className="btn btn-small"
                      onClick={resetKeys}
                      disabled={busy}
                    >
                      恢复默认
                    </button>
                  </div>
                </div>

                {testing && (
                  <div className="test-live">
                    <span className="status-dot recording" />
                    <span>测试中 · 按键事件已临时拦截</span>
                  </div>
                )}

                <div className="keys-layout">
                  <RemoteVisual
                    capturing={capturing}
                    pressed={pressedAction}
                    disabled={
                      !snap.key_capture_supported || busy || testing
                    }
                    onPick={(action) => capture(action)}
                  />

                  <div className="keys-main">
                    <div className="key-list">
                      {ACTIONS.map((action) => {
                        const meta = metaFor(action);
                        const target = targetFor(action);
                        const enabled = enabledFor(action);
                        const disabled =
                          !snap.key_mapping_full && action !== "voice";
                        const targetName = target
                          ? targetLabel(target)
                          : "未设置";
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
                            <div className="key-flow">
                              <span className="arrow">→</span>
                              <button
                                className={`shortcut ${
                                  capturing === action ? "capturing" : ""
                                }`}
                                disabled={
                                  disabled ||
                                  busy ||
                                  testing ||
                                  !snap.key_capture_supported
                                }
                                onClick={() => capture(action)}
                                title="点击设置目标键"
                              >
                                <Icon name="keyboard" size={13} />
                                {capturing === action
                                  ? "按下按键…"
                                  : targetName}
                              </button>
                              <Switch
                                checked={enabled}
                                disabled={disabled || busy}
                                label={`${meta.label}启用`}
                                onChange={(value) => {
                                  if (action === "back") {
                                    saveKeys({
                                      back_target: value
                                        ? "backspace"
                                        : "none",
                                    });
                                  } else {
                                    const field =
                                      action === "voice"
                                        ? "voice_to_right_option"
                                        : action === "menu"
                                          ? "menu_to_context"
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

                    {testResult && (
                      <div
                        className={`test-result ${
                          testResult.ok ? "ok" : "bad"
                        }`}
                      >
                        <span>{testResult.source}</span>
                        <span className="arrow">→</span>
                        <strong>{testResult.target}</strong>
                      </div>
                    )}

                    {!snap.key_mapping_full && (
                      <p className="inline-note">
                        Windows 默认只支持语音键；Home / TV / 返回需要
                        Interception 驱动。
                      </p>
                    )}
                    <p className="inline-note">
                      {snap.key_capture_supported
                        ? "录制需要键盘控制权限。"
                        : "当前平台暂不支持录制，使用默认映射。"}
                    </p>
                  </div>
                </div>
              </section>
            </div>
          )}

          {page === "audio" && (
            <div className="page">
              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>虚拟麦克风</h3>
                  </div>
                  <button className="btn btn-small" onClick={() => setGuide("mic")}>
                    安装指引
                  </button>
                </div>

                {!hintPresent && (
                  <div className="audio-callout">
                    <div>
                      <strong>需要虚拟音频设备</strong>
                      <span>应用写入虚拟输出，会议软件读取它的输入。</span>
                    </div>
                    <button
                      className="btn btn-small btn-primary"
                      onClick={() => setGuide("mic")}
                    >
                      去安装
                    </button>
                  </div>
                )}

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

                <p className="inline-note">
                  设备自动刷新。
                  {platform === "macos"
                    ? " 推荐 BlackHole 2ch。"
                    : platform === "windows"
                      ? " 推荐 VB-CABLE。"
                      : " 使用 PipeWire 虚拟输出。"}
                </p>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>音频处理</h3>
                  </div>
                </div>
                <div className="processing-list">
                  <div className="row">
                    <div className="row-main">
                      <Icon name="wave" size={17} />
                      <div>
                        <strong>降噪</strong>
                        <span>抑制稳态背景噪声</span>
                      </div>
                    </div>
                    <Switch
                      checked={draft.audio_processing.noise_reduction}
                      label="降噪"
                      onChange={(value) =>
                        saveProcessing({ noise_reduction: value })
                      }
                    />
                  </div>
                  <div className="row">
                    <div className="row-main">
                      <Icon name="wave" size={17} />
                      <div>
                        <strong>噪声门</strong>
                        <span>安静时压低底噪</span>
                      </div>
                    </div>
                    <Switch
                      checked={draft.audio_processing.noise_gate}
                      label="噪声门"
                      onChange={(value) => saveProcessing({ noise_gate: value })}
                    />
                  </div>
                  <div className="row">
                    <div className="row-main">
                      <Icon name="mic" size={17} />
                      <div>
                        <strong>增益</strong>
                        <span>输出音量</span>
                      </div>
                    </div>
                    <select
                      className="select compact"
                      value={draft.audio_processing.gain_db}
                      onChange={(event) =>
                        saveProcessing({
                          gain_db: Number(event.target.value),
                        })
                      }
                    >
                      <option value={0}>0 dB</option>
                      <option value={6}>6 dB</option>
                      <option value={9}>9 dB</option>
                      <option value={12}>12 dB</option>
                      <option value={15}>15 dB</option>
                      <option value={18}>18 dB</option>
                    </select>
                  </div>
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>回放</h3>
                  </div>
                </div>
                <div className="row">
                  <div className="row-main">
                    <Icon name="play" size={17} />
                    <div>
                      <strong>上次录音</strong>
                      <span>
                        {snap.last_recording ? "可播放" : "暂无录音"}
                      </span>
                    </div>
                  </div>
                  <button
                    className="btn btn-primary"
                    onClick={openRecording}
                    disabled={!snap.last_recording}
                  >
                    <Icon name="play" size={14} />
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
                    <h3>更新</h3>
                  </div>
                  <button
                    className="btn btn-small"
                    onClick={() => checkUpdate(true)}
                    disabled={checkingUpdate}
                  >
                    {checkingUpdate ? "检查中…" : "检查更新"}
                  </button>
                </div>
                <div className="update-status">
                  <div className="update-status-copy">
                    <strong>当前 v{snap.version}</strong>
                    <span>
                      {updateInfo
                        ? updateInfo.available
                          ? `新版本 v${updateInfo.latest} 可用`
                          : "已是最新版本"
                        : updateError
                          ? updateError
                          : "尚未检查"}
                    </span>
                  </div>
                  {updateInfo?.available && (
                    <button
                      className="btn btn-small btn-primary"
                      onClick={downloadUpdate}
                    >
                      下载更新
                    </button>
                  )}
                </div>
              </section>

              <section className="section">
                <div className="section-head">
                  <div>
                    <h3>运行日志</h3>
                  </div>
                </div>
                <LogList logs={snap.logs} scrollRef={logRef} />
              </section>

              <section className="section about">
                <div className="about-grid">
                  <div>
                    <span>版本</span>
                    <strong>{snap.version}</strong>
                  </div>
                  <div>
                    <span>平台</span>
                    <strong>{snap.platform}</strong>
                  </div>
                  <div>
                    <span>按键后端</span>
                    <strong>{backendLabel(status.key_backend)}</strong>
                  </div>
                  <div>
                    <span>音频链路</span>
                    <strong>16 kHz → 48 kHz</strong>
                  </div>
                </div>
                <div className="author-row">
                  <div className="author-copy">
                    <span>作者 vlln</span>
                    <strong>
                      如果它让你的遥控器更好用，去 GitHub 点个 Star 支持一下
                    </strong>
                  </div>
                  <button
                    className="btn btn-small btn-star"
                    onClick={() =>
                      openUrl(
                        "https://github.com/vlln/xiaomi-remote-voice-coding-adapter",
                      )
                    }
                  >
                    <Icon name="github" size={15} />
                    去点 Star
                  </button>
                </div>
              </section>
            </div>
          )}
        </div>

        {dirty && (
          <footer className="savebar">
            <span className="savebar-hint">有未保存的更改</span>
            <div className="savebar-actions">
              <button
                className="btn btn-primary"
                onClick={save}
                disabled={busy}
              >
                保存设置
              </button>
            </div>
          </footer>
        )}
      </main>

      {capturing && (
        <div className="capture-overlay">
          <div className="capture-card">
            <span className="capture-key">
              <Icon name="keyboard" size={30} />
            </span>
            <h2>按下要映射到的按键</h2>
            <p>
              正在设置「{ACTION_META[capturing].label}」的目标键
            </p>
            <span className="capture-timer">10 秒后自动取消</span>
            <button className="btn btn-ghost" onClick={cancelCapture}>
              取消
            </button>
          </div>
        </div>
      )}

      {guide && (
        <div className="modal-overlay" onMouseDown={() => setGuide(null)}>
          <div className="modal" onMouseDown={(event) => event.stopPropagation()}>
            <div className="modal-head">
              <h2>{guideTitle}</h2>
              <button
                className="icon-btn"
                onClick={() => setGuide(null)}
                aria-label="关闭"
              >
                <Icon name="x" size={16} />
              </button>
            </div>
            <div className="modal-body">{guideContent()}</div>
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
