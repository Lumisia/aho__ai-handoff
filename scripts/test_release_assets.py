import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

from verify_release_assets import CLI_TARGETS, verify


class ReleaseAssetsTest(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for target in CLI_TARGETS:
            if target.startswith("windows"):
                with zipfile.ZipFile(self.root / f"ai-handoff-cli-{target}.zip", "w") as archive:
                    for binary in ("ai-handoff.exe", "ai-handoff-host.exe"):
                        archive.writestr(binary, b"fixture")
            else:
                with tarfile.open(self.root / f"ai-handoff-cli-{target}.tar.gz", "w:gz") as archive:
                    for binary in ("ai-handoff", "ai-handoff-host"):
                        info = tarfile.TarInfo(binary)
                        info.size = 7
                        info.mode = 0o755
                        archive.addfile(info, io.BytesIO(b"fixture"))
        for arch in ("x86_64", "aarch64"):
            (self.root / f"ai-handoff-gui-windows-{arch}-setup.exe").write_bytes(b"MZfixture")
        for path in self.root.iterdir():
            self.checksum(path)

    @staticmethod
    def checksum(path):
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        path.with_name(path.name + ".sha256").write_text(f"{digest}  {path.name}\n", encoding="ascii")

    def test_complete_inventory(self):
        verify(self.root)

    def test_missing_asset(self):
        (self.root / "ai-handoff-cli-linux-aarch64.tar.gz").unlink()
        with self.assertRaisesRegex(ValueError, "Missing or empty"):
            verify(self.root)

    def test_changed_asset(self):
        (self.root / "ai-handoff-gui-windows-x86_64-setup.exe").write_bytes(b"MZchanged")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            verify(self.root)

    def test_checksum_cannot_hide_missing_host_binary(self):
        path = self.root / "ai-handoff-cli-windows-x86_64.zip"
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("ai-handoff.exe", b"fixture")
        self.checksum(path)
        with self.assertRaisesRegex(ValueError, "archive inventory"):
            verify(self.root)


if __name__ == "__main__":
    unittest.main()
