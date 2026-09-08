"""Windows notification-area icons, derived from the same mark as the app icon."""
import os
import sys

from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generate_icon as g  # noqa: E402

out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "icons")
mark = g.build_icon(with_tile=False)
for size in (32, 64):
    mark.resize((size, size), Image.Resampling.LANCZOS).save(
        os.path.join(out, f"tray-{size}.png")
    )
print("wrote tray-32.png, tray-64.png")
