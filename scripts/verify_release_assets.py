"""Verify the complete release inventory without executing or extracting it."""

import hashlib
from pathlib import Path
import sys
import tarfile
import zipfile


CLI_TARGETS = (
    "linux-x86_64", "linux-aarch64", "darwin-x86_64", "darwin-aarch64",
    "windows-x86_64", "windows-aarch64",
)


def verify(directory: Path) -> None:
    artifacts = [
        f"ai-handoff-cli-{target}.{'zip' if target.startswith('windows') else 'tar.gz'}"
        for target in CLI_TARGETS
    ] + [f"ai-handoff-gui-windows-{arch}-setup.exe" for arch in ("x86_64", "aarch64")]
    for name in artifacts:
        path = directory / name
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError(f"Missing or empty release asset: {name}")
        checksum = directory / (name + ".sha256")
        fields = checksum.read_text(encoding="ascii").split()
        with path.open("rb") as stream:
            digest = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        if len(fields) != 2 or fields[0].lower() != digest.hexdigest() or fields[1] != name:
            raise ValueError(f"Invalid SHA-256 checksum: {name}")
        if name.endswith(".zip"):
            with zipfile.ZipFile(path) as archive:
                members = archive.infolist()
                if (len(members) != 2
                        or {member.filename for member in members} != {"ai-handoff.exe", "ai-handoff-host.exe"}
                        or any(member.is_dir() or member.file_size == 0 for member in members)):
                    raise ValueError(f"Invalid CLI archive inventory: {name}")
                if archive.testzip() is not None:
                    raise ValueError(f"Corrupt CLI archive: {name}")
        elif name.endswith(".tar.gz"):
            with tarfile.open(path, "r:gz") as archive:
                members = archive.getmembers()
                if (len(members) != 2
                        or {member.name for member in members} != {"ai-handoff", "ai-handoff-host"}
                        or any(not member.isfile() or member.size == 0 or not member.mode & 0o111
                               for member in members)):
                    raise ValueError(f"Invalid CLI archive inventory or executable mode: {name}")
        elif name.endswith(".exe"):
            with path.open("rb") as stream:
                if stream.read(2) != b"MZ":
                    raise ValueError(f"Invalid Windows installer header: {name}")
    print(f"Release assets verified: {len(artifacts)} artifacts and {len(artifacts)} checksums")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python verify_release_assets.py <asset-directory>")
    verify(Path(sys.argv[1]))
