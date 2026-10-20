#!/usr/bin/env python3
"""Xiaomi Remote 2 Pro — 语音桥接 (BLE → BlackHole 系统麦克风)

通过 CoreBluetooth 连接遥控器 ATVV 语音服务,
实时解码 ADPCM 音频流,写入 BlackHole 虚拟麦克风,
让任意 App 使用遥控器作为系统拾音设备。

使用:
    python3 -u scripts/voice_capture.py          # 前台运行
    python3 -u scripts/voice_capture.py --debug  # 同时保存 WAV 调试

依赖: BlackHole 2ch (brew install blackhole-2ch)
"""

from __future__ import annotations

import objc
import sys
import time
import threading
import wave
from collections import deque
from enum import Enum, auto
from Foundation import NSRunLoop, NSDate, NSObject
import CoreBluetooth

import sounddevice as sd
import numpy as np

# ── UUIDs ────────────────────────────────────────────────────
VOICE_SERVICE = "ab5e0001-5a21-4f05-bc7d-af01f617b664"
VOICE_TX = "ab5e0002-5a21-4f05-bc7d-af01f617b664"
VOICE_AUDIO = "ab5e0003-5a21-4f05-bc7d-af01f617b664"
VOICE_CONTROL = "ab5e0004-5a21-4f05-bc7d-af01f617b664"
GET_CAPS = bytes((0x0A, 0x01, 0x00, 0x00, 0x03, 0x03))

# ── ADPCM 解码器 ────────────────────────────────────────────
STEP_TABLE = (
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31,
    34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411,
    1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026,
    4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442,
    11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623,
    27086, 29794, 32767,
)
INDEX_TABLE = (-1, -1, -1, -1, 2, 4, 6, 8)


class AdpcmDecoder:
    def __init__(self):
        self.predictor = 0
        self.step_index = 0

    def reset(self, predictor: int, step_index: int):
        self.predictor = max(-32768, min(32767, int(predictor)))
        self.step_index = max(0, min(88, int(step_index)))

    def decode_nibble(self, nibble: int) -> int:
        step = STEP_TABLE[self.step_index]
        diff = step >> 3
        if nibble & 1:
            diff += step >> 2
        if nibble & 2:
            diff += step >> 1
        if nibble & 4:
            diff += step
        self.predictor += -diff if nibble & 8 else diff
        self.predictor = max(-32768, min(32767, self.predictor))
        self.step_index += INDEX_TABLE[nibble & 7]
        self.step_index = max(0, min(88, self.step_index))
        return self.predictor

    def decode_bytes(self, data: bytes) -> list[int]:
        samples = []
        for value in data:
            samples.append(self.decode_nibble(value >> 4))
            samples.append(self.decode_nibble(value & 0x0F))
        return samples


class DebugWavRecorder:
    """同步保存滤波前后的 16kHz PCM，便于逐样本 A/B。"""

    def __init__(self, raw_path="voice_raw.wav", processed_path="voice_processed.wav"):
        self._lock = threading.Lock()
        self._raw_wav = self._open_wav(raw_path)
        self._processed_wav = self._open_wav(processed_path)
        self._closed = False
        self.raw_path = str(raw_path)
        self.processed_path = str(processed_path)

    @staticmethod
    def _open_wav(path):
        output = wave.open(str(path), "wb")
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16000)
        return output

    def write_pair(self, raw_samples: list[int], processed_samples: list[int]):
        if len(raw_samples) != len(processed_samples):
            raise ValueError("raw/processed 调试音频长度不一致")
        raw_bytes = np.asarray(raw_samples, dtype="<i2").tobytes()
        processed_bytes = np.asarray(processed_samples, dtype="<i2").tobytes()
        with self._lock:
            if self._closed:
                return
            self._raw_wav.writeframesraw(raw_bytes)
            self._processed_wav.writeframesraw(processed_bytes)

    def close(self):
        with self._lock:
            if self._closed:
                return
            self._closed = True
            self._raw_wav.close()
            self._processed_wav.close()
        print(f"  原始调试 WAV 已保存: {self.raw_path}", flush=True)
        print(f"  处理后调试 WAV 已保存: {self.processed_path}", flush=True)


# ── 音频输出到 BlackHole ─────────────────────────────────────

