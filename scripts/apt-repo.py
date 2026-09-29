#!/usr/bin/env python3
"""Generate the apt repository the site serves under docs/apt from .deb files.

    python3 scripts/apt-repo.py --root docs/apt kintsu_0.2.0_amd64.deb kintsu_0.2.0_arm64.deb
    python3 scripts/apt-repo.py --root docs/apt --sign-key <fingerprint> ...
    python3 scripts/apt-repo.py --self-test

Pure Python: the control file is read out of each .deb (an ar archive), so this
runs anywhere, no dpkg-dev needed. New packages are copied into
pool/main/k/kintsu/, older versions beyond --keep are removed, and
dists/<suite>/main/binary-<arch>/Packages(.gz) and dists/<suite>/Release are
rewritten from what the pool holds. With --sign-key, gpg writes InRelease and
Release.gpg and the public key is exported next to them; without it, stale
signatures are removed so apt never sees a signature that does not match.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import io
import lzma
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from datetime import datetime, timezone
from email.utils import format_datetime
from pathlib import Path

AR_MAGIC = b"!<arch>\n"


def ar_members(data: bytes) -> dict[str, bytes]:
    """The members of an ar archive, by name."""
    if not data.startswith(AR_MAGIC):
        raise ValueError("not an ar archive")
    members: dict[str, bytes] = {}
    pos = len(AR_MAGIC)
    while pos + 60 <= len(data):
        header = data[pos : pos + 60]
        name = header[0:16].decode("ascii").strip().rstrip("/")
        size = int(header[48:58])
        start = pos + 60
        members[name] = data[start : start + size]
        pos = start + size + (size & 1)
    return members


def ar_archive(members: list[tuple[str, bytes]]) -> bytes:
    """An ar archive, for the self-test's fake packages."""
    out = bytearray(AR_MAGIC)
    for name, body in members:
        header = f"{name:<16}{0:<12}{0:<6}{0:<6}{'100644':<8}{len(body):<10}`\n"
        out += header.encode("ascii") + body
        if len(body) & 1:
            out += b"\n"
    return bytes(out)


def control_of(deb: bytes) -> str:
    """The text of the control file inside a .deb."""
    members = ar_members(deb)
    name = next((n for n in members if n.startswith("control.tar")), None)
    if name is None:
        raise ValueError("no control.tar member")
    raw = members[name]
    if name.endswith(".xz"):
        raw = lzma.decompress(raw)
    elif name.endswith(".gz"):
        raw = gzip.decompress(raw)
    elif name != "control.tar":
        raise ValueError(f"unsupported control compression: {name}")
    with tarfile.open(fileobj=io.BytesIO(raw)) as tar:
        for member in tar.getmembers():
            if member.name.lstrip("./") == "control":
                handle = tar.extractfile(member)
                if handle is None:
                    break
                return handle.read().decode("utf-8")
    raise ValueError("no control file")


def parse_control(text: str) -> list[tuple[str, str]]:
    """The fields in order; a continuation line stays attached to its field."""
    fields: list[tuple[str, str]] = []
    for line in text.splitlines():
        if not line.strip():
            continue
        if line[0] in " \t" and fields:
            key, value = fields[-1]
            fields[-1] = (key, f"{value}\n{line}")
        else:
            key, _, value = line.partition(":")
            fields.append((key.strip(), value.strip()))
    return fields


def field(fields: list[tuple[str, str]], name: str) -> str:
    return next(v for k, v in fields if k == name)


def version_key(version: str) -> list:
    """Orders Debian-style versions well enough for X.Y.Z releases."""
    return [int(part) if part.isdigit() else part for part in re.findall(r"\d+|[^\d.+~-]+", version)]


def digests(data: bytes) -> dict[str, str]:
    return {
        "MD5sum": hashlib.md5(data).hexdigest(),
        "SHA1": hashlib.sha1(data).hexdigest(),
        "SHA256": hashlib.sha256(data).hexdigest(),
    }


class Package:
    def __init__(self, path: Path, root: Path):
        data = path.read_bytes()
        self.path = path
        self.fields = parse_control(control_of(data))
        self.name = field(self.fields, "Package")
        self.version = field(self.fields, "Version")
        self.arch = field(self.fields, "Architecture")
        self.filename = path.relative_to(root).as_posix()
        self.size = len(data)
        self.digests = digests(data)

    def stanza(self) -> str:
        lines = [f"{k}: {v}" for k, v in self.fields if k != "Description" and v]
        lines.append(f"Filename: {self.filename}")
        lines.append(f"Size: {self.size}")
        lines.extend(f"{k}: {v}" for k, v in self.digests.items())
        lines.append(f"Description: {field(self.fields, 'Description')}")
        return "\n".join(lines) + "\n"


