import threading
import tempfile
import time
import unittest
import wave
from pathlib import Path
from unittest import mock

from scripts import voice_capture


class FakeStream:
    instances = []
    active_writes = 0
    max_active_writes = 0
    state_lock = threading.Lock()

    def __init__(self, **kwargs):
        self.kwargs = kwargs
        self.write_sizes = []
        self.stopped = False
        self.aborted = False
        self.closed = False
        self.__class__.instances.append(self)

    def start(self):
        pass

    def write(self, output):
        with self.state_lock:
            self.__class__.active_writes += 1
            self.__class__.max_active_writes = max(
                self.__class__.max_active_writes,
                self.__class__.active_writes,
            )
        try:
            self.write_sizes.append(len(output))
            time.sleep(0.002)
        finally:
            with self.state_lock:
                self.__class__.active_writes -= 1

    def stop(self):
        self.stopped = True

    def abort(self):
        self.aborted = True

    def close(self):
        self.closed = True


class FakePeripheral:
    def __init__(self):
        self.commands = []

    def writeValue_forCharacteristic_type_(self, value, characteristic, write_type):
        self.commands.append(bytes(value))


class FailingPeripheral(FakePeripheral):
    def writeValue_forCharacteristic_type_(self, value, characteristic, write_type):
        raise RuntimeError("BLE write failed")


class FakeAudioOutput:
    def __init__(self):
        self.starts = 0
        self.stops = []
        self.writes = []

    def start(self):
        self.starts += 1

    def stop(self, drain=False):
        self.stops.append(drain)

    def write(self, samples):
        self.writes.append(samples)


class AudioOutputTests(unittest.TestCase):
    def setUp(self):
        FakeStream.instances = []
        FakeStream.active_writes = 0
        FakeStream.max_active_writes = 0
        self.patches = (
            mock.patch.object(voice_capture.sd, "query_devices", return_value=[{"name": "BlackHole 2ch"}]),
            mock.patch.object(voice_capture.sd, "OutputStream", side_effect=FakeStream),
        )
        for patcher in self.patches:
            patcher.start()

    def tearDown(self):
        for patcher in reversed(self.patches):
            patcher.stop()

    def test_waiting_consumer_does_not_block_writer(self):
        output = voice_capture.AudioOutput()
        output.start()
        time.sleep(0.02)

        started = time.monotonic()
        output.write([1] * 480)
        elapsed = time.monotonic() - started
        output.stop(drain=True)
        output._thread.join(timeout=1)

        self.assertLess(elapsed, 0.02)
        self.assertFalse(output._thread.is_alive())
        self.assertEqual(output._written, 480)

    def test_restart_cancels_old_drain_before_new_thread(self):
        output = voice_capture.AudioOutput()
        output.start()
        output.write([1] * 4800)
        output.stop(drain=True)

        output.start()
        output.write([2] * 480)
        output.stop(drain=True)
        output._thread.join(timeout=1)

        self.assertFalse(output._thread.is_alive())
        self.assertEqual(FakeStream.max_active_writes, 1)
        self.assertEqual(len(FakeStream.instances), 1)
        self.assertTrue(all(
            size <= 480 * 3
            for stream in FakeStream.instances
            for size in stream.write_sizes
        ))

    def test_backlog_is_bounded_to_recent_audio(self):
        output = voice_capture.AudioOutput()
        with output._condition:
            output._running = True

        output.write(list(range(6000)))

        with output._condition:
            self.assertEqual(len(output._buffer), output.MAX_BUFFER_SAMPLES)
            self.assertEqual(output._buffer[0], 1200)
            self.assertEqual(output._dropped, 1200)

    def test_resampler_is_continuous_across_chunks(self):
        output = voice_capture.AudioOutput()
        first = output._resample([0, 3])[:, 0].tolist()
        second = output._resample([6])[:, 0].tolist()

        self.assertEqual(first, [0, 0, 0, 1, 2, 3])
        self.assertEqual(second, [4, 5, 6])


class DebugWavRecorderTests(unittest.TestCase):
    def test_raw_and_processed_files_are_aligned(self):
        with tempfile.TemporaryDirectory() as directory:
            raw_path = Path(directory) / "raw.wav"
            processed_path = Path(directory) / "processed.wav"
            recorder = voice_capture.DebugWavRecorder(raw_path, processed_path)
            recorder.write_pair([1, -2, 3], [10, -20, 30])
            recorder.write_pair([4, 5], [40, 50])
            recorder.close()

            with wave.open(str(raw_path), "rb") as raw_wav:
                self.assertEqual(raw_wav.getframerate(), 16000)
                self.assertEqual(raw_wav.getnchannels(), 1)
                self.assertEqual(raw_wav.getnframes(), 5)
            with wave.open(str(processed_path), "rb") as processed_wav:
                self.assertEqual(processed_wav.getframerate(), 16000)
                self.assertEqual(processed_wav.getnchannels(), 1)
                self.assertEqual(processed_wav.getnframes(), 5)

    def test_mismatched_pair_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            recorder = voice_capture.DebugWavRecorder(
                Path(directory) / "raw.wav",
                Path(directory) / "processed.wav",
            )
            with self.assertRaisesRegex(ValueError, "长度不一致"):
                recorder.write_pair([1], [1, 2])
            recorder.close()