class AudioOutput:
    """独立线程写入 BlackHole, 避免阻塞 BLE RunLoop"""

    MAX_BUFFER_SAMPLES = 4800  # 300ms @ 16kHz，优先保证实时性

    def __init__(self):
        self._buffer = deque()
        self._condition = threading.Condition()
        self._running = False
        self._draining = False
        self._thread: threading.Thread | None = None
        self._stream = None
        self._device_idx = None
        self._written = 0
        self._dropped = 0
        self._previous = 0
        self._have_previous = False

    def prepare(self):
        """预先打开 BlackHole，避免首段音频在 PortAudio 初始化期间积压。"""
        if self._stream is not None:
            return
        devices = sd.query_devices()
        for i, dev in enumerate(devices):
            if "BlackHole 2ch" in dev["name"]:
                self._device_idx = i
                break
        if self._device_idx is None:
            raise RuntimeError("BlackHole 2ch 未找到")

        started = time.monotonic()
        self._stream = sd.OutputStream(
            device=self._device_idx,
            samplerate=48000,
            channels=2,
            dtype='int16',
            blocksize=480,
            latency=0.05,
        )
        self._stream.start()
        elapsed = time.monotonic() - started
        print(f"  BlackHole 音频输出已就绪 (device={self._device_idx}, "
              f"初始化={elapsed:.3f}s)", flush=True)

    def start(self):
        if self._running:
            return
        self.prepare()

        # 中断尚未排完的旧会话。每次只写固定大小的块，因此旧线程应在
        # 一个音频块内退出；绝不能在旧线程存活时复用下面这些共享字段。
        with self._condition:
            self._running = False
            self._draining = False
            self._buffer.clear()
            self._condition.notify_all()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=1)
            if self._thread.is_alive():
                raise RuntimeError("旧音频输出线程未能停止")
        self._thread = None

        with self._condition:
            self._have_previous = False
            self._written = 0
            self._dropped = 0
            self._draining = False
            self._running = True
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        print("  BlackHole 音频会话已启动", flush=True)

    def write(self, samples: list[int]):
        with self._condition:
            if not self._running:
                return
            self._buffer.extend(samples)
            overflow = len(self._buffer) - self.MAX_BUFFER_SAMPLES
            for _ in range(max(0, overflow)):
                self._buffer.popleft()
            self._dropped += max(0, overflow)
            self._condition.notify()

    def _run(self):
        # 预缓冲: 攒够 ~2 帧 (30ms @16kHz) 再开流, 吸收 BLE 通知突发/抖动
        # (实测 BLE 通知间隔会在 1ms~27ms 间跳动, 不预缓冲会 underrun)
        pre_buffer = 480  # 2 帧 × 240 samples
        with self._condition:
            while self._running and len(self._buffer) < pre_buffer:
                self._condition.wait()
            # drain 模式下即使不足预缓冲也要播放尾部。
            if not self._running and (not self._draining or not self._buffer):
                return

        # 固定为最多 30ms，防止一次 stream.write 被历史积压阻塞数秒。
        output_chunk = 480
        error = None
        try:
            while True:
                with self._condition:
                    while self._running and not self._buffer:
                        self._condition.wait()
                    if not self._running and (not self._draining or not self._buffer):
                        break
                    count = min(len(self._buffer), output_chunk)
                    chunk = [self._buffer.popleft() for _ in range(count)]
                    stream = self._stream
                if stream is None:
                    break

                stream.write(self._resample(chunk))
                with self._condition:
                    self._written += len(chunk)
        except Exception as exc:
            error = exc
        finally:
            with self._condition:
                self._running = False
                self._draining = False
                self._buffer.clear()

        dropped = f", {self._dropped} 过期采样丢弃" if self._dropped else ""
        print(f"  音频输出: {self._written} 采样写入{dropped}", flush=True)
        if error is not None:
            print(f"  ⚠️ BlackHole 输出异常: {error}", flush=True)

    def _resample(self, chunk: list[int]) -> np.ndarray:
        """16kHz→48kHz 线性插值，保持跨块连续。"""
        current = np.asarray(chunk, dtype=np.int32)
        previous = np.empty_like(current)
        previous[0] = self._previous if self._have_previous else current[0]
        previous[1:] = current[:-1]
        delta = current - previous

        mono = np.empty(len(current) * 3, dtype=np.int16)
        mono[0::3] = previous + np.rint(delta / 3.0).astype(np.int32)
        mono[1::3] = previous + np.rint(delta * (2.0 / 3.0)).astype(np.int32)
        mono[2::3] = current
        self._previous = int(current[-1])
        self._have_previous = True
        return np.repeat(mono[:, None], 2, axis=1)

    def stop(self, drain: bool = False):
        with self._condition:
            self._draining = drain
            self._running = False
            if not drain:
                self._buffer.clear()
            self._condition.notify_all()
        if not drain and self._thread:
            self._thread.join(timeout=1)

    def close(self):
        self.stop()
        stream = self._stream
        self._stream = None
        if stream is not None:
            stream.abort()
            stream.close()