def pool_dir(root: Path, component: str, package: str) -> Path:
    initial = package[:4] if package.startswith("lib") else package[:1]
    return root / "pool" / component / initial / package


def add_to_pool(root: Path, component: str, debs: list[Path]) -> None:
    for deb in debs:
        name = field(parse_control(control_of(deb.read_bytes())), "Package")
        target = pool_dir(root, component, name)
        target.mkdir(parents=True, exist_ok=True)
        shutil.copy2(deb, target / deb.name)


def prune(packages: list[Package], keep: int) -> list[Package]:
    """Keeps the newest `keep` versions of each package and architecture."""
    kept: list[Package] = []
    by_key: dict[tuple[str, str], list[Package]] = {}
    for package in packages:
        by_key.setdefault((package.name, package.arch), []).append(package)
    for group in by_key.values():
        group.sort(key=lambda p: version_key(p.version), reverse=True)
        kept.extend(group[:keep])
        for old in group[keep:]:
            old.path.unlink()
    return kept


def write_gz(path: Path, data: bytes) -> None:
    # A fixed mtime keeps the file byte-identical when the content is, so a
    # re-run leaves the working tree unchanged.
    with open(path, "wb") as out, gzip.GzipFile(fileobj=out, mode="wb", mtime=0) as gz:
        gz.write(data)


def generate(
    root: Path,
    debs: list[Path],
    *,
    suite: str = "stable",
    component: str = "main",
    origin: str = "kintsu",
    label: str = "kintsu",
    description: str = "kintsu, the bubble under failed commands",
    keep: int = 3,
    sign_key: str | None = None,
    gpg: str = "gpg",
    now: datetime | None = None,
) -> list[Package]:
    root = root.resolve()
    add_to_pool(root, component, debs)
    pool = root / "pool" / component
    packages = [Package(p, root) for p in sorted(pool.rglob("*.deb"))]
    packages = prune(packages, keep)
    dist = root / "dists" / suite
    if dist.exists():
        shutil.rmtree(dist)
    arches = sorted({p.arch for p in packages})
    indexed: list[tuple[str, bytes]] = []
    for arch in arches:
        directory = dist / component / f"binary-{arch}"
        directory.mkdir(parents=True)
        ordered = sorted(packages, key=lambda p: (p.name, version_key(p.version)))
        text = "\n".join(p.stanza() for p in ordered if p.arch == arch)
        data = text.encode("utf-8")
        (directory / "Packages").write_bytes(data)
        write_gz(directory / "Packages.gz", data)
        for name in ("Packages", "Packages.gz"):
            indexed.append((f"{component}/binary-{arch}/{name}", (directory / name).read_bytes()))
    release = [
        f"Origin: {origin}",
        f"Label: {label}",
        f"Suite: {suite}",
        f"Codename: {suite}",
        f"Date: {format_datetime(now or datetime.now(timezone.utc), usegmt=True)}",
        f"Architectures: {' '.join(arches)}",
        f"Components: {component}",
        f"Description: {description}",
    ]
    for algorithm, name in (("md5", "MD5Sum"), ("sha1", "SHA1"), ("sha256", "SHA256")):
        release.append(f"{name}:")
        for path, data in indexed:
            release.append(f" {hashlib.new(algorithm, data).hexdigest()} {len(data)} {path}")
    (dist / "Release").write_text("\n".join(release) + "\n", encoding="utf-8")
    sign(dist, root, sign_key, gpg)
    return packages


def sign(dist: Path, root: Path, key: str | None, gpg: str) -> None:
    for stale in ("InRelease", "Release.gpg"):
        (dist / stale).unlink(missing_ok=True)
    if key is None:
        return
    common = [gpg, "--batch", "--yes", "--local-user", key]
    release = str(dist / "Release")
    subprocess.run(
        common + ["--armor", "--detach-sign", "--output", str(dist / "Release.gpg"), release],
        check=True,
    )
    subprocess.run(common + ["--clearsign", "--output", str(dist / "InRelease"), release], check=True)
    # The keyring apt wants in signed-by, and its readable form for people.
    export = [gpg, "--batch", "--export"]
    (root / "kintsu.gpg").write_bytes(subprocess.run(export + [key], check=True, capture_output=True).stdout)
    (root / "kintsu.asc").write_bytes(
        subprocess.run(export + ["--armor", key], check=True, capture_output=True).stdout
    )


