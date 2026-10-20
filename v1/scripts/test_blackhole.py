#!/usr/bin/env python3
"""测试 BlackHole 是否有音频输入"""
import sounddevice as sd
import numpy as np

for i, d in enumerate(sd.query_devices()):
    if 'BlackHole' in d['name']:
        print(f'Recording from BlackHole (device {i}) for 2s...')
        r = sd.rec(48000 * 2, samplerate=48000, channels=2, device=i, dtype='int16')
        sd.wait()
        peak = int(np.max(np.abs(r)))
        energy = float(np.mean(r.astype(float) ** 2))
        print(f'Peak: {peak}, Energy: {energy:.2f}')
        if peak > 0:
            print('✅ BlackHole 有音频!')
        else:
            print('❌ BlackHole 静音')
        break