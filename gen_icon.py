"""生成 SecureNote 应用图标源文件 (1024x1024 PNG)"""
import zlib
import struct


def create_png(width, height, pixel_fn):
    """用纯 Python 生成 PNG（不依赖 PIL）"""
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter type: none
        for x in range(width):
            r, g, b, a = pixel_fn(x, y)
            raw += struct.pack("BBBB", r, g, b, a)

    def chunk(tag, data):
        c = tag + data
        crc = zlib.crc32(c) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + c + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )
    return png


def pixel(x, y):
    """圆角深蓝背景 + 白色锁形"""
    # 圆角矩形背景：逐角检查
    margin, radius = 60, 160
    L, R = margin, 1024 - margin          # 左右边界
    T, B = margin, 1024 - margin          # 上下边界
    in_bg = L <= x < R and T <= y < B
    if in_bg:
        # 四个圆角区域检查
        if x < L + radius and y < T + radius:
            dx, dy = x - (L + radius), y - (T + radius)
            if dx * dx + dy * dy > radius * radius:
                in_bg = False
        elif x >= R - radius and y < T + radius:
            dx, dy = x - (R - radius - 1), y - (T + radius)
            if dx * dx + dy * dy > radius * radius:
                in_bg = False
        elif x < L + radius and y >= B - radius:
            dx, dy = x - (L + radius), y - (B - radius - 1)
            if dx * dx + dy * dy > radius * radius:
                in_bg = False
        elif x >= R - radius and y >= B - radius:
            dx, dy = x - (R - radius - 1), y - (B - radius - 1)
            if dx * dx + dy * dy > radius * radius:
                in_bg = False
    if not in_bg:
        return (0, 0, 0, 0)

    # 渐变背景：从 #4f6ef7 到 #3a4fd0
    t = y / 1024
    r = int(0x4F + (0x3A - 0x4F) * t)
    g = int(0x6E + (0x4F - 0x6E) * t)
    b = int(0xF7 + (0xD0 - 0xF7) * t)

    # 锁体（圆角矩形）
    lock_l, lock_r = 332, 692
    lock_t, lock_b = 460, 780
    # 锁环（圆环）
    ring_cx, ring_cy, ring_r = 512, 400, 130
    ring_w = 42

    # 锁孔
    key_cx, key_cy = 512, 590
    key_r = 38
    key_slot = key_cx - 14 <= x <= key_cx + 14 and key_cy <= y <= key_cy + 90

    in_body = lock_l <= x < lock_r and lock_t <= y < lock_b
    dx, dy = x - ring_cx, y - ring_cy
    dist_sq = dx * dx + dy * dy
    in_ring = (ring_r - ring_w) ** 2 <= dist_sq <= ring_r**2 and y <= ring_cy + 30

    if in_body or in_ring:
        if in_body and key_cy - key_r <= y <= key_cy + key_r:
            ddx, ddy = x - key_cx, y - key_cy
            if ddx * ddx + ddy * ddy <= key_r * key_r:
                return (r, g, b, 255)  # 锁孔用背景色
        if in_body and key_slot:
            return (r, g, b, 255)
        return (255, 255, 255, 255)

    return (r, g, b, 255)


png = create_png(1024, 1024, pixel)
with open(r"F:\workbuddy\2026-11-09-14-05-47\securenote\app-icon.png", "wb") as f:
    f.write(png)
print("图标已生成:", len(png), "bytes")
