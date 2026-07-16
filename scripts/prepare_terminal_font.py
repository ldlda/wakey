#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "brotli==1.2.0",
#   "fonttools==4.63.0",
# ]
# ///
"""Rebuild Wakey's self-hosted terminal webfont from Nerd Fonts."""

from __future__ import annotations

import hashlib
import os
import shutil
import tarfile
import tempfile
import urllib.request
from pathlib import Path

from fontTools.ttLib import TTFont

NERD_FONTS_VERSION = "v3.4.0"
ARCHIVE_SHA256 = "ef552a3e638f25125c6ad4c51176a6adcdce295ab1d2ffacf0db060caf8c1582"
WOFF2_SHA256 = "50680a64466fbcc3eff68e00f84505f55681c86f8450c042416af5a8dbc1cce1"
FONT_FILE = "JetBrainsMonoNerdFontMono-Regular"
ARCHIVE_URL = (
    "https://github.com/ryanoasis/nerd-fonts/releases/download/"
    f"{NERD_FONTS_VERSION}/JetBrainsMono.tar.xz"
)

ROOT = Path(__file__).resolve().parent.parent
OUTPUT_DIR = ROOT / "ui/src/assets/fonts/jetbrains-mono-nerd"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_sha256(path: Path, expected: str) -> None:
    actual = sha256(path)
    if actual != expected:
        raise RuntimeError(
            f"checksum mismatch for {path}\nexpected: {expected}\nactual:   {actual}"
        )


def download(url: str, destination: Path) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "wakey-font-preparer"})
    with (
        urllib.request.urlopen(request, timeout=60) as response,
        destination.open("wb") as output,
    ):
        shutil.copyfileobj(response, output)


def extract_member(archive: tarfile.TarFile, name: str, destination: Path) -> None:
    source = archive.extractfile(name)
    if source is None:
        raise RuntimeError(f"missing {name} in Nerd Fonts archive")
    with source, destination.open("wb") as output:
        shutil.copyfileobj(source, output)


def install_file(source: Path, destination: Path) -> None:
    """Atomically replace an asset without exposing a partial file to Vite."""
    temporary = destination.with_name(f".{destination.name}.tmp")
    try:
        shutil.copyfile(source, temporary)
        os.chmod(temporary, 0o644)
        temporary.replace(destination)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> None:
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="wakey-terminal-font.") as temp:
        work_dir = Path(temp)
        archive_path = work_dir / "JetBrainsMono.tar.xz"
        ttf_path = work_dir / f"{FONT_FILE}.ttf"
        woff2_path = work_dir / f"{FONT_FILE}.woff2"
        license_path = work_dir / "OFL.txt"

        print(f"Downloading Nerd Fonts {NERD_FONTS_VERSION} JetBrainsMono archive")
        download(ARCHIVE_URL, archive_path)
        verify_sha256(archive_path, ARCHIVE_SHA256)

        with tarfile.open(archive_path, "r:xz") as archive:
            extract_member(archive, ttf_path.name, ttf_path)
            extract_member(archive, license_path.name, license_path)

        # FontTools otherwise stamps the current time into the head table,
        # making equivalent rebuilds produce different WOFF2 files.
        font = TTFont(ttf_path, recalcTimestamp=False)
        font.flavor = "woff2"
        font.save(woff2_path)
        verify_sha256(woff2_path, WOFF2_SHA256)

        install_file(woff2_path, OUTPUT_DIR / woff2_path.name)
        install_file(license_path, OUTPUT_DIR / license_path.name)

    print(f"Prepared {OUTPUT_DIR / f'{FONT_FILE}.woff2'}")


if __name__ == "__main__":
    main()
