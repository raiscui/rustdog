#!/usr/bin/env python3
"""预取 ort-sys 要用的 onnxruntime 预编译包 — 让 Windows/macOS/Linux 的构建不再依赖 CDN 的即时可用性。

背景 (2026-11-26 实锤, RAI-397):
    `main` 的 `test (windows-latest)` 在 Build 就红, 一次测试都没跑到。日志显示
    ort-sys build script 先 `downloading from 'https://cdn.pyke.io/.../+directml.tar.lzma2'`,
    然后在 `build/dynamic_link.rs:42` panic (`Os { code: 3, kind: NotFound }`)。
    code 3 不是下载失败 —— 是 **copy_dylibs 的目标路径不存在**: 它把 dist 里的 DLL
    软链/复制到 `<profile>/`、`<profile>/examples/`、`<profile>/deps/` 三处,
    而 `cargo build --bin rdog` 从不创建 `<profile>/examples`。Linux 的 dist 只有
    `libonnxruntime.a` (静态), 匹配不到 `.so` 所以这条路从未被走到; Windows 的 dist
    带 `DirectML.dll`, 于是必踩。

本脚本负责"获取可复现"这一半 (panic 那一半由 ci.yml 的 profile 目录步骤处理):
    1. 目标三元组 → dist 行的映射**不抄写**, 从 Cargo.lock 锁定的那个 ort-sys crate
       内嵌的 `build/download/dist.tsv` 读取 (真相源随 crate 版本走);
    2. 挑行规则与 ort-sys `resolve_dist()` 一致: 本仓库没有开启任何 ort backend
       feature, 请求集合为空, 所有候选的公共 feature 数为 0, 稳定排序取该目标的
       **第一行** (Windows 上是 `+directml`、macOS 上是 `+coreml`, 这是 pyke 的基线包);
    3. 校验 sha256 后按 pyke 自己的缓存布局落到
       `<cache_root>/dfbin/<target>/<sha256>/`, 于是 ort-sys build script 直接命中
       (`bin_extract_dir.exists()`), 完全跳过网络;
    4. 已命中则秒退, 幂等。

用法:
    python3 scripts/ci/prefetch-ort-dist.py [--target TARGET] [--cache-root DIR] [--repo-root .]
退出码: 0 = 缓存已就绪 (命中或刚预取); 1 = 拿不到/校验不过; 2 = 用法错误
"""

from __future__ import annotations

import argparse
import hashlib
import lzma
import os
import platform
import shutil
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ORT_SYS_CRATE = "ort-sys"
DIST_TABLE_MEMBER = "build/download/dist.tsv"
PYKE_ROOT = "ort.pyke.io"
DOWNLOAD_ATTEMPTS = 3
# pyke 的 .tar.lzma2 是裸 LZMA2 流 (没有 xz/alone 头)。Python 的 FORMAT_RAW 需要
# 显式 filter, 属性在流内自描述, dict_size 只需给到不小于编码器的值即可。
LZMA2_DICT_CANDIDATES = (1 << 26, 1 << 27, 1 << 28)


def annotate(level: str, title: str, message: str) -> None:
    """GitHub Actions 注解; 本地跑时退化成 stderr。"""
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::{level} title={title}::{message}")
    else:
        print(f"[{level}] {title}: {message}", file=sys.stderr)


def step_summary(text: str) -> None:
    path = os.environ.get("GITHUB_STEP_SUMMARY")
    if not path:
        return
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(text.rstrip() + "\n")


def cargo_home() -> Path:
    return Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo")


def host_target() -> str:
    """等价于 `rustc -vV` 的 host 行。"""
    import subprocess

    out = subprocess.run(
        ["rustc", "-vV"], capture_output=True, text=True, check=True
    ).stdout
    for line in out.splitlines():
        if line.startswith("host:"):
            return line.split(":", 1)[1].strip()
    raise SystemExit("could not read host triple from `rustc -vV`")


def pyke_cache_root() -> Path:
    """镜像 ort-sys `src/internal/dirs.rs` 的 cache_dir()。"""
    system = platform.system()
    if system == "Windows":
        base = os.environ.get("LOCALAPPDATA") or str(Path.home() / "AppData" / "Local")
        return Path(base) / PYKE_ROOT
    if system == "Darwin":
        return Path.home() / "Library" / "Caches" / PYKE_ROOT
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / PYKE_ROOT


def ort_sys_version(repo_root: Path) -> str:
    """从 Cargo.lock 读锁定版本 (oar-ocr-core 写死 `=2.0.0-rc.13`, 这里只做对账)"""
    in_pkg = False
    for line in (repo_root / "Cargo.lock").read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line == "[[package]]":
            in_pkg = False
        elif line == f'name = "{ORT_SYS_CRATE}"':
            in_pkg = True
        elif in_pkg and line.startswith("version = "):
            return line.split('"')[1]
    raise SystemExit(f"Cargo.lock has no {ORT_SYS_CRATE} package entry")


def dist_table_text(version: str) -> str:
    """取 dist 表: 优先 registry/src 里的已解压 crate, 否则从 registry/cache 的 .crate 读。"""
    root = cargo_home() / "registry"
    for table in sorted(root.glob(f"src/*/{ORT_SYS_CRATE}-{version}/{DIST_TABLE_MEMBER}")):
        return table.read_text(encoding="utf-8")
    for crate in sorted(root.glob(f"cache/*/{ORT_SYS_CRATE}-{version}.crate")):
        with tarfile.open(crate) as archive:
            member = archive.extractfile(f"{ORT_SYS_CRATE}-{version}/{DIST_TABLE_MEMBER}")
            if member is not None:
                return member.read().decode("utf-8")
    raise SystemExit(
        f"cannot find {DIST_TABLE_MEMBER} for {ORT_SYS_CRATE} {version}; run `cargo fetch --locked` first"
    )


