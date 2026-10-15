from pathlib import Path
import struct
import subprocess
import tempfile


root = Path(__file__).resolve().parents[1] / "src-tauri" / "icons"
with tempfile.TemporaryDirectory(prefix="margin-icons-") as temporary:
    images = {}
    for size in [16, 32, 48, 64, 128, 256, 512, 1024]:
        path = Path(temporary) / f"{size}.png"
        subprocess.run(
            ["rsvg-convert", "-w", str(size), "-h", str(size), str(root / "icon.svg"), "-o", str(path)],
            check=True,
        )
        images[size] = path.read_bytes()

for name, size in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256)]:
    (root / name).write_bytes(images[size])

chunks = []
for kind, size in [(b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256), (b"ic09", 512), (b"ic10", 1024)]:
    chunks.append(kind + struct.pack(">I", 8 + len(images[size])) + images[size])
body = b"".join(chunks)
(root / "icon.icns").write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)

sizes = [16, 32, 48, 64, 128, 256]
offset = 6 + 16 * len(sizes)
entries = []
for size in sizes:
    entries.append(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(images[size]), offset))
    offset += len(images[size])
(root / "icon.ico").write_bytes(
    struct.pack("<HHH", 0, 1, len(sizes)) + b"".join(entries) + b"".join(images[size] for size in sizes)
)
print("Generated PNG, ICNS, and ICO assets.")