class NoiseGateTests(unittest.TestCase):
    def test_calibration_is_passthrough_then_steady_noise_closes_gate(self):
        gate = voice_capture.NoiseGate()
        noise = [20, -20] * 120

        for _ in range(gate.CALIBRATION_FRAMES):
            self.assertEqual(gate.process(noise), noise)
        self.assertEqual(gate.gain, 1.0)

        for _ in range(30):
            gate.process(noise)
        self.assertLess(gate.gain, 0.25)

    def test_speech_opens_calibrated_gate_quickly(self):
        gate = voice_capture.NoiseGate()
        noise = [20, -20] * 120
        speech = [200, -200] * 120
        for _ in range(gate.CALIBRATION_FRAMES + 30):
            gate.process(noise)

        for _ in range(4):
            gate.process(speech)
        self.assertGreater(gate.gain, 0.9)


class SessionStateTests(unittest.TestCase):
    def make_delegate(self):
        delegate = voice_capture.VoiceCaptureDelegate.alloc().init()
        delegate.caps = {"version": 0x0100, "frame_size": 120}
        delegate._connected = True
        delegate.peripheral = FakePeripheral()
        delegate.tx_char = object()
        delegate.audio_output = FakeAudioOutput()
        return delegate

    def test_control_storm_keeps_session_closing_until_quiet(self):
        delegate = self.make_delegate()
        delegate._session_state = voice_capture.VoiceSessionState.CLOSING
        delegate._closing_activity = 10.0

        self.assertTrue(delegate._closing_event_is_stale(10.5, 0x04))
        self.assertTrue(delegate._closing_event_is_stale(11.0, 0x04))
        self.assertTrue(delegate._closing_event_is_stale(11.5, 0x04))
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.CLOSING)

        self.assertFalse(delegate._closing_event_is_stale(12.3, 0x04))
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.IDLE)

    def test_mic_open_failure_stops_local_output(self):
        delegate = self.make_delegate()
        delegate.peripheral = FailingPeripheral()

        with self.assertRaisesRegex(RuntimeError, "BLE write failed"):
            delegate._send_mic_open()

        self.assertFalse(delegate.mic_open)
        self.assertEqual(delegate.audio_output.starts, 1)
        self.assertEqual(delegate.audio_output.stops, [False])

    def test_second_start_before_audio_is_logged_as_confirmation(self):
        delegate = self.make_delegate()
        delegate._session_state = voice_capture.VoiceSessionState.STREAMING
        delegate.mic_open = True
        delegate._session_start = 10.0
        delegate.audio_frames = 1

        with mock.patch.object(voice_capture.time, "monotonic", return_value=10.03):
            with mock.patch("builtins.print") as print_mock:
                delegate._handle_control(b"\x04\x00\x00\x00")

        messages = [str(call.args[0]) for call in print_mock.call_args_list]
        self.assertTrue(any("AUDIO_START 确认" in message for message in messages))
        self.assertFalse(any("⚠️" in message for message in messages))

    def test_start_during_audio_remains_a_warning(self):
        delegate = self.make_delegate()
        delegate._session_state = voice_capture.VoiceSessionState.STREAMING
        delegate.mic_open = True
        delegate.audio_frames = 10
        delegate._session_start = 10.0

        with mock.patch.object(voice_capture.time, "monotonic", return_value=10.2):
            with mock.patch("builtins.print") as print_mock:
                delegate._handle_control(b"\x04\x00\x00\x00")

        messages = [str(call.args[0]) for call in print_mock.call_args_list]
        self.assertTrue(any("⚠️ 流中重复 AUDIO_START" in message for message in messages))

    def test_tail_guard_emits_only_frames_confirmed_by_following_audio(self):
        delegate = self.make_delegate()
        raw_frames = [[1], [2], [3]]
        processed_frames = [[10], [20], [30]]

        for raw, processed in zip(raw_frames, processed_frames):
            delegate._queue_processed_frame(raw, processed)

        self.assertEqual(delegate.audio_output.writes, [[10]])
        self.assertEqual(delegate.pcm_samples, 1)
        self.assertEqual(list(delegate._tail_guard), [([2], [20]), ([3], [30])])

    def test_stop_storm_cannot_reopen_but_quiet_new_start_can(self):
        delegate = self.make_delegate()

        with mock.patch.object(voice_capture.time, "monotonic", return_value=10.0):
            delegate._handle_control(b"\x04\x00\x00\x00")
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.STREAMING)
        self.assertEqual(delegate.audio_output.starts, 1)

        with mock.patch.object(voice_capture.time, "monotonic", return_value=11.0):
            delegate._handle_control(b"\x00\x00")
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.CLOSING)

        for timestamp, opcode in ((11.1, 0x04), (11.2, 0x00), (11.8, 0x04)):
            with mock.patch.object(voice_capture.time, "monotonic", return_value=timestamp):
                delegate._handle_control(bytes((opcode, 0x00)))
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.CLOSING)
        self.assertEqual(delegate.audio_output.starts, 1)

        with mock.patch.object(voice_capture.time, "monotonic", return_value=12.6):
            delegate._handle_control(b"\x04\x00\x00\x00")
        self.assertIs(delegate._session_state, voice_capture.VoiceSessionState.STREAMING)
        self.assertEqual(delegate.audio_output.starts, 2)


if __name__ == "__main__":
    unittest.main()
