#!/usr/bin/env python3
"""Run the real receiver against valid, corrupt, reordered and competing UDP senders."""
import pathlib
import socket
import struct
import subprocess
import tempfile
import time

binary = pathlib.Path(__file__).resolve().parents[1] / 'desktop/target/release/receiver'
with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as allocation:
    allocation.bind(('127.0.0.1', 0))
    port = allocation.getsockname()[1]
with tempfile.TemporaryFile(mode='w+') as log:
    proc = subprocess.Popen([str(binary), f'127.0.0.1:{port}'], stdout=log, stderr=subprocess.STDOUT)
    try:
        def output():
            log.seek(0)
            return log.read()
        for _ in range(100):
            if 'Flick UDP' in output():
                break
            assert proc.poll() is None, output()
            time.sleep(.02)
        else:
            raise AssertionError('Receiver did not bind')
        target = ('127.0.0.1', port)
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sender, socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as intruder:
            for _ in range(30):
                stamp = time.monotonic_ns()
                sender.sendto(struct.pack('>Qffff', stamp, .5, 1., -2., 9.81), target)
                sender.sendto(struct.pack('>Qffff', stamp - 1, -9., 1., -2., 9.81), target)
                intruder.sendto(struct.pack('>Qffff', stamp + 1, -8., 1., -2., 9.81), target)
                sender.sendto(struct.pack('>Qffff', stamp + 1, float('inf'), 0., 0., 0.), target)
                sender.sendto(struct.pack('>Qffff', stamp + 1, float('nan'), float('nan'), 0., 0.), target)
                sender.sendto(b'bad', target)
                time.sleep(1 / 60)
            time.sleep(.4)
            assert 'Telemetry stale' in output(), output()
            for _ in range(20):
                sender.sendto(struct.pack('>Qffff', time.monotonic_ns(), -.25, 1., -2., 9.81), target)
                time.sleep(1 / 60)
            for _ in range(20):
                sender.sendto(struct.pack('>Qffff', time.monotonic_ns(), float('nan'), 3., -2., 9.81), target)
                time.sleep(1 / 60)
        text = output()
        assert '+0.500 m/s' in text and '-0.250 m/s' in text, text
        assert 'no 20 kHz lock (motion only) | accel [ +3.000' in text, text
        assert '-9.000 m/s' not in text and '-8.000 m/s' not in text and 'NaN' not in text, text
        assert proc.poll() is None, text
        print('PASS real UDP receiver: valid packets, motion-only packets, corruption, reordering, peer ownership, stale status and recovery')
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
