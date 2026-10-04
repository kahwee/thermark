#!/usr/bin/env python3
"""Verify published Thermark assets. Never publishes or edits remote state."""

import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import struct
import subprocess
import tarfile


def github(repo, path):
    return json.loads(subprocess.check_output(
        ['gh', 'api', f'repos/{repo}/{path}'], text=True
    ))


def verify(args):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', args.repo):
        raise ValueError('repository must be owner/name')
    if not re.fullmatch(r'v\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?', args.tag):
        raise ValueError('tag must be v<semantic-version>')
    if args.expected_commit and not re.fullmatch(r'[0-9a-f]{40}', args.expected_commit):
        raise ValueError('expected commit must be a full lowercase SHA')
    if args.smoke and (not args.font or not args.font.is_file()):
        raise ValueError('--smoke requires an existing --font file')

    version = args.tag[1:]
    release = github(args.repo, f'releases/tags/{args.tag}')
    if release['draft'] or release['tag_name'] != args.tag:
        raise ValueError('release is not published for the requested tag')
    obj = github(args.repo, f'git/ref/tags/{args.tag}')['object']
    for _ in range(4):
        if obj['type'] == 'commit':
            break
        if obj['type'] != 'tag':
            raise ValueError('tag does not resolve to a commit')
        obj = github(args.repo, f'git/tags/{obj["sha"]}')['object']
    if obj['type'] != 'commit':
        raise ValueError('tag nesting exceeds verification limit')
    if args.expected_commit and obj['sha'] != args.expected_commit:
        raise ValueError('published tag differs from the tested commit')

    assets = {asset['name']: asset for asset in release['assets']}
    expected = {
        f'thermark-{version}-{system}-{arch}-{variant}.tar.gz'
        for system in ['Linux', 'macOS']
        for arch in ['ARM64', 'X64']
        for variant in ['ble', 'full']
    }
    if len(assets) != len(release['assets']) or set(assets) != (
        expected | {name + '.sha256' for name in expected}
    ):
        raise ValueError('asset inventory differs from the eight-archive matrix')
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=True)

    def download(asset):
        name = asset['name']
        url = f'https://github.com/{args.repo}/releases/download/{args.tag}/{name}'
        if asset['browser_download_url'] != url:
            raise ValueError(f'unexpected download URL for {name}')
        path = root / name
        subprocess.run([
            'curl', '--fail', '--silent', '--show-error', '--location',
            '--output', str(path), url
        ], check=True)
        if path.stat().st_size != asset['size']:
            raise ValueError(f'asset size mismatch: {name}')
        digest = 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()
        if asset.get('digest') and digest != asset['digest']:
            raise ValueError(f'GitHub digest mismatch: {name}')

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(download, assets.values()))

    for name in sorted(expected):
        digest = hashlib.sha256((root / name).read_bytes()).hexdigest()
        sidecar = (root / (name + '.sha256')).read_text().split()
        if len(sidecar) != 2 or sidecar[1].lstrip('*') != name or sidecar[0] != digest:
            raise ValueError(f'sidecar checksum mismatch: {name}')
        with tarfile.open(root / name) as archive:
            stem = name.removesuffix('.tar.gz')
            members = archive.getmembers()
            expected_members = {stem, *(stem + '/' + file for file in [
                'thermark', 'README.md', 'LICENSE'
            ])}
            if len(members) != 4 or {member.name for member in members} != expected_members:
                raise ValueError(f'archive member inventory mismatch: {name}')
            if not archive.getmember(stem).isdir():
                raise ValueError(f'archive root is not a directory: {name}')
            if not all(archive.getmember(stem + '/' + file).isfile() for file in [
                'thermark', 'README.md', 'LICENSE'
            ]):
                raise ValueError(f'archive contains non-regular files: {name}')
            if not archive.getmember(stem + '/thermark').mode & 0o111:
                raise ValueError(f'archive binary is not executable: {name}')
        print('Verified:', name)

    if args.smoke:
        smoke(root, version, args.font.resolve())
    print(json.dumps({
        'release_url': release['html_url'], 'tag': args.tag, 'commit': obj['sha'],
        'verified_archives': len(expected), 'smoke_test': args.smoke,
        'prerelease': release['prerelease'],
    }))


def smoke(root, version, font):
    system = {'Linux': 'Linux', 'Darwin': 'macOS'}.get(platform.system())
    arch = {'x86_64': 'X64', 'amd64': 'X64', 'aarch64': 'ARM64', 'arm64': 'ARM64'}.get(
        platform.machine().lower()
    )
    if not system or not arch:
        raise ValueError('smoke test requires a supported Linux or macOS host')
    stem = f'thermark-{version}-{system}-{arch}-ble'
    installed = root / 'installed'
    installed.mkdir(exist_ok=True)
    with tarfile.open(root / (stem + '.tar.gz')) as archive:
        archive.extractall(installed, filter='data')
    binary = installed / stem / 'thermark'
    environment = os.environ.copy()
    environment.pop('THERMARK_ADDR', None)
    # Isolate every child command from the user's saved printer configuration.
    import tempfile
    with tempfile.TemporaryDirectory(prefix='thermark-release-smoke-') as directory:
        environment['THERMARK_CONFIG'] = str(Path(directory) / 'config.json')
        actual = subprocess.check_output([binary, '--version'], env=environment, text=True).strip()
        if actual != f'thermark {version}':
            raise ValueError('published binary version mismatch')
        for command in ['--help', 'tasks']:
            subprocess.run([binary, command], env=environment, check=True, stdout=subprocess.DEVNULL)
        png = root / 'first-label.png'
        subprocess.run([
            binary, 'qr', '--url', 'https://example.com', '--text', 'Hello, label!',
            '--model', 'b1', '--label', '50x30', '--font', font,
            '--save', png, '--no-print'
        ], env=environment, check=True)
        data = png.read_bytes()
        if data[:8] != b'\x89PNG\r\n\x1a\n' or struct.unpack('>II', data[16:24]) != (384, 240):
            raise ValueError('offline preview is not the expected 384x240 PNG')
        if Path(environment['THERMARK_CONFIG']).exists():
            raise ValueError('offline smoke test wrote printer configuration')
    print('Published host binary smoke test passed.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', default='kahwee/thermark')
    parser.add_argument('--tag', required=True)
    parser.add_argument('--expected-commit')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--smoke', action='store_true')
    parser.add_argument('--font', type=Path)
    args = parser.parse_args()
    try:
        verify(args)
    except (ValueError, subprocess.CalledProcessError, OSError, KeyError, struct.error) as error:
        parser.exit(1, f'Verification failed: {error}\n')


if __name__ == '__main__':
    main()