# ── 降噪 ────────────────────────────────────────────────────

class NoiseReducer:
    """轻度降噪: 高通 + 低通平滑, 不损伤人声"""

    def __init__(self):
        self._prev_input = 0.0
        self._prev_output = 0.0

    def _highpass(self, samples: list[int]) -> list[int]:
        """一阶高通 (~220Hz) 去除 DC + AC 气流低频主体"""
        alpha = 0.91
        result = []
        for s in samples:
            output = alpha * (self._prev_output + s - self._prev_input)
            self._prev_input = float(s)
            self._prev_output = output
            result.append(int(output))
        return result

    def _lowpass(self, samples: list[int]) -> list[int]:
        """3 点移动平均, 平滑 ADPCM 量化噪声"""
        if len(samples) < 3:
            return samples
        result = samples[:]
        for i in range(1, len(samples) - 1):
            result[i] = (samples[i - 1] + samples[i] * 2 + samples[i + 1]) >> 2
        return result

    def process(self, samples: list[int]) -> list[int]:
        samples = self._highpass(samples)
        samples = self._lowpass(samples)
        return samples


class NoiseGate:
    """自适应噪声门: 启动校准后衰减稳定背景噪声 (AC/气流)。

    - 前 450ms 保持全通并用低分位能量初始化底噪，避免初值过低永不开门
    - 仅在门限以下缓慢上调底噪，避免连续语音抬高估计
    - 软扩展门: 门限以下按能量比例衰减, 避免硬切产生的断续感
    - 跨块线性插值增益, 避免咔哒声
    """

    CALIBRATION_FRAMES = 30

    def __init__(self):
        self._noise_floor = None
        self._calibration = []
        self._gain = 1.0
        self._threshold_db = 8.0    # 门限高于底噪 8dB
        self._floor_gain = 0.15     # 关门时 -16dB
        self._attack = 0.15         # 关门速度: ~100ms
        self._release = 0.5         # 开门速度: ~20ms

    @property
    def gain(self) -> float:
        return self._gain

    def process(self, samples: list[int]) -> list[int]:
        if not samples:
            return samples
        n = len(samples)
        e = sum(s * s for s in samples) / n

        if len(self._calibration) < self.CALIBRATION_FRAMES:
            self._calibration.append(e)
            if len(self._calibration) == self.CALIBRATION_FRAMES:
                self._noise_floor = max(1.0, float(np.percentile(self._calibration, 20)))
            return samples

        assert self._noise_floor is not None
        thr_sq = self._noise_floor * (10.0 ** (self._threshold_db / 20.0)) ** 2
        if e < self._noise_floor:
            self._noise_floor = max(1.0, 0.9 * self._noise_floor + 0.1 * e)
        elif e < thr_sq:
            self._noise_floor = 0.995 * self._noise_floor + 0.005 * e

        # 软扩展门: 使用能量比，底噪附近约衰减到 floor_gain。
        if e > thr_sq:
            target = 1.0
        else:
            target = max(self._floor_gain, e / thr_sq)

        coef = self._attack if target < self._gain else self._release
        new_gain = self._gain * (1 - coef) + target * coef

        # 跨块线性插值增益, 消除咔哒
        g0 = self._gain
        out = [max(-32768, min(32767, int(s * (g0 + (new_gain - g0) * (i / n)))))
               for i, s in enumerate(samples)]
        self._gain = new_gain
        return out


# ── CoreBluetooth 语音捕获 ──────────────────────────────────

