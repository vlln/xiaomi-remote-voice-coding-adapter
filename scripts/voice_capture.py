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
import struct
import sys
import time
import threading
import wave
from collections import deque
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


# ── 音频输出到 BlackHole ─────────────────────────────────────

class AudioOutput:
    """独立线程写入 BlackHole, 避免阻塞 BLE RunLoop"""

    def __init__(self, debug_wav: bool = False):
        self._buffer = deque()
        self._lock = threading.Lock()
        self._running = False
        self._draining = False
        self._thread: threading.Thread | None = None
        self._written = 0
        self._previous = 0
        self._have_previous = False
        self._debug = debug_wav
        self._debug_wav: wave.Wave_write | None = None

    def start(self):
        if self._running:
            return
        # 清除上一轮残留数据, 防止旧录音串入新触发
        self._draining = False
        with self._lock:
            self._buffer.clear()
        # 等待旧线程退出 (buffer 已清空, 会很快结束)
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=2)
        self._thread = None

        devices = sd.query_devices()
        bh_idx = None
        for i, dev in enumerate(devices):
            if "BlackHole 2ch" in dev["name"]:
                bh_idx = i
                break
        if bh_idx is None:
            raise RuntimeError("BlackHole 2ch 未找到")

        if self._debug:
            self._debug_wav = wave.open("voice_debug.wav", "wb")
            self._debug_wav.setnchannels(1)
            self._debug_wav.setsampwidth(2)
            self._debug_wav.setframerate(16000)

        self._have_previous = False
        self._written = 0
        self._running = True
        self._thread = threading.Thread(target=self._run, args=(bh_idx,), daemon=True)
        self._thread.start()
        print(f"  BlackHole 音频输出已启动 (device={bh_idx})", flush=True)

    def write(self, samples: list[int]):
        if not self._running:
            return
        with self._lock:
            self._buffer.extend(samples)

    def _run(self, device_idx: int):
        # 预缓冲: 攒够 ~2 帧 (30ms @16kHz) 再开流, 吸收 BLE 通知突发/抖动
        # (实测 BLE 通知间隔会在 1ms~27ms 间跳动, 不预缓冲会 underrun)
        pre_buffer = 480  # 2 帧 × 240 samples
        while self._running and len(self._buffer) < pre_buffer:
            time.sleep(0.003)
        # 若等待期间被 abort (非 drain), 直接退出
        if not self._running and not self._draining:
            return

        stream = sd.OutputStream(
            device=device_idx,
            samplerate=48000,
            channels=2,
            dtype='int16',
            blocksize=480,  # 10ms
            latency=0.05,   # 50ms 内部缓冲, 吸收 BLE 27ms 抖动 (原 'low' ~10ms 会 underrun)
        )
        stream.start()

        try:
            # _running=False 后仍继续排空 _buffer (drain 模式), 避免残留数据串入下一轮
            while self._running or (self._draining and self._buffer):
                with self._lock:
                    if not self._buffer:
                        time.sleep(0.005)
                        continue
                    chunk = list(self._buffer)
                    self._buffer.clear()

                if self._debug_wav:
                    for s in chunk:
                        self._debug_wav.writeframesraw(struct.pack("<h", s))

                # 16kHz→48kHz 线性插值 (跨块连续, 与 reference 一致)
                output = np.zeros((len(chunk) * 3, 2), dtype=np.int16)
                previous = self._previous if self._have_previous else chunk[0]
                cursor = 0
                for current in chunk:
                    delta = current - previous
                    output[cursor, 0] = previous + round(delta / 3.0)
                    output[cursor, 1] = output[cursor, 0]
                    output[cursor + 1, 0] = previous + round(delta * (2.0 / 3.0))
                    output[cursor + 1, 1] = output[cursor + 1, 0]
                    output[cursor + 2, 0] = current
                    output[cursor + 2, 1] = current
                    cursor += 3
                    previous = current
                self._previous = chunk[-1]
                self._have_previous = True

                stream.write(output)
                self._written += len(chunk)
        finally:
            # drain: 等待 PortAudio 内部缓冲播完; 中断: 立即丢弃
            if self._draining:
                stream.stop()
            else:
                stream.abort()
            stream.close()
            if self._debug_wav:
                self._debug_wav.close()
                print(f"  调试 WAV 已保存: voice_debug.wav", flush=True)
            print(f"  音频输出: {self._written} 采样写入", flush=True)

    def stop(self, drain: bool = False):
        if drain:
            self._draining = True
            self._running = False
        else:
            self._draining = False
            self._running = False
            with self._lock:
                self._buffer.clear()
            if self._thread:
                self._thread.join(timeout=2)


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
    """自适应噪声门: 估计底噪, 说话间衰减背景噪声 (AC/气流)。

    - 仅在非语音段 (能量低于门限 4x) 更新底噪估计, 避免被语音抬高
    - 软扩展门: 门限以下按 sqrt 衰减, 避免硬切产生的断续感
    - 跨块线性插值增益, 避免咔哒声
    """

    def __init__(self):
        self._noise_floor = 1.0   # 从最小值开始, 让底噪在静音时自然适配上来
        self._gain = 1.0
        self._threshold_db = 12.0   # 门限高于底噪 12dB
        self._floor_gain = 0.15     # 关门时 -16dB
        self._attack = 0.08         # 关门速度: 慢关门 (~200ms)
        self._release = 0.35        # 开门速度: 快开门 (~50ms)

    @property
    def gain(self) -> float:
        return self._gain

    def process(self, samples: list[int]) -> list[int]:
        if not samples:
            return samples
        n = len(samples)
        e = sum(s * s for s in samples) / n

        thr_sq = self._noise_floor * (10.0 ** (self._threshold_db / 20.0)) ** 2
        # 仅在"确实安静"时更新底噪: 能量低于门限本身,
        # 避免说话时底噪被抬高导致门限上漂、增益越压越低
        if e < thr_sq:
            self._noise_floor = 0.95 * self._noise_floor + 0.05 * e
            if self._noise_floor < 1.0:
                self._noise_floor = 1.0

        # 软扩展门: 门限以上全通, 以下按 sqrt 渐衰到 floor_gain
        if e > thr_sq:
            target = 1.0
        else:
            target = max(self._floor_gain, (e / thr_sq) ** 0.5)

        coef = self._attack if target < self._gain else self._release
        new_gain = self._gain * (1 - coef) + target * coef

        # 跨块线性插值增益, 消除咔哒
        g0 = self._gain
        out = [max(-32768, min(32767, int(s * (g0 + (new_gain - g0) * (i / n)))))
               for i, s in enumerate(samples)]
        self._gain = new_gain
        return out


