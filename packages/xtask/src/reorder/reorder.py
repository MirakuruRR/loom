"""Reorders an enum's values while a Blueprint holding one is loaded, then saves that Blueprint, all
in one editor, for the examples' check that the value it holds follows the reorder. Run by the
pythonscript commandlet, with CONTENT, the folder /Game/Loom/Reorder is, and REORDERED, the enum's
reordered source, written above it."""

import json
import shutil

import unreal


def build():
    report = json.loads(unreal.LoomLibrary.build_blueprints(False))
    if not report["ok"]:
        raise RuntimeError(f"the build failed: {report['errors']}")
    return {b["path"]: b["status"] for b in report["blueprints"]}


build()
held = unreal.EditorAssetLibrary.load_asset("/Game/Loom/Reorder/BP_Order")
shutil.copy(REORDERED, f"{CONTENT}/E_Order.lm")  # noqa: F821, written above this script
statuses = build()
if statuses.get("/Game/Loom/Reorder/BP_Order") != "unchanged":
    raise RuntimeError(f"BP_Order was built again, so the check proves nothing: {statuses}")
if not unreal.EditorAssetLibrary.save_loaded_asset(held, False):
    raise RuntimeError("could not save BP_Order")
