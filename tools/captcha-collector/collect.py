#!/usr/bin/env python3
"""
验证码样本采集工具

用于批量抓取赣南师范大学统一身份认证平台的验证码图片，
为训练纯 Rust 模板匹配识别器准备数据集。

采集到的样本需要人工标注，标注结果保存为同名的 .txt 文件，
每行只写 4 个字符（例如 `aB3d`）。

用法:
    python3 collect.py --count 300 --out captcha_samples
    python3 collect.py --count 50 --out samples --interval 1.0

注意:
    请控制采集频率，避免对学校服务器造成压力。
    建议 interval 不低于 0.5 秒，单次采集不超过 500 张。
"""

import argparse
import base64
import json
import os
import secrets
import sys
import time
import urllib.request
from pathlib import Path

KAPTCHA_URL = "https://cas.gnnu.edu.cn/lyuapServer/kaptcha"

HEADERS = {
    "X-Requested-With": "XMLHttpRequest",
    "User-Agent": (
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/120.0 Safari/537.36"
    ),
}


def fetch_one(timeout: int = 20) -> tuple[bytes, str]:
    """抓取一张验证码，返回 (图片字节, uid)"""
    request_id = secrets.token_hex(16)
    url = f"{KAPTCHA_URL}?id={request_id}"

    req = urllib.request.Request(url, headers=HEADERS)
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        payload = json.loads(resp.read())

    raw = payload["content"]
    # 剥离 data URL 前缀
    if ";base64," in raw:
        raw = raw.split(";base64,", 1)[1]

    return base64.b64decode(raw), payload["uid"]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="采集教务系统验证码样本"
    )
    parser.add_argument(
        "--count", type=int, default=100,
        help="采集数量（默认 100，建议不超过 500）",
    )
    parser.add_argument(
        "--out", type=str, default="captcha_samples",
        help="输出目录（默认 captcha_samples）",
    )
    parser.add_argument(
        "--interval", type=float, default=0.5,
        help="每次请求间隔秒数（默认 0.5，不建议低于 0.3）",
    )
    parser.add_argument(
        "--timeout", type=int, default=20,
        help="单次请求超时秒数（默认 20）",
    )
    args = parser.parse_args()

    if args.interval < 0.3:
        print("警告: interval 低于 0.3 秒可能对服务器造成压力", file=sys.stderr)
    if args.count > 1000:
        print("警告: 单次采集超过 1000 张，建议分批进行", file=sys.stderr)

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    # 从已有文件数量推算起始序号，支持断点续采
    existing = sorted(out_dir.glob("*.png"))
    start = len(existing)
    if start:
        print(f"检测到已有 {start} 张样本，从序号 {start} 继续")

    ok = 0
    fail = 0

    for i in range(start, start + args.count):
        try:
            image, uid = fetch_one(args.timeout)
        except Exception as exc:  # noqa: BLE001
            fail += 1
            print(f"[{i:04d}] 采集失败: {exc}", file=sys.stderr)
            # 连续失败过多时提前退出
            if fail >= 10:
                print("连续失败过多，终止采集", file=sys.stderr)
                break
            time.sleep(args.interval * 2)
            continue

        png_path = out_dir / f"{i:04d}.png"
        png_path.write_bytes(image)

        # 同时记录 uid，便于后续复现
        meta_path = out_dir / f"{i:04d}.json"
        meta_path.write_text(
            json.dumps({"uid": uid, "index": i}, ensure_ascii=False),
            encoding="utf-8",
        )

        ok += 1
        if ok % 10 == 0 or ok == 1:
            print(f"已采集 {ok}/{args.count}")

        time.sleep(args.interval)

    print(f"\n完成: 成功 {ok} 张, 失败 {fail} 张")
    print(f"输出目录: {out_dir.resolve()}")
    if ok:
        print(
            "\n下一步: 人工标注每个样本的 4 位字符，"
            "保存为同名 .txt 文件（例如 0000.txt 内容为 aB3d）"
        )

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
