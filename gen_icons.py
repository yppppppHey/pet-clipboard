"""从透明宠物 PNG 生成 Tauri 打包所需的全部 Windows 图标（含 .ico）。

用法：
  python gen_icons.py [源PNG] [输出目录]
默认源： <仓库>/frontend/assets/pet.png
默认出： <仓库>/src-tauri/icons
"""

import os
import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:
    sys.exit("缺少 Pillow，请先安装：pip install pillow")


def default_src() -> Path:
    return Path(__file__).resolve().parent / "frontend" / "assets" / "pet.png"


def default_out() -> Path:
    return Path(__file__).resolve().parent / "src-tauri" / "icons"


src = Image.open(sys.argv[1] if len(sys.argv) > 1 else default_src()).convert("RGBA")
out_dir = Path(sys.argv[2] if len(sys.argv) > 2 else default_out())
os.makedirs(out_dir, exist_ok=True)


def square_icon(size, margin_ratio=0.06):
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    inner = max(1, int(size * (1 - margin_ratio)))
    ratio = min(inner / src.width, inner / src.height)
    w = max(1, round(src.width * ratio))
    h = max(1, round(src.height * ratio))
    resized = src.resize((w, h), Image.LANCZOS)
    canvas.paste(resized, ((size - w) // 2, (size - h) // 2), resized)
    return canvas


sizes = {
    "32x32.png": 32,
    "128x128.png": 128,
    "128x128@2x.png": 256,
    "icon.png": 512,
}
for name, size in sizes.items():
    square_icon(size).save(os.path.join(out_dir, name), "PNG")

ico_sizes = [16, 24, 32, 48, 64, 128, 256]
base = square_icon(256)
base.save(
    os.path.join(out_dir, "icon.ico"),
    format="ICO",
    sizes=[(s, s) for s in ico_sizes],
)

for name in list(sizes.keys()) + ["icon.ico"]:
    p = os.path.join(out_dir, name)
    print(name, os.path.getsize(p))