class VoiceSessionState(Enum):
    IDLE = auto()
    ARMED = auto()
    STREAMING = auto()
    CLOSING = auto()


class VoiceCaptureDelegate(NSObject):
    CONTROL_QUIET_SECONDS = 0.75
    START_CONFIRM_SECONDS = 0.1
    TAIL_GUARD_FRAMES = 2

    def init(self):
        self = objc.super(VoiceCaptureDelegate, self).init()
        if self is None:
            return None
        self.manager = None
        self.peripheral = None
        self.tx_char = None
        self.audio_char = None
        self.control_char = None
        self.caps = None
        self.decoder = AdpcmDecoder()
        self.noise_reducer = NoiseReducer()
        self.noise_gate = NoiseGate()
        self.audio_output = None  # 在 _start_capture 中初始化
        self.debug_recorder = None
        self.mic_open = False
        self.audio_frames = 0
        self.pcm_samples = 0
        self.stop_event = False
        self.started = False
        self._end_speech_lock = threading.Lock()  # 保护 _end_speech 并发调用
        self._pending_audio = bytearray()
        self._tail_guard = deque()
        self._skip_frames = 0
        self._audio_subscribed = False
        self._control_subscribed = False
        self._t0 = time.monotonic()    # 调试时间戳基准
        self._verbose = False          # --verbose 时打印每条 BLE 通知
        self._session_state = VoiceSessionState.IDLE
        self._last_audio_time = 0.0    # 最后一次收到音频帧的时间 (monotonic)
        self._session_start = 0.0      # 当前会话开始时间 (monotonic)
        self._closing_activity = 0.0   # CLOSING 期间最后一个残余控制包
        self._connected = False        # BLE 是否已连接
        self._watchdog_thread = None   # 看门狗线程
        self._watchdog_stop = False    # 看门狗停止标志
        return self

    def _ts(self) -> str:
        return f"[+{time.monotonic() - self._t0:7.3f}s]"

    def _start_capture(self, debug: bool = False, verbose: bool = False):
        self._verbose = verbose
        self.debug_recorder = DebugWavRecorder() if debug else None
        self.audio_output = AudioOutput()
        self.audio_output.prepare()
        self.manager = CoreBluetooth.CBCentralManager.alloc().initWithDelegate_queue_(self, None)
        # 启动独立看门狗线程 (不受 runloop 阻塞影响)
        self._watchdog_stop = False
        self._watchdog_thread = threading.Thread(target=self._watchdog_loop, daemon=True)
        self._watchdog_thread.start()
        print("正在初始化蓝牙...")

    def _send_mic_open(self):
        if not self._connected or not self.peripheral or not self.tx_char:
            print(f"{self._ts()} ⚠️ MIC_OPEN 跳过 (未连接)", flush=True)
            return False
        # 先确认本地输出可用，避免命令已发出但本地处于半开启状态。
        self.audio_output.start()
        version = self.caps.get("version", 0x0100) if self.caps else 0x0100
        cmd = bytes((0x0C, 0x00)) if version >= 0x0100 else bytes((0x0C, 0x00, 0x02))
        try:
            self.peripheral.writeValue_forCharacteristic_type_(cmd, self.tx_char, 1)
        except Exception:
            self.audio_output.stop()
            raise
        self.mic_open = True
        self._session_start = time.monotonic()
        self._last_audio_time = self._session_start
        print(f"{self._ts()} 已发送 MIC_OPEN: {cmd.hex()}", flush=True)
        return True

    def _send_mic_close(self):
        """发送 MIC_CLOSE, 正式关闭遥控器麦克风会话。"""
        if not self._connected or not self.peripheral or not self.tx_char or not self.caps:
            return
        version = self.caps.get("version", 0x0100)
        close_cmd = bytes((0x0D, 0x00)) if version >= 0x0100 else bytes((0x0D,))
        self.peripheral.writeValue_forCharacteristic_type_(close_cmd, self.tx_char, 1)
        print(f"{self._ts()} 已发送 MIC_CLOSE: {close_cmd.hex()}", flush=True)

    # ── CBCentralManagerDelegate ─────────────────────────────

    def centralManagerDidUpdateState_(self, manager):
        try:
            states = {5: "PoweredOn", 4: "Unauthorized", 3: "Unsupported", 2: "PoweredOff"}
            state_name = states.get(manager.state(), f"Unknown({manager.state()})")
            print(f"蓝牙状态: {state_name}")

            if manager.state() == 5:
                voice_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_SERVICE)
                peripherals = manager.retrieveConnectedPeripheralsWithServices_([voice_uuid])
                print(f"已连接 ATVV 设备: {len(peripherals)}")

                if peripherals:
                    self.peripheral = peripherals[0]
                    print(f"找到: {self.peripheral.name() or 'unnamed'}")
                    self.peripheral.setDelegate_(self)
                    manager.connectPeripheral_options_(self.peripheral, None)
                else:
                    print("扫描中...")
                    manager.scanForPeripheralsWithServices_options_([voice_uuid], None)
        except Exception:
            import traceback
            traceback.print_exc()

    def centralManager_didDiscoverPeripheral_advertisementData_RSSI_(self, manager, peripheral, ad_data, rssi):
        try:
            name = peripheral.name() or "unnamed"
            if '小米' in str(name) or 'xiaomi' in str(name).lower() or 'mi rc' in str(name).lower():
                print(f"发现: {name}, 连接中...")
                self.peripheral = peripheral
                self.peripheral.setDelegate_(self)
                manager.stopScan()
                manager.connectPeripheral_options_(peripheral, None)
        except Exception:
            import traceback
            traceback.print_exc()

    def centralManager_didConnectPeripheral_(self, manager, peripheral):
        try:
            print(f"已连接: {peripheral.name() or 'unnamed'}")
            self._connected = True
            voice_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_SERVICE)
            peripheral.discoverServices_([voice_uuid])
        except Exception:
            import traceback
            traceback.print_exc()

    def centralManager_didFailToConnectPeripheral_error_(self, manager, peripheral, error):
        try:
            print(f"连接失败: {error}")
            self._connected = False
        except Exception:
            import traceback
            traceback.print_exc()

    def centralManager_didDisconnectPeripheral_error_(self, manager, peripheral, error):
        try:
            print(f"{self._ts()} 🔌 BLE 断连: {peripheral.name() or 'unnamed'} error={error}", flush=True)
            self._connected = False
            self.mic_open = False
            self._session_state = VoiceSessionState.IDLE
            self._closing_activity = 0.0
            self._tail_guard.clear()
            if self.audio_output:
                self.audio_output.stop()
        except Exception:
            import traceback
            traceback.print_exc()

    # ── CBPeripheralDelegate ─────────────────────────────────

    def peripheral_didDiscoverServices_(self, peripheral, error):
        try:
            if error:
                print(f"服务发现失败: {error}")
                return
            for svc in peripheral.services():
                if str(svc.UUID()).casefold() == VOICE_SERVICE:
                    print("找到 ATVV 语音服务")
                    tx_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_TX)
                    audio_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_AUDIO)
                    control_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_CONTROL)
                    peripheral.discoverCharacteristics_forService_(
                        [tx_uuid, audio_uuid, control_uuid], svc)
        except Exception:
            import traceback
            traceback.print_exc()

    def peripheral_didDiscoverCharacteristicsForService_error_(self, peripheral, service, error):
        try:
            if error:
                print(f"特征发现失败: {error}")
                return
            for char in service.characteristics():
                uuid = str(char.UUID()).casefold()
                print(f"  特征: {uuid} 属性=0x{char.properties():x}")
                if uuid == VOICE_TX:
                    self.tx_char = char
                elif uuid == VOICE_AUDIO:
                    self.audio_char = char
                elif uuid == VOICE_CONTROL:
                    self.control_char = char

            if self.audio_char and self.control_char:
                print("订阅 Audio + Control 通知...")
                peripheral.setNotifyValue_forCharacteristic_(True, self.audio_char)
                peripheral.setNotifyValue_forCharacteristic_(True, self.control_char)
        except Exception:
            import traceback
            traceback.print_exc()

    def peripheral_didUpdateNotificationStateForCharacteristic_error_(self, peripheral, char, error):
        try:
            uuid = str(char.UUID()).casefold()
            if error:
                print(f"订阅失败 {uuid}: {error}")
                return
            print(f"订阅成功: {uuid}")

            if uuid == VOICE_AUDIO:
                self._audio_subscribed = True
            elif uuid == VOICE_CONTROL:
                self._control_subscribed = True

            if self._audio_subscribed and self._control_subscribed:
                print(f"发送 GET_CAPS: {GET_CAPS.hex()}")
                peripheral.writeValue_forCharacteristic_type_(GET_CAPS, self.tx_char, 1)
                print(f"\n按住遥控器语音键说话... (Ctrl+C 停止)\n")
                self.started = True
        except Exception:
            import traceback
            traceback.print_exc()

    def peripheral_didUpdateValueForCharacteristic_error_(self, peripheral, char, error):
        try:
            if error:
                return
            data = char.value()
            if data is None:
                return
            payload = bytes(data)
            uuid = str(char.UUID()).casefold()

            if uuid == VOICE_CONTROL:
                if self._verbose:
                    print(f"{self._ts()} BLE通知到达: CTL len={len(payload)}", flush=True)
                self._handle_control(payload)
            elif uuid == VOICE_AUDIO:
                if self._verbose:
                    print(f"{self._ts()} BLE通知到达: AUD len={len(payload)}", flush=True)
                self._handle_audio(payload)
        except Exception:
            import traceback
            traceback.print_exc()

    def _handle_control(self, data: bytes):
        if not data:
            return
        opcode = data[0]
        print(f"{self._ts()} 控制: opcode=0x{opcode:02X} len={len(data)} "
              f"state={self._session_state.name} mic_open={self.mic_open}", flush=True)

        if opcode == 0x0B:
            self.caps = self._parse_caps(data)
            print(f"{self._ts()} 能力: {self.caps}", flush=True)
        elif opcode == 0x08:
            print(f"{self._ts()} 🎤 语音键按下!", flush=True)
            now = time.monotonic()
            if self._closing_event_is_stale(now, opcode):
                return
            if self.caps and self._session_state is VoiceSessionState.IDLE:
                if self._send_mic_open():
                    self._session_state = VoiceSessionState.ARMED
        elif opcode == 0x04:
            now = time.monotonic()
            if self._closing_event_is_stale(now, opcode):
                return

            if self._session_state is VoiceSessionState.STREAMING:
                # MIC_OPEN 后约 30ms 的第二个 0x04 是遥控器启动确认；
                # 个别情况下首个音频帧会先到，因此按时间窗而不是帧数判断。
                since_open = now - self._session_start
                if 0 <= since_open <= self.START_CONFIRM_SECONDS:
                    print(f"{self._ts()} AUDIO_START 确认", flush=True)
                else:
                    print(f"{self._ts()} ⚠️ 流中重复 AUDIO_START "
                          f"(audio_frames={self.audio_frames} pending={len(self._pending_audio)}), "
                          "忽略重置", flush=True)
            else:
                if self._session_state is VoiceSessionState.IDLE and not self._send_mic_open():
                    return
                self._session_state = VoiceSessionState.STREAMING
                print(f"{self._ts()} 🎤 音频流开始", flush=True)
                self.decoder.reset(0, 0)
                self._pending_audio = bytearray()
                self._tail_guard.clear()
                self._skip_frames = 8  # 跳过前 ~100ms 消除按键声
        elif opcode == 0x0A and len(data) >= 7:
            predictor = int.from_bytes(data[4:6], "big", signed=True)
            self.decoder.reset(predictor, data[6])
            print(f"{self._ts()} 音频同步: predictor={predictor} step={data[6]}", flush=True)
        elif opcode == 0x00:
            now = time.monotonic()
            if self._session_state is VoiceSessionState.CLOSING:
                self._closing_activity = now
                print(f"{self._ts()} 忽略 CLOSING 期间的 AUDIO_STOP", flush=True)
                return
            if not self.mic_open:
                print(f"{self._ts()} 忽略重复 AUDIO_STOP (mic_open=False)", flush=True)
                return
            print(f"{self._ts()} 🎤 语音结束 (遥控器通知)", flush=True)
            self._end_speech()

    def _closing_event_is_stale(self, now: float, opcode: int) -> bool:
        """持续控制风暴会不断延后重新武装；静默后允许真正的新会话。"""
        if self._session_state is not VoiceSessionState.CLOSING:
            return False
        quiet_for = now - self._closing_activity
        if quiet_for >= self.CONTROL_QUIET_SECONDS:
            self._session_state = VoiceSessionState.IDLE
            self._closing_activity = 0.0
            return False
        self._closing_activity = now
        print(f"{self._ts()} ⚠️ CLOSING 期间 0x{opcode:02X} 被忽略 "
              f"(此前静默 {quiet_for:.3f}s)", flush=True)
        return True

    def _end_speech(self):
        """结束当前语音会话: 刷尾部、停输出、发 MIC_CLOSE。

        可能在 BLE 回调线程或看门狗线程被调用, 用锁保护。
        """
        with self._end_speech_lock:
            if not self.mic_open:
                return  # 已被另一线程结束
            self.mic_open = False
            self._session_state = VoiceSessionState.CLOSING
            self._closing_activity = time.monotonic()
            # 刷新尾部残留
            pending = getattr(self, '_pending_audio', bytearray())
            tail_frames = 0
            if pending and self.caps:
                frame_size = self.caps.get("frame_size", 120)
                if len(pending) > 0:
                    pad_len = frame_size - len(pending) % frame_size
                    if pad_len < frame_size:
                        pending.extend(b'\x00' * pad_len)
                    while len(pending) >= frame_size:
                        frame = bytes(pending[:frame_size])
                        del pending[:frame_size]
                        raw_samples = self.decoder.decode_bytes(frame)
                        samples = self.noise_reducer.process(raw_samples)
                        samples = self.noise_gate.process(samples)
                        samples = self._apply_gain(samples, 15.0)
                        self._queue_processed_frame(raw_samples, samples)
                        tail_frames += 1
            print(f"{self._ts()} 尾部刷新: {tail_frames} 帧, pending={len(pending)} bytes", flush=True)
            self._pending_audio = bytearray()
            self._skip_frames = 0
            discarded_tail = len(self._tail_guard)
            self._tail_guard.clear()
            if discarded_tail:
                print(f"{self._ts()} 尾帧保护: 丢弃 {discarded_tail} 帧", flush=True)
            self.audio_frames = 0
            self.pcm_samples = 0
            self.noise_reducer = NoiseReducer()
            self.noise_gate = NoiseGate()
            self.audio_output.stop(drain=True)
            self._send_mic_close()

    def _handle_audio(self, data: bytes):
        if (self._session_state is not VoiceSessionState.STREAMING or
                not self.mic_open or self.caps is None or not self._connected):
            return
        self._last_audio_time = time.monotonic()
        frame_size = self.caps.get("frame_size", 120)
        pending = getattr(self, '_pending_audio', bytearray())
        pending.extend(data)
        if self._verbose:
            print(f"{self._ts()} 音频通知: len={len(data)} pending={len(pending)}", flush=True)
        while len(pending) >= frame_size:
            if not self.mic_open:
                break  # 看门狗/0x00 在处理过程中结束了会话
            frame = bytes(pending[:frame_size])
            del pending[:frame_size]
            self._pending_audio = pending

            self.audio_frames += 1

            skip = getattr(self, '_skip_frames', 0)
            if skip > 0:
                if self._verbose:
                    print(f"{self._ts()} 跳帧 #{self.audio_frames} (skip remaining={skip - 1})", flush=True)
                self._skip_frames = skip - 1
                continue

            raw_samples = self.decoder.decode_bytes(frame)
            samples = self.noise_reducer.process(raw_samples)
            samples = self.noise_gate.process(samples)
            samples = self._apply_gain(samples, 15.0)
            self._queue_processed_frame(raw_samples, samples)

            if self.audio_frames == 1 or self.audio_frames % 10 == 0:
                print(f"{self._ts()} 音频: #{self.audio_frames} 帧, {self.pcm_samples} 采样, gate_gain={self.noise_gate.gain:.3f}", flush=True)

    def _queue_processed_frame(self, raw_samples: list[int], samples: list[int]):
        """延迟两帧输出；会话结束时丢弃遥控器释放键产生的尾部脉冲。"""
        self._tail_guard.append((raw_samples, samples))
        if len(self._tail_guard) <= self.TAIL_GUARD_FRAMES:
            return
        raw_to_emit, samples_to_emit = self._tail_guard.popleft()
        if self.debug_recorder:
            self.debug_recorder.write_pair(raw_to_emit, samples_to_emit)
        self.audio_output.write(samples_to_emit)
        self.pcm_samples += len(samples_to_emit)

    @staticmethod
    def _apply_gain(samples: list[int], gain_db: float) -> list[int]:
        gain = 10.0 ** (gain_db / 20.0)
        return [max(-32768, min(32767, round(s * gain))) for s in samples]

    def _parse_caps(self, data: bytes) -> dict | None:
        if len(data) < 7 or data[0] != 0x0B:
            return None
        version = int.from_bytes(data[1:3], "big")
        if version >= 0x0100:
            codecs = data[3]
            interaction = data[4]
            frame_size = int.from_bytes(data[5:7], "big")
            if codecs == 0 and len(data) >= 9 and data[4] & 0x03:
                codecs = data[4]
                interaction = 0x03
        else:
            if len(data) < 9:
                return None
            codecs = data[4]
            interaction = 0x00
            frame_size = int.from_bytes(data[5:7], "big")
        selected_codec = 0x02 if codecs & 0x02 else 0x01
        return {
            "version": version,
            "codecs": codecs,
            "interaction": interaction,
            "frame_size": frame_size,
            "selected_codec": selected_codec,
            "sample_rate": 16000 if selected_codec == 0x02 else 8000,
        }

    def _watchdog_loop(self):
        """独立看门狗线程: 不受 runloop 阻塞影响, 每 500ms 检查一次。

        - 断连: BLE 已断连但 mic_open 还没清 → 强制清理
        - 音频静音: mic_open 但超过 2s 没收到音频帧 → 0x00 可能丢了
          (不用 1s, BLE 正常抖动偶尔会有 1s 间隔)
        - 会话超时: 不限制, 用户可能长时间说话; 由音频静音 + 循环检测兜底
        """
        while not self._watchdog_stop:
            time.sleep(0.5)
            if not self.mic_open:
                continue
            now = time.monotonic()

            if not self._connected:
                print(f"{self._ts()} 🚨 看门狗: BLE 已断连, 强制清理", flush=True)
                self._end_speech()
                continue

            audio_silence = 2.0       # 2s 无音频帧 (正常间隔 15ms)

            silence = now - self._last_audio_time

            if silence > audio_silence:
                print(f"{self._ts()} 🚨 看门狗: 音频静音超时 ({silence:.1f}s 无音频), 强制结束", flush=True)
                self._end_speech()

    def _cleanup(self):
        self._watchdog_stop = True
        if self._watchdog_thread:
            self._watchdog_thread.join(timeout=2)

        if self.mic_open:
            self._send_mic_close()
            self.mic_open = False
        self._session_state = VoiceSessionState.IDLE

        if self.audio_output:
            self.audio_output.close()

        if self.debug_recorder:
            self.debug_recorder.close()

        if self.audio_char:
            self.peripheral.setNotifyValue_forCharacteristic_(False, self.audio_char)
        if self.control_char:
            self.peripheral.setNotifyValue_forCharacteristic_(False, self.control_char)

        if self.manager and self.peripheral:
            self.manager.cancelPeripheralConnection_(self.peripheral)

        print(f"\n✅ 完成: {self.audio_frames} 帧, {self.pcm_samples} 采样", flush=True)


def main():
    import argparse
    parser = argparse.ArgumentParser(description="Xiaomi Remote 2 Pro 语音桥接 (→ BlackHole)")
    parser.add_argument("--duration", type=float, default=0, help="最长运行时长(秒), 0=不限")
    parser.add_argument("--debug", action="store_true", help="保存 voice_raw.wav + voice_processed.wav")
    parser.add_argument("--verbose", "-v", action="store_true", help="打印每条 BLE 通知 (调试用)")
    args = parser.parse_args()

    delegate = VoiceCaptureDelegate.alloc().init()
    delegate._start_capture(debug=args.debug, verbose=args.verbose)

    deadline = time.time() + args.duration if args.duration > 0 else float('inf')
    try:
        while time.time() < deadline and not delegate.stop_event:
            NSRunLoop.currentRunLoop().runUntilDate_(NSDate.dateWithTimeIntervalSinceNow_(0.1))
    except KeyboardInterrupt:
        pass
    finally:
        if not delegate.started:
            print("\n⚠️ 未能连接或启动语音服务")
        delegate._cleanup()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
