#!/usr/bin/env python3
"""nuubOS release key from a passphrase (EPIC-048, user decision 2026-10-12).

There is no key file: the Ed25519 release key is derived with scrypt from a
random passphrase kept in the maintainer's password manager, only in memory,
each time a release is signed.

  tools/ota/release-key.py new            make a passphrase (shown once) and
                                          write board/nuubos/common/ota/update-key.pub
  tools/ota/release-key.py check          ask the passphrase, compare with update-key.pub
  tools/ota/release-key.py sign FILE SIG  ask the passphrase, sign FILE into SIG
                                          (refused when it does not match update-key.pub)

The passphrase is asked on the terminal, or read from NUUBOS_RELEASE_PASSPHRASE
(scripts, tests); NUUBOS_RELEASE_PUBKEY replaces update-key.pub (tests). Signing goes through openssl; the derived key reaches it
through a pipe, never a file.
"""

import getpass
import hashlib
import os
import secrets
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PUBKEY = os.environ.get("NUUBOS_RELEASE_PUBKEY") or os.path.join(ROOT, "board", "nuubos", "common", "ota", "update-key.pub")
# Crockford base32 (no i, l, o, u): 25 characters = 125 random bits.
ALPHABET = "0123456789abcdefghjkmnpqrstvwxyz"
GROUPS, GROUP = 5, 5
# Changing any of these changes every key: never edit them.
SALT = b"nuubOS release key v1"
SCRYPT = dict(n=2**17, r=8, p=1, maxmem=256 * 1024 * 1024, dklen=32)
# PKCS#8 DER prefix of a raw Ed25519 private key (RFC 8410).
PKCS8_PREFIX = bytes.fromhex("302e020100300506032b657004220420")


def fail(message):
    print("[FAIL] " + message, file=sys.stderr)
    sys.exit(1)


def normalize(phrase):
    text = "".join(c for c in phrase.lower() if c.isalnum())
    if len(text) != GROUPS * GROUP or any(c not in ALPHABET for c in text):
        fail("not a nuubOS release passphrase (%d characters from %s)" % (GROUPS * GROUP, ALPHABET))
    return text


def ask():
    phrase = os.environ.get("NUUBOS_RELEASE_PASSPHRASE") or getpass.getpass("nuubOS release passphrase: ")
    return normalize(phrase)


def derive(phrase):
    return PKCS8_PREFIX + hashlib.scrypt(phrase.encode(), salt=SALT, **SCRYPT)


def openssl(args, key):
    """Run openssl with the DER key on a pipe (/dev/fd/N)."""
    read_fd, write_fd = os.pipe()
    try:
        proc = subprocess.Popen(["openssl"] + [a.replace("KEY", "/dev/fd/%d" % read_fd) for a in args],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                pass_fds=(read_fd,))
        os.close(read_fd)
        read_fd = -1
        os.write(write_fd, key)
        os.close(write_fd)
        write_fd = -1
        out, err = proc.communicate()
    finally:
        for fd in (read_fd, write_fd):
            if fd >= 0:
                os.close(fd)
    if proc.returncode != 0:
        fail("openssl: " + err.decode(errors="replace").strip())
    return out


def public_hex(key):
    der = openssl(["pkey", "-inform", "DER", "-in", "KEY", "-pubout", "-outform", "DER"], key)
    return der[-32:].hex()


def expected_public():
    try:
        with open(PUBKEY) as f:
            return "".join(f.read().split())
    except OSError:
        fail(PUBKEY + " missing: run 'release-key.py new' first")


def cmd_new():
    if os.path.exists(PUBKEY) and "--force" not in sys.argv:
        fail(PUBKEY + " exists: devices already trust that key (--force replaces it)")
    phrase = "".join(secrets.choice(ALPHABET) for _ in range(GROUPS * GROUP))
    shown = "-".join(phrase[i:i + GROUP] for i in range(0, len(phrase), GROUP))
    pub = public_hex(derive(phrase))
    with open(PUBKEY, "w") as f:
        f.write(pub + "\n")
    print("Release passphrase (save it in your password manager now; it is not stored anywhere):")
    print()
    print("    " + shown)
    print()
    print("[PASS] public key written to " + os.path.relpath(PUBKEY, ROOT) + " (commit it)")


def cmd_check():
    if public_hex(derive(ask())) != expected_public():
        fail("this passphrase does not match update-key.pub")
    print("[PASS] passphrase matches update-key.pub")


def cmd_sign(path, sig_path):
    key = derive(ask())
    if public_hex(key) != expected_public():
        fail("this passphrase does not match update-key.pub: nothing signed")
    # -rawin (Ed25519 one-shot) needs a regular file to read, not a pipe.
    sig = openssl(["pkeyutl", "-sign", "-keyform", "DER", "-inkey", "KEY", "-rawin", "-in", path], key)
    if len(sig) != 64:
        fail("unexpected signature size")
    with open(sig_path, "wb") as f:
        f.write(sig)


def main():
    args = [a for a in sys.argv[1:] if a != "--force"]
    if args == ["new"]:
        cmd_new()
    elif args == ["check"]:
        cmd_check()
    elif len(args) == 3 and args[0] == "sign":
        cmd_sign(args[1], args[2])
    else:
        print(__doc__.strip(), file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main()