def resolve_dist(version: str, target: str) -> tuple[str, str]:
    """按 ort-sys resolve_dist() 的规则取该目标的第一行 (请求 feature 集合为空)。"""
    rows = [
        line.split("\t")
        for line in dist_table_text(version).splitlines()[1:]
        if line and not line.startswith("#")
    ]
    for row in rows:
        if len(row) == 4 and row[0] == target:
            return row[2], row[3]
    raise SystemExit(
        f"{ORT_SYS_CRATE} {version} ships no prebuilt binaries for {target} "
        f"(no row in {DIST_TABLE_MEMBER})"
    )


def fetch(url: str) -> bytes:
    last: Exception | None = None
    for attempt in range(1, DOWNLOAD_ATTEMPTS + 1):
        try:
            request = urllib.request.Request(
                url, headers={"User-Agent": "rustdog-ci-prefetch-ort-dist"}
            )
            with urllib.request.urlopen(request, timeout=300) as response:
                return response.read()
        except (urllib.error.URLError, TimeoutError) as error:
            last = error
            print(f"prefetch attempt {attempt}/{DOWNLOAD_ATTEMPTS} failed: {error}", file=sys.stderr)
            if attempt < DOWNLOAD_ATTEMPTS:
                time.sleep(5 * attempt)
    raise SystemExit(f"download failed after {DOWNLOAD_ATTEMPTS} attempts: {last}")


def decompress_lzma2(data: bytes) -> bytes:
    last: Exception | None = None
    for dict_size in LZMA2_DICT_CANDIDATES:
        try:
            return lzma.decompress(
                data,
                format=lzma.FORMAT_RAW,
                filters=[{"id": lzma.FILTER_LZMA2, "dict_size": dict_size}],
            )
        except lzma.LZMAError as error:
            last = error
            continue
    raise SystemExit(f"cannot decode LZMA2 payload: {last}")


def extract_into(tar_bytes: bytes, destination: Path) -> list[str]:
    """解包到 destination; 拒绝绝对路径与 `..`, 只接受普通文件与目录。"""
    written: list[str] = []
    with tempfile.SpooledTemporaryFile(max_size=64 << 20) as spool:
        spool.write(tar_bytes)
        spool.seek(0)
        with tarfile.open(fileobj=spool) as archive:
            for member in archive.getmembers():
                name = Path(member.name)
                if name.is_absolute() or any(part == ".." for part in name.parts):
                    raise SystemExit(f"refusing unsafe archive member: {member.name}")
                if member.isdir():
                    (destination / name).mkdir(parents=True, exist_ok=True)
                    continue
                if not member.isfile():
                    raise SystemExit(f"refusing non-regular archive member: {member.name}")
                out_path = destination / name
                out_path.parent.mkdir(parents=True, exist_ok=True)
                source = archive.extractfile(member)
                if source is None:
                    raise SystemExit(f"archive member has no data: {member.name}")
                with out_path.open("wb") as handle:
                    shutil.copyfileobj(source, handle)
                written.append(member.name)
    return written


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", default=None, help="默认取 rustc host")
    parser.add_argument("--cache-root", default=None, help="默认取 ort-sys 自己的 cache 根")
    parser.add_argument("--repo-root", default=".", help="含 Cargo.lock 的目录")
    args = parser.parse_args()

    repo_root = Path(args.repo_root).resolve()
    target = args.target or host_target()
    cache_root = Path(args.cache_root) if args.cache_root else pyke_cache_root()
    version = ort_sys_version(repo_root)
    url, digest = resolve_dist(version, target)
    install_dir = cache_root / "dfbin" / target / digest

    print(f"ort-dist: {ORT_SYS_CRATE} {version} → {target}")
    print(f"ort-dist: dist {url.rsplit('/', 1)[-1]} sha256={digest}")

    existing = sorted(p.name for p in install_dir.glob("*")) if install_dir.is_dir() else []
    if existing:
        print(f"ort-dist: HIT {install_dir} ({', '.join(existing)})")
        step_summary(
            f"**ort dist cache hit** `{target}` — `{', '.join(existing)}` (sha256 `{digest[:12]}…`)"
        )
        return 0

    if install_dir.exists():
        shutil.rmtree(install_dir)

    print(f"ort-dist: MISS, downloading {url}")
    payload = fetch(url)
    actual = hashlib.sha256(payload).hexdigest()
    if actual != digest:
        annotate(
            "error",
            "onnxruntime dist digest mismatch",
            f"{url} sha256={actual}, expected {digest}; refusing to seed the ort cache",
        )
        return 1

    install_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f"tmp.{digest[:8]}.", dir=install_dir.parent))
    try:
        written = extract_into(decompress_lzma2(payload), staging)
        if not written:
            annotate("error", "onnxruntime dist empty", f"{url} extracted no files")
            return 1
        if install_dir.exists():  # 并发/半成品防护
            shutil.rmtree(install_dir)
        os.replace(staging, install_dir)
    finally:
        if staging.exists():
            shutil.rmtree(staging, ignore_errors=True)

    print(f"ort-dist: PREFETCHED {install_dir} ({', '.join(written)})")
    step_summary(
        f"**ort dist prefetched** `{target}` — `{', '.join(written)}` (sha256 `{digest[:12]}…` verified)"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit:
        raise
    except Exception as error:  # 任何其它异常同样要变成 CI 注解而不是裸 traceback
        annotate("error", "onnxruntime dist unavailable", f"{type(error).__name__}: {error}")
        raise SystemExit(1)