def fake_deb(name: str, version: str, arch: str, payload: bytes = b"") -> bytes:
    control = (
        f"Package: {name}\nVersion: {version}\nArchitecture: {arch}\n"
        "Maintainer: Test <test@example.invalid>\nInstalled-Size: 1\nDepends: \n"
        "Section: utils\nPriority: optional\n"
        "Description: a test package\n line two of the description\n"
    ).encode()
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w:gz") as tar:
        info = tarfile.TarInfo("./control")
        info.size = len(control)
        tar.addfile(info, io.BytesIO(control))
    return ar_archive(
        [("debian-binary", b"2.0\n"), ("control.tar.gz", buffer.getvalue()), ("data.tar.gz", payload)]
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        root = work / "apt"
        debs = []
        for arch in ("amd64", "arm64"):
            deb = work / f"kintsu_0.2.0_{arch}.deb"
            deb.write_bytes(fake_deb("kintsu", "0.2.0", arch, payload=arch.encode()))
            debs.append(deb)
        when = datetime(2026, 9, 29, 12, 0, tzinfo=timezone.utc)
        packages = generate(root, debs, keep=3, now=when)
        assert {p.filename for p in packages} == {
            "pool/main/k/kintsu/kintsu_0.2.0_amd64.deb",
            "pool/main/k/kintsu/kintsu_0.2.0_arm64.deb",
        }, [p.filename for p in packages]
        stanza = (root / "dists/stable/main/binary-amd64/Packages").read_text()
        assert "Filename: pool/main/k/kintsu/kintsu_0.2.0_amd64.deb" in stanza, stanza
        assert "Depends" not in stanza, "an empty field is dropped"
        assert f"SHA256: {hashlib.sha256(debs[0].read_bytes()).hexdigest()}" in stanza
        assert stanza.rstrip().endswith("line two of the description"), stanza
        release = (root / "dists/stable/Release").read_text()
        assert "Architectures: amd64 arm64" in release, release
        assert "Date: Tue, 29 Sep 2026 12:00:00 GMT" in release, release
        packages_gz = (root / "dists/stable/main/binary-arm64/Packages.gz").read_bytes()
        listed = f" {hashlib.sha256(packages_gz).hexdigest()} {len(packages_gz)} main/binary-arm64/Packages.gz"
        assert listed in release, release
        assert gzip.decompress(packages_gz) == (root / "dists/stable/main/binary-arm64/Packages").read_bytes()
        assert not (root / "dists/stable/InRelease").exists(), "unsigned: no signature files"
        newer = work / "kintsu_0.3.0_amd64.deb"
        newer.write_bytes(fake_deb("kintsu", "0.3.0", "amd64"))
        packages = generate(root, [newer], keep=1, now=when)
        assert [p.version for p in packages if p.arch == "amd64"] == ["0.3.0"], "older versions beyond --keep go"
        assert not (root / "pool/main/k/kintsu/kintsu_0.2.0_amd64.deb").exists()
        assert (root / "pool/main/k/kintsu/kintsu_0.2.0_arm64.deb").exists(), "each architecture counts on its own"
        assert version_key("0.10.0") > version_key("0.9.1") > version_key("0.9.0")
    print("apt-repo self-test ok")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("debs", nargs="*", type=Path, help=".deb files to add to the pool")
    parser.add_argument("--root", type=Path, default=Path("docs/apt"))
    parser.add_argument("--suite", default="stable")
    parser.add_argument("--component", default="main")
    parser.add_argument("--origin", default="kintsu")
    parser.add_argument("--label", default="kintsu")
    parser.add_argument("--description", default="kintsu, the bubble under failed commands")
    parser.add_argument("--keep", type=int, default=3, help="versions kept per package and architecture")
    parser.add_argument("--sign-key", help="the gpg key that signs Release; unsigned when absent")
    parser.add_argument("--gpg", default="gpg")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        self_test()
        return 0
    if not args.debs and not args.root.exists():
        parser.error("nothing to do: no .deb given and no repository to rewrite")
    packages = generate(
        args.root,
        args.debs,
        suite=args.suite,
        component=args.component,
        origin=args.origin,
        label=args.label,
        description=args.description,
        keep=args.keep,
        sign_key=args.sign_key,
        gpg=args.gpg,
    )
    for package in packages:
        print(f"{package.name} {package.version} {package.arch}  {package.filename}")
    print(f"{'signed' if args.sign_key else 'unsigned'} repository at {args.root}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
