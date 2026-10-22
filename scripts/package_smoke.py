#!/usr/bin/env python3
"""Exercise available CPU/CUDA packages in fresh prefixes, without models or display."""
import json
import pathlib
import subprocess
import tarfile
import tempfile

root = pathlib.Path(__file__).resolve().parent.parent
artifacts = []
for backend in ("cpu", "cuda"):
    candidates = sorted((root / "target" / "packages").glob(f"*-{backend}.tar.gz"), key=lambda p: p.stat().st_mtime)
    if candidates:
        artifacts.append((backend, candidates[-1]))
if not artifacts or artifacts[0][0] != "cpu":
    raise SystemExit("Build a CPU package first")

with tempfile.TemporaryDirectory(prefix="subveil-install-smoke-") as temporary:
    work = pathlib.Path(temporary)
    packages = {}
    environment = {"PATH": "/usr/bin:/bin", "HOME": str(work)}
    for backend, artifact in artifacts:
        unpacked = work / backend
        with tarfile.open(artifact) as archive:
            archive.extractall(unpacked, filter="data")
        package = next(unpacked.iterdir())
        packages[backend] = package
        subprocess.run(["sha256sum", "--check", "--quiet", "SHA256SUMS"], cwd=package, check=True, timeout=15)
        prefix = work / f"{backend} installed with space"
        subprocess.run(["bash", str(package / "install.sh"), str(prefix)], check=True, timeout=15)
        app = prefix / "bin" / "subveil"
        subprocess.run([str(app), "--check-installation"], env=environment, check=True, timeout=15)
        library = prefix / "lib" / "subveil"
        cuda_helper = library / "subveil-whisper-cuda"
        assert cuda_helper.exists() == (backend == "cuda"), "unexpected CUDA helper role"
        for name in ("bundle-backend", "subveil-whisper", "subveil-inference") + (("subveil-whisper-cuda",) if backend == "cuda" else ()):
            helper = library / name
            backup = helper.with_suffix(".saved")
            helper.rename(backup)
            result = subprocess.run([str(app), "--check-installation"], env=environment, capture_output=True, timeout=15)
            assert result.returncode != 0, f"missing {name} accepted"
            backup.rename(helper)
        helper = library / "subveil-whisper"
        backup = helper.with_suffix(".saved")
        helper.rename(backup)
        mismatched = {"Capabilities": {"protocol": 999, "version": "wrong", "build_id": "wrong", "helper": "whisper", "backends": ["cpu"]}}
        helper.write_text("#!/bin/sh\nprintf '%s\\n' '" + json.dumps(mismatched) + "'\n")
        helper.chmod(0o755)
        result = subprocess.run([str(app), "--check-installation"], env=environment, capture_output=True, timeout=15)
        assert result.returncode != 0, "mismatched helper accepted"
        assert "版本不匹配" in result.stderr.decode(), "wrong failure reason"
        helper.unlink()
        backup.rename(helper)
        print(f"{backend}: checksums, fresh install, empty environment, missing helpers and protocol mismatch passed")
    if "cuda" in packages:
        prefix = work / "cuda installed with space"
        subprocess.run(["bash", str(packages["cpu"] / "install.sh"), str(prefix)], check=True, timeout=15)
        assert not (prefix / "lib" / "subveil" / "subveil-whisper-cuda").exists(), "CPU update retained old CUDA helper"
        subprocess.run([str(prefix / "bin" / "subveil"), "--check-installation"], env=environment, check=True, timeout=15)
        print("CUDA to CPU update: stale CUDA helper removed")
