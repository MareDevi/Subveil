#!/usr/bin/env python3
"""Build a local Linux installation archive. Run through mise; never publishes."""
import argparse
import hashlib
import json
import pathlib
import platform
import shutil
import subprocess
import tarfile
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=["debug", "release"], default="release")
    parser.add_argument("--cuda", action="store_true")
    args = parser.parse_args()
    if platform.system() != "Linux":
        parser.error("Linux packages only")
    source = ROOT / "target" / args.profile
    names = ["subveil", "subveil-inference", "subveil-whisper"]
    if args.cuda:
        names.append("subveil-whisper-cuda")
    host = next(line.removeprefix("host: ") for line in run("rustc", "-vV").splitlines() if line.startswith("host: "))
    metadata = json.loads(run("cargo", "metadata", "--format-version", "1", "--filter-platform", host, "--locked", "--offline"))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "subveil")
    flavor = "cuda" if args.cuda else "cpu"
    name = f"subveil-{version}-linux-{platform.machine()}-{flavor}"
    destination = ROOT / "target" / "packages"
    destination.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="subveil-package-") as temporary:
        package = pathlib.Path(temporary) / name
        binary = package / "lib" / "subveil"
        binary.mkdir(parents=True)
        for filename in names:
            shutil.copy2(source / filename, binary / filename)
            subprocess.run(["strip", str(binary / filename)], check=True)
        # Check exact version, build ID, protocol and compiled backends without
        # starting a display, loading weights or touching user credentials.
        subprocess.run([str(binary / "subveil"), "--check-installation"], check=True)
        documentation = package / "share" / "doc" / "subveil"
        documentation.mkdir(parents=True)
        shutil.copy2(ROOT / "README.md", documentation)
        shutil.copy2(ROOT / "docs" / "packaging.md", documentation)
        licenses = documentation / "dependency-licenses"
        licenses.mkdir()
        inventory = []
        for dependency in metadata["packages"]:
            inventory.append({key: dependency.get(key) for key in ["name", "version", "license", "license_file", "repository"]})
            directory = pathlib.Path(dependency["manifest_path"]).parent
            # Include published crate notices. This inventory is evidence, not
            # a grant of rights for this currently unlicensed application.
            candidates = [p for p in directory.iterdir() if p.is_file() and p.name.lower().startswith(("license", "copying", "notice"))]
            for path in candidates:
                target = licenses / f"{dependency['name']}-{dependency['version']}"
                target.mkdir(exist_ok=True)
                shutil.copy2(path, target / path.name)
        (documentation / "dependencies.json").write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
        shutil.copy2(ROOT / "scripts" / "install.sh", package / "install.sh")
        (package / "install.sh").chmod(0o755)
        checksums = []
        for path in sorted(package.rglob("*")):
            if path.is_file():
                checksums.append(hashlib.sha256(path.read_bytes()).hexdigest() + "  " + str(path.relative_to(package)))
        (package / "SHA256SUMS").write_text("\n".join(checksums) + "\n")
        output = destination / f"{name}.tar.gz"
        with tarfile.open(output, "w:gz") as archive:
            archive.add(package, arcname=name)
        print(output)


if __name__ == "__main__":
    main()
