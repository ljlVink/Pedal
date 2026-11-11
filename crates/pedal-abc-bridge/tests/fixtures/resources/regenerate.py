import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("restool", type=Path)
args = parser.parse_args()
destination = Path(__file__).resolve().parent


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


with tempfile.TemporaryDirectory(prefix="pedal-restool-") as temporary:
    root = Path(temporary)
    resources = root / "entry" / "resources"
    save(resources / "base/element/string.json", {"string": [
        {"name": "greeting", "value": "Hello \"Ark\"\n世界"},
        {"name": "empty", "value": ""},
        {"name": "reference", "value": "$string:greeting"},
    ]})
    save(resources / "zh_Hans_CN/element/string.json", {"string": [
        {"name": "greeting", "value": "你好，世界"},
    ]})
    save(resources / "dark/element/color.json", {"color": [{"name": "background", "value": "#000000"}]})
    save(resources / "base/element/color.json", {"color": [{"name": "background", "value": "#FFFFFF"}]})
    save(resources / "base/element/strarray.json", {"strarray": [{"name": "labels", "value": [
        {"value": "Alpha"}, {"value": "中文"}, {"value": ""}]}]})
    save(resources / "base/element/intarray.json", {"intarray": [{"name": "numbers", "value": [1, -2, 3]}]})
    save(resources / "base/element/plural.json", {"plural": [{"name": "items", "value": [
        {"quantity": "one", "value": "one item"}, {"quantity": "other", "value": "%d items"}]}]})
    save(resources / "base/element/integer.json", {"integer": [{"name": "answer", "value": 42}]})
    save(resources / "base/element/boolean.json", {"boolean": [{"name": "enabled", "value": True}]})
    save(resources / "base/element/float.json", {"float": [{"name": "spacing", "value": "12.5vp"}]})
    save(resources / "base/profile/pages.json", {"src": ["pages/Index"]})
    save(resources / "rawfile/data.json", {"raw": "原始内容"})
    media = resources / "base/media/icon.svg"
    media.parent.mkdir(parents=True, exist_ok=True)
    media.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8"/></svg>', encoding="utf-8")
    for version, api in [("v1", 12), ("v2", 20)]:
        descriptor = {"app": {"bundleName": "com.pedal.resources", "versionCode": 1,
            "versionName": "1.0.0", "minAPIVersion": api, "targetAPIVersion": api},
            "module": {"name": "entry", "type": "entry", "deviceTypes": ["phone"]}}
        config = root / "entry/module.json"
        save(config, descriptor)
        output = root / version
        output.mkdir()
        subprocess.run([str(args.restool), "-i", str(root / "entry"), "-j", str(config),
            "-p", "com.pedal.resources", "-o", str(output), "-r", str(root / f"{version}.txt")], check=True)
        index = output / "resources.index"
        (destination / f"{version}.index").write_bytes(index.read_bytes())
        package = root / f"{version}.hap"
        with zipfile.ZipFile(package, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(index, "resources.index")
            archive.write(config, "module.json")
        for config_only in [False, True]:
            command = [str(args.restool), "dump"] + (["config"] if config_only else []) + [str(package)]
            result = subprocess.run(command, check=True, capture_output=True, encoding="utf-8")
            save(destination / f"{version}{'.config' if config_only else ''}.json", json.loads(result.stdout))
    save(destination / "module.json", descriptor)
