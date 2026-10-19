# xiaomi-remote

> **V2（Tauri 跨平台 App）** 位于 [`v2/`](v2/README.md)：菜单栏 App + Rust BLE/音频链路 + 设备级按键映射，支持 macOS / Windows / Linux。以下为 V1（Python + Karabiner）文档。

小米蓝牙语音遥控器 2 Pro (XMRM-XX) 的 macOS 工具集。

## 功能

- **按键映射** — Karabiner-Elements 配置,映射语音/Home/TV 键
- **语音桥接** — 遥控器麦克风 → BlackHole 虚拟麦克风,任意 App 可用

## 依赖

```bash
brew install blackhole-2ch     # 虚拟音频驱动
python3 -m pip install -e .    # Python 依赖
```

## 使用

### 语音桥接

```bash
python3 -u scripts/voice_capture.py
```

按下遥控器语音键说话,音频实时输出到 BlackHole 2ch。

**系统音频设置:**
1. 打开 **Audio MIDI Setup** → 左下角 + → **创建聚集设备**
2. 勾选 **BlackHole 2ch** → 重命名为 "Remote Mic"
3. 右键 → **将此设备用于声音输入**

### 按键映射

在 Karabiner-Elements → Complex Modifications → Add rule → 导入 `scripts/xiaomi_remote_karabiner.json`

| 按键 | 映射 |
|------|------|
| 语音 | right_alt |
| Home | escape |
| TV | backspace |

### 调试

```bash
python3 -u scripts/voice_capture.py --debug  # 同时保存 voice_raw.wav + voice_processed.wav
python3 scripts/test_blackhole.py             # 测试 BlackHole 音频
```

## 协议与参考

基于 Google ATVV (Android TV Voice) 协议实现。以下代码逻辑参考 [remote-bridge-hub](https://github.com/xxb26553663-star/remote-bridge-hub) (GPL-3.0):

- **ADPCM 解码器** (`AdpcmDecoder`): `STEP_TABLE`, `INDEX_TABLE`, `decode_nibble`/`decode_bytes` — 来自 `atvv_record.py`
- **音频后处理** (`_postprocess`): 3 点平滑滤波 + 增益 — 来自 `atvv_record.py:postprocess`
- **16kHz→48kHz 重采样** (`push_16k`): 跨块线性插值 — 来自 `atvv_live_bridge.py:UdpPcmOutput.push_16k`
- **ATVV 协议交互**: GET_CAPS、MIC_OPEN/CLOSE 命令、opcode 处理 — 来自 `atvv_record.py`/`atvv_live_bridge.py`
- **CAPS 解析** (`_parse_caps`): 版本兼容处理 (codecs==0 fallback) — 来自 `atvv_record.py:parse_caps`

## 已知限制

- 返回键 (HID usage 0x00F1) Karabiner 无法匹配
- BlackHole 需重启后生效

## 音频处理链

```
ADPCM 解码 → 跳过前 8 帧(消按键声) → 高通滤波(220Hz) → 低通平滑(3点) → 自适应噪声门(AC/气流) → 15dB 增益 → 16kHz→48kHz 重采样 → BlackHole
```
