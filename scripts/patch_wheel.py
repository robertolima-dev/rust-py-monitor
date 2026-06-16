"""
Patches a maturin-built wheel to downgrade Metadata-Version from 2.4 to 2.1.

Maturin 1.7+ generates Metadata-Version: 2.4 (PEP 639), which includes
License-File and License-Expression fields not yet accepted by some PyPI
servers. This script strips those fields and downgrades the version so
twine upload works normally.

Usage:
    python scripts/patch_wheel.py target/wheels/*.whl
"""
import base64
import glob
import hashlib
import io
import sys
import zipfile
from pathlib import Path


def patch_wheel(whl_path: str) -> str:
    whl_path = Path(whl_path)
    print(f"Patching: {whl_path.name}")

    # Read all files from the original wheel
    files: dict[str, bytes] = {}
    with zipfile.ZipFile(whl_path) as zin:
        for name in zin.namelist():
            files[name] = zin.read(name)

    # Find the METADATA file
    metadata_key = next(k for k in files if k.endswith("/METADATA"))
    metadata = files[metadata_key].decode("utf-8")

    # Show what we're removing
    for line in metadata.splitlines():
        if line.startswith(("License-File:", "License-Expression:", "Metadata-Version:")):
            print(f"  before: {line}")

    # Downgrade metadata version and remove PEP 639 fields
    patched_lines = []
    for line in metadata.splitlines(keepends=True):
        if line.startswith("Metadata-Version:"):
            patched_lines.append("Metadata-Version: 2.1\n")
            print(f"  after:  Metadata-Version: 2.1")
        elif line.startswith("License-File:") or line.startswith("License-Expression:"):
            print(f"  removed: {line.rstrip()}")
        else:
            patched_lines.append(line)

    files[metadata_key] = "".join(patched_lines).encode("utf-8")

    # Recompute RECORD checksums (required for valid wheels)
    record_key = next(k for k in files if k.endswith("/RECORD"))
    record_lines = []
    for name, content in files.items():
        if name == record_key:
            continue
        digest = hashlib.sha256(content).digest()
        hash_str = "sha256=" + base64.urlsafe_b64encode(digest).rstrip(b"=").decode()
        record_lines.append(f"{name},{hash_str},{len(content)}\n")
    record_lines.append(f"{record_key},,\n")
    files[record_key] = "".join(record_lines).encode("utf-8")

    # Save to dist/ keeping the exact original filename (no suffix added).
    # Adding any suffix (e.g. -fixed) breaks the wheel naming convention and
    # causes PyPI to reject it with "Invalid build number".
    out_dir = whl_path.parent.parent.parent / "dist"
    out_dir.mkdir(exist_ok=True)
    out_path = out_dir / whl_path.name
    with zipfile.ZipFile(out_path, "w", compression=zipfile.ZIP_DEFLATED) as zout:
        for name, content in files.items():
            zout.writestr(name, content)

    print(f"  saved:  dist/{out_path.name}\n")
    return str(out_path)


if __name__ == "__main__":
    patterns = sys.argv[1:] or ["target/wheels/*.whl"]
    wheels = []
    for pattern in patterns:
        wheels.extend(glob.glob(pattern))

    if not wheels:
        print("No wheels found.")
        sys.exit(1)

    fixed = [patch_wheel(w) for w in wheels if "-fixed" not in w]
    print("Done. Upload with:")
    for path in fixed:
        print(f"  twine upload {path}")
