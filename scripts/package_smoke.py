#!/usr/bin/env python3
"""Exercise an installed package in a fresh prefix, with no model or display."""
import json
import pathlib
import subprocess
import tarfile
import tempfile

root = pathlib.Path(__file__).resolve().parent.parent
artifacts = sorted((root / "target" / "packages").glob("*-cpu.tar.gz"), key=lambda p: p.stat().st_mtime)
if not artifacts:
    raise SystemExit("Build a CPU package first")
with tempfile.TemporaryDirectory(prefix="subveil-install-smoke-") as temporary:
    work = pathlib.Path(temporary)
    with tarfile.open(artifacts[-1]) as archive:
        archive.extractall(work / "unpacked", filter="data")
    package = next((work / "unpacked").iterdir())
    subprocess.run(["sha256sum", "--check", "--quiet", "SHA256SUMS"], cwd=package, check=True, timeout=15)
    prefix = work / "installed with space"
    subprocess.run(["bash", str(package / "install.sh"), str(prefix)], check=True, timeout=15)
    app = prefix / "bin" / "subveil"
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(work)}
    subprocess.run([str(app), "--check-installation"], env=environment, check=True, timeout=15)
    helper = prefix / "lib" / "subveil" / "subveil-whisper"
    helper.unlink()
    result = subprocess.run([str(app), "--check-installation"], env=environment, capture_output=True, timeout=15)
    assert result.returncode != 0, "missing helper accepted"
    mismatched = {"Capabilities": {"protocol": 999, "version": "wrong", "build_id": "wrong", "helper": "whisper", "backends": ["cpu"]}}
    helper.write_text("#!/bin/sh\nprintf '%s\\n' '" + json.dumps(mismatched) + "'\n")
    helper.chmod(0o755)
    result = subprocess.run([str(app), "--check-installation"], env=environment, capture_output=True, timeout=15)
    assert result.returncode != 0, "mismatched helper accepted"
    assert "版本不匹配" in result.stderr.decode(), "wrong failure reason"
print("Installed layout, checksums, empty environment, missing helper and protocol mismatch: passed")
