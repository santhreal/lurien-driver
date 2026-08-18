#!/usr/bin/env python3
"""The server half of the `audio` fixture: a spoken code that only exists as sound.

A code is minted per nonce and kept here. The page is handed `sha256(code)` and
grades the typed answer against it, so the digits are never in the DOM. The
recording is synthesized on request with the host's own speech synthesizer and
carries the noise a vendor's clip carries, so nothing is committed and a clip
cannot be recognized by its file name or its bytes.

The recipe is the one `captcha/vision/tests/audio_transcription.rs` measures
against: espeak-ng en-gb at 120 words per minute with a 25 ms gap between digits,
then hiss, mains hum and a rumbling second speaker. Both proofs have to be reading
the same kind of clip or neither says anything about the other.

`?audio=noise` mints a nonce whose recording is the noise without the speech: a
recording that says nothing, for the phase that asserts a thin reading is refused
rather than typed.

Usage: audio_fixture.py --port PORT --fixtures DIR [--child-host 127.0.0.1]
"""

import argparse
import hashlib
import http.server
import io
import math
import os
import random
import secrets
import subprocess
import tempfile
import threading
import wave

RATE = 22050
NOISE_SECONDS = 2.5
DIGITS = 5

lock = threading.Lock()
minted = {}
clips = {}
args = None


def mint(mode):
    """A fresh nonce, the code behind it, and the digest the page grades against."""
    code = "".join(random.choice("0123456789") for _ in range(DIGITS))
    nonce = secrets.token_hex(8)
    with lock:
        minted[nonce] = (code, mode)
    return nonce, hashlib.sha256(code.encode()).hexdigest()


def speak(code):
    """The code read aloud, digit by digit, as 16-bit mono samples."""
    spoken = " ".join(code)
    with tempfile.TemporaryDirectory() as dir:
        path = os.path.join(dir, "clip.wav")
        subprocess.run(
            ["espeak-ng", "-v", "en-gb", "-s", "120", "-g", "25", "-w", path, spoken],
            check=True,
            capture_output=True,
        )
        with wave.open(path, "rb") as source:
            rate = source.getframerate()
            width = source.getsampwidth()
            channels = source.getnchannels()
            frames = source.readframes(source.getnframes())
    if width != 2:
        raise RuntimeError(f"espeak-ng wrote {width * 8}-bit samples, not 16")
    samples = [
        int.from_bytes(frames[i : i + 2], "little", signed=True) / 32768.0
        for i in range(0, len(frames), 2)
    ]
    if channels > 1:
        samples = [
            sum(samples[i : i + channels]) / channels
            for i in range(0, len(samples), channels)
        ]
    return samples, rate


def distort(samples, rate, seed):
    """Hiss, 120 Hz mains hum, and a second speaker rumbling under the first.

    Deterministic per nonce, so a failing run is reproducible from its evidence.
    The levels are the ones the Rust proof measures with: chosen so an
    unconstrained transcript of the clean clip still reads the digits, which is
    what makes this noise and not an unreadable clip.
    """
    state = (seed * 6364136223846793005 + 1) % (1 << 64)

    def noise():
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return ((state >> 33) / float((1 << 31) - 1)) - 1.0

    out = []
    for n, sample in enumerate(samples):
        t = n / rate
        hiss = 0.02 * noise()
        hum = 0.01 * math.sin(2.0 * math.pi * 120.0 * t)
        babble = (
            0.12
            * math.sin(2.0 * math.pi * 190.0 * t)
            * (0.5 + 0.5 * math.sin(2.0 * math.pi * 3.1 * t))
        )
        out.append(sample + hiss + hum + babble)
    peak = max((abs(sample) for sample in out), default=0.0)
    if peak > 0.0:
        out = [sample * 0.95 / peak for sample in out]
    return out


def wav(samples, rate):
    """The samples as a WAV container, the way a widget serves one."""
    buffer = io.BytesIO()
    with wave.open(buffer, "wb") as sink:
        sink.setnchannels(1)
        sink.setsampwidth(2)
        sink.setframerate(rate)
        sink.writeframes(
            b"".join(
                int(max(-1.0, min(1.0, sample)) * 32767).to_bytes(
                    2, "little", signed=True
                )
                for sample in samples
            )
        )
    return buffer.getvalue()


def clip(nonce):
    """The recording behind a nonce, synthesized once and then remembered."""
    with lock:
        if nonce in clips:
            return clips[nonce]
        if nonce not in minted:
            return None
        code, mode = minted[nonce]
    seed = int(nonce[:8], 16)
    if mode == "noise":
        samples = [0.0] * int(RATE * NOISE_SECONDS)
        rate = RATE
    else:
        samples, rate = speak(code)
    bytes = wav(distort(samples, rate, seed), rate)
    with lock:
        clips[nonce] = bytes
    return bytes


def page(name, replacements):
    with open(os.path.join(args.fixtures, name), "r", encoding="utf-8") as source:
        text = source.read()
    for key, value in replacements.items():
        text = text.replace(key, value)
    return text.encode()


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *ignored):
        pass

    def send(self, status, body, kind):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path, _, query = self.path.partition("?")
        mode = ""
        for pair in query.split("&"):
            key, _, value = pair.partition("=")
            if key == "audio":
                mode = value
        if path == "/parent.html":
            child = f"http://{args.child_host}:{args.port}/challenge_audio.html"
            if mode:
                child = f"{child}?audio={mode}"
            return self.send(
                200, page("challenge_audio_parent.html", {"CHILD_URL": child}), "text/html"
            )
        if path == "/challenge_audio.html":
            nonce, digest = mint(mode)
            return self.send(
                200,
                page(
                    "challenge_audio.html",
                    {"FIXTURE_NONCE": nonce, "FIXTURE_HASH": digest},
                ),
                "text/html",
            )
        if path == "/mint":
            nonce, digest = mint(mode)
            body = f'{{"nonce":"{nonce}","hash":"{digest}"}}'.encode()
            return self.send(200, body, "application/json")
        if path.startswith("/clip/") and path.endswith(".wav"):
            body = clip(path[len("/clip/") : -len(".wav")])
            if body is None:
                return self.send(404, b"no such recording", "text/plain")
            return self.send(200, body, "audio/wav")
        return self.send(404, b"not found", "text/plain")


def main():
    global args
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--child-host", default="127.0.0.1")
    parser.add_argument("--bind", default="0.0.0.0")
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer((args.bind, args.port), Handler)
    server.serve_forever()


if __name__ == "__main__":
    main()