# ── CoreBluetooth 语音捕获 ──────────────────────────────────

class VoiceCaptureDelegate(NSObject):
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
        self.mic_open = False
        self.audio_frames = 0
        self.pcm_samples = 0
        self.stop_event = False
        self.started = False
        self._pending_audio = bytearray()
        self._skip_frames = 0
        self._audio_subscribed = False
        self._control_subscribed = False
        self._t0 = time.monotonic()    # 调试时间戳基准
        self._verbose = False          # --verbose 时打印每条 BLE 通知
        self._audio_started = False    # 当前会话是否已收到过 0x04
        self._last_activity = 0.0      # 最后一次收到音频/控制的时间 (monotonic)
        self._session_start = 0.0      # 当前会话开始时间 (monotonic)
        return self

    def _ts(self) -> str:
        return f"[+{time.monotonic() - self._t0:7.3f}s]"

    def _start_capture(self, debug: bool = False, verbose: bool = False):
        self._verbose = verbose
        self.audio_output = AudioOutput(debug_wav=debug)
        self.manager = CoreBluetooth.CBCentralManager.alloc().initWithDelegate_queue_(self, None)
        print("正在初始化蓝牙...")

    def _send_mic_open(self):
        version = self.caps.get("version", 0x0100) if self.caps else 0x0100
        cmd = bytes((0x0C, 0x00)) if version >= 0x0100 else bytes((0x0C, 0x00, 0x02))
        self.peripheral.writeValue_forCharacteristic_type_(cmd, self.tx_char, 1)
        self.mic_open = True
        self._session_start = time.monotonic()
        self._last_activity = self._session_start
        self.audio_output.start()
        print(f"{self._ts()} 已发送 MIC_OPEN: {cmd.hex()}", flush=True)

    def _send_mic_close(self):
        """发送 MIC_CLOSE, 正式关闭遥控器麦克风会话。"""
        if not self.peripheral or not self.tx_char or not self.caps:
            return
        version = self.caps.get("version", 0x0100)
        close_cmd = bytes((0x0D, 0x00)) if version >= 0x0100 else bytes((0x0D,))
        self.peripheral.writeValue_forCharacteristic_type_(close_cmd, self.tx_char, 1)
        print(f"{self._ts()} 已发送 MIC_CLOSE: {close_cmd.hex()}", flush=True)

    # ── CBCentralManagerDelegate ─────────────────────────────

    def centralManagerDidUpdateState_(self, manager):
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

    def centralManager_didDiscoverPeripheral_advertisementData_RSSI_(self, manager, peripheral, ad_data, rssi):
        name = peripheral.name() or "unnamed"
        if '小米' in str(name) or 'xiaomi' in str(name).lower() or 'mi rc' in str(name).lower():
            print(f"发现: {name}, 连接中...")
            self.peripheral = peripheral
            self.peripheral.setDelegate_(self)
            manager.stopScan()
            manager.connectPeripheral_options_(peripheral, None)

    def centralManager_didConnectPeripheral_(self, manager, peripheral):
        print(f"已连接: {peripheral.name() or 'unnamed'}")
        voice_uuid = objc.lookUpClass('CBUUID').UUIDWithString_(VOICE_SERVICE)
        peripheral.discoverServices_([voice_uuid])

    def centralManager_didFailToConnectPeripheral_error_(self, manager, peripheral, error):
        print(f"连接失败: {error}")

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
        self._last_activity = time.monotonic()
        opcode = data[0]
        print(f"{self._ts()} 控制: opcode=0x{opcode:02X} len={len(data)} mic_open={self.mic_open}", flush=True)

        if opcode == 0x0B:
            self.caps = self._parse_caps(data)
            print(f"{self._ts()} 能力: {self.caps}", flush=True)
        elif opcode == 0x08:
            print(f"{self._ts()} 🎤 语音键按下!", flush=True)
            if self.caps and not self.mic_open:
                # 仅在未开麦时 open; 已开麦时忽略, 避免重复 close+open 导致死循环
                self._send_mic_open()
        elif opcode == 0x04:
            if self._audio_started:
                # 同一会话内重复 0x04: 遥控器双发, 不能重置解码器/再跳帧
                print(f"{self._ts()} ⚠️ 重复 AUDIO_START (audio_frames={self.audio_frames} pending={len(self._pending_audio)}), 忽略重置", flush=True)
            else:
                self._audio_started = True
                print(f"{self._ts()} 🎤 音频流开始", flush=True)
                self.decoder.reset(0, 0)
                self._pending_audio = bytearray()
                self._skip_frames = 8  # 跳过前 ~100ms 消除按键声
                if not self.mic_open:
                    self._send_mic_open()
        elif opcode == 0x0A and len(data) >= 7:
            predictor = int.from_bytes(data[4:6], "big", signed=True)
            self.decoder.reset(predictor, data[6])
            print(f"{self._ts()} 音频同步: predictor={predictor} step={data[6]}", flush=True)
        elif opcode == 0x00:
            if not self.mic_open:
                print(f"{self._ts()} 忽略重复 AUDIO_STOP (mic_open=False)", flush=True)
                return
            print(f"{self._ts()} 🎤 语音结束 (遥控器通知)", flush=True)
            self._end_speech()

    def _end_speech(self):
        """结束当前语音会话: 刷尾部、停输出、发 MIC_CLOSE。"""
        self.mic_open = False
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
                    samples = self.decoder.decode_bytes(frame)
                    samples = self.noise_reducer.process(samples)
                    samples = self.noise_gate.process(samples)
                    samples = self._apply_gain(samples, 15.0)
                    self.audio_output.write(samples)
                    self.pcm_samples += len(samples)
                    tail_frames += 1
        print(f"{self._ts()} 尾部刷新: {tail_frames} 帧, pending={len(pending)} bytes", flush=True)
        self._pending_audio = bytearray()
        self._skip_frames = 0
        self._audio_started = False
        self.audio_frames = 0
        self.pcm_samples = 0
        self.noise_reducer = NoiseReducer()
        self.noise_gate = NoiseGate()
        self.audio_output.stop(drain=True)
        self._send_mic_close()

    def _handle_audio(self, data: bytes):
        if not self.mic_open or self.caps is None:
            return
        self._last_activity = time.monotonic()
        frame_size = self.caps.get("frame_size", 120)
        pending = getattr(self, '_pending_audio', bytearray())
        pending.extend(data)
        if self._verbose:
            print(f"{self._ts()} 音频通知: len={len(data)} pending={len(pending)}", flush=True)
        while len(pending) >= frame_size:
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

            samples = self.decoder.decode_bytes(frame)
            samples = self.noise_reducer.process(samples)
            samples = self.noise_gate.process(samples)
            samples = self._apply_gain(samples, 15.0)
            self.audio_output.write(samples)
            self.pcm_samples += len(samples)

            if self.audio_frames == 1 or self.audio_frames % 10 == 0:
                print(f"{self._ts()} 音频: #{self.audio_frames} 帧, {self.pcm_samples} 采样, gate_gain={self.noise_gate.gain:.3f}", flush=True)

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

    def _watchdog(self):
        """看门狗: 检测异常状态并强制结束会话。

        - 静音超时: mic_open 但超过 2s 没收到任何音频/控制 → 0x00 可能丢了
        - 会话超时: mic_open 超过 30s → 可能卡死, 强制结束
        """
        if not self.mic_open:
            return
        now = time.monotonic()
        silence_timeout = 2.0    # 2s 无活动
        max_session = 30.0       # 最长 30s

        if now - self._last_activity > silence_timeout:
            print(f"{self._ts()} 🚨 看门狗: 静音超时 ({now - self._last_activity:.1f}s 无活动), 强制结束", flush=True)
            self._end_speech()
        elif now - self._session_start > max_session:
            print(f"{self._ts()} 🚨 看门狗: 会话超时 ({now - self._session_start:.1f}s), 强制结束", flush=True)
            self._end_speech()

    def _cleanup(self):
        if self.mic_open:
            self._send_mic_close()
            self.mic_open = False

        if self.audio_output:
            self.audio_output.stop()

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
    parser.add_argument("--debug", action="store_true", help="保存 voice_debug.wav")
    parser.add_argument("--verbose", "-v", action="store_true", help="打印每条 BLE 通知 (调试用)")
    args = parser.parse_args()

    delegate = VoiceCaptureDelegate.alloc().init()
    delegate._start_capture(debug=args.debug, verbose=args.verbose)

    deadline = time.time() + args.duration if args.duration > 0 else float('inf')
    while time.time() < deadline and not delegate.stop_event:
        NSRunLoop.currentRunLoop().runUntilDate_(NSDate.dateWithTimeIntervalSinceNow_(0.1))
        delegate._watchdog()

    if not delegate.started:
        print("\n⚠️ 未能连接或启动语音服务")

    delegate._cleanup()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())