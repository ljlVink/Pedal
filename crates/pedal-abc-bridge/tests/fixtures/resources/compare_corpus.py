import argparse
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("pedal", type=Path)
parser.add_argument("restool", type=Path)
parser.add_argument("corpus", type=Path)
parser.add_argument("--package", type=Path, action="append", help="Package path relative to corpus; repeat to select multiple")
args = parser.parse_args()


def normalize(value):
    for resource in value.get("resource", []):
        resource["entryValues"].sort(key=lambda item: json.dumps(item, sort_keys=True))
    if "config" in value:
        value["config"].sort()
    return value


if args.package:
    packages = [args.corpus / path for path in args.package]
else:
    packages = sorted(p for p in (args.corpus / "AppGallery").iterdir() if p.suffix in (".hap", ".hsp"))
    assert args.corpus / "AppGallery/entry.hap" in packages
    packages += [args.corpus / path for path in [
        "Hinote/HiNote.hap",
        "encrypted/1791634601426-357f36c5-48b7-494e-a2ad-32a21eb3c2ca.hap",
        "arkts_probe/entry/build/default/outputs/default/entry-default-unsigned.hap",
    ]]
total = 0
for package in packages:
    assert package.is_file(), package
    for config_only in [False, True]:
        suffix = ["dump"] + (["config"] if config_only else []) + [str(package)]
        actual = subprocess.run([str(args.pedal), "restool"] + suffix, capture_output=True, encoding="utf-8", check=True)
        expected = subprocess.run([str(args.restool)] + suffix, capture_output=True, encoding="utf-8", check=True)
        actual = normalize(json.loads(actual.stdout))
        expected = normalize(json.loads(expected.stdout))
        assert actual == expected, f"{package}, config={config_only}"
        if not config_only:
            total += sum(resource["entryCount"] for resource in actual["resource"])
    print(f"PASS {package}", flush=True)
print(f"Matched official restool: {len(packages)} packages, {total} resource variants, dump + config")
