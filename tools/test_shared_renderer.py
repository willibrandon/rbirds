#!/usr/bin/env python3
"""A local terminal double for the macOS shared-image lifecycle tests."""

import base64
import ctypes
import errno
import fcntl
import json
import mmap
import os
import pty
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

libc = ctypes.CDLL(None, use_errno=True)
# Darwin's mode argument is variadic, including on Apple arm64.
libc.shm_open.argtypes = [ctypes.c_char_p, ctypes.c_int]
libc.shm_open.restype = ctypes.c_int
libc.shm_unlink.argtypes = [ctypes.c_char_p]
libc.shm_unlink.restype = ctypes.c_int


def absent(name):
    fd = libc.shm_open(name, os.O_RDONLY)
    if fd >= 0:
        os.close(fd)
        return False
    assert ctypes.get_errno() == errno.ENOENT
    return True


def run(binary, mode):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 640, 384))
    saved = termios.tcgetattr(slave)
    args = [binary, '--render', 'kitty', '--color', 'ember', '--birds', '32',
            '--panel', '--depth', '--trails', '--hawks', '2', '--seed', '42']
    child = subprocess.Popen(args, stdin=slave, stdout=slave, stderr=subprocess.PIPE,
                             env={'PATH': os.environ['PATH'], 'TERM': 'xterm-256color'})
    data = bytearray()
    cursor = 0
    names = set()
    images = 0
    inline = 0
    acted = False
    version_replied = False
    query_replied = False
    deadline = time.monotonic() + 8
    try:
        while time.monotonic() < deadline:
            if master >= 0 and select.select([master], [], [], .02)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    chunk = b''
                data.extend(chunk)
                if not version_replied and b'\x1b[>q' in data:
                    os.write(master, b'\x1bP>|iTerm2 3.6.6\x1b\\')
                    version_replied = True
                while True:
                    start = data.find(b'\x1b_G', cursor)
                    end = data.find(b'\x1b\\', start) if start >= 0 else -1
                    if end < 0:
                        break
                    cursor = end + 2
                    body = bytes(data[start + 3:end])
                    header, _, payload = body.partition(b';')
                    fields = dict(item.split(b'=', 1) for item in header.split(b',') if b'=' in item)
                    if fields.get(b'a') == b't' and fields.get(b'o') == b'z':
                        inline += 1
                        if mode.startswith('late') and inline >= 6 and not acted:
                            os.write(master, b'q')
                            acted = True
                    if fields.get(b't') != b's':
                        continue
                    name = base64.b64decode(payload, validate=True)
                    assert re.fullmatch(rb'/rb[0-9a-f]{24}', name), name
                    assert name[3:11] == f'{child.pid:08x}'.encode(), name
                    names.add(name)
                    if acted:
                        continue
                    fd = libc.shm_open(name, os.O_RDONLY)
                    assert fd >= 0, (mode, name, ctypes.get_errno())
                    try:
                        size = os.fstat(fd).st_size
                        wanted = int(fields[b's']) * int(fields[b'v']) * 4
                        assert 0 < wanted <= size
                        with mmap.mmap(fd, size, access=mmap.ACCESS_READ) as memory:
                            pixels = memory[:wanted]
                        assert os.fstat(fd).st_mode & 0o777 == 0o600
                    finally:
                        os.close(fd)
                    if fields.get(b'a') == b'q':
                        assert pixels == bytes(4)
                        assert libc.shm_unlink(name) == 0
                        if mode.startswith('late'):
                            prefix = b'\x1b_Gi=' + fields[b'i'] + b';EBADF:'
                            if mode == 'late_partial':
                                os.write(master, prefix)
                                prefix = b''
                            time.sleep(.25)
                            os.write(master, prefix + b' query failed h e K \x1b\\')
                        else:
                            os.write(master, b'\x1b_Gi=' + fields[b'i'] + b';OK\x1b\\')
                        query_replied = True
                    else:
                        assert query_replied and any(pixels)
                        images += 1
                        if mode == 'normal':
                            assert libc.shm_unlink(name) == 0
                            if images >= 6:
                                os.write(master, b'q')
                                acted = True
                        elif not acted:
                            acted = True
                            if mode == 'signal':
                                child.send_signal(signal.SIGTERM)
                            else:
                                os.close(master)
                                master = -1
                                break
            if child.poll() is not None:
                if master < 0 or not select.select([master], [], [], .02)[0]:
                    break
            if master < 0:
                time.sleep(.01)
        assert child.poll() is not None, (mode, 'child timed out')
        stderr = child.stderr.read().decode(errors='replace')
        expected = {'normal': 0, 'signal': 143, 'hangup': 1, 'late_partial': 0, 'late_full': 0}[mode]
        assert child.returncode == expected, (mode, child.returncode, stderr)
        assert query_replied
        assert inline >= 6 and images == 0 if mode.startswith('late') else images > 0
        if mode != 'hangup':
            assert termios.tcgetattr(slave) == saved
            assert data.endswith(b'\x1b[?1049l')
        assert all(absent(name) for name in names), (mode, 'pending shared image leaked')
        return {'mode': mode, 'images': images, 'inline': inline, 'names': len(names), 'status': child.returncode}
    finally:
        if child.poll() is None:
            child.kill()
            child.wait()
        child.stderr.close()
        for name in names:
            libc.shm_unlink(name)
        if master >= 0:
            os.close(master)
        os.close(slave)


if __name__ == '__main__':
    print(json.dumps([run(sys.argv[1], mode) for mode in ('normal', 'signal', 'hangup', 'late_partial', 'late_full')]))
